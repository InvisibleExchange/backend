use invisible_backend::server::grpc::MarginChangeResponse;
use invisible_backend::server::server_helpers::periodic_updates::start_periodic_updates;
use invisible_backend::transaction_batch::batch_functions::batch_transition::TREE_DEPTH;
use invisible_backend::transaction_batch::TransactionBatch;
use parking_lot::Mutex;
use std::{collections::HashMap, net::SocketAddr, sync::Arc, thread::ThreadId};
use tokio::net::TcpListener;

use invisible_backend::perpetual::perp_helpers::perp_rollback::PerpRollbackInfo;
use invisible_backend::server::{
    engine::EngineService,
    grpc::{engine_proto, GrpcMessage, GrpcTxResponse, MessageType},
    server_helpers::{handle_connection, init_order_books, WsConnectionsMap},
};

use invisible_backend::transactions::transaction_helpers::rollbacks::RollbackInfo;

use tokio::sync::{mpsc, oneshot, Mutex as TokioMutex, Semaphore};
use tonic::transport::Server;

use engine_proto::engine_server::EngineServer;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // env_logger::init();

    // * =============================================================================================================================

    // ? A channel between the state_update thread and all the client connection threads that open
    let (mpsc_tx, mut transaction_mpsc_rx) =
        mpsc::channel::<(GrpcMessage, oneshot::Sender<GrpcTxResponse>)>(100);

    // ? A map shared between threads that checks if the state has been updated before an error occured and requires a rollback
    let rollback_safeguard: Arc<Mutex<HashMap<ThreadId, RollbackInfo>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let perp_rollback_safeguard: Arc<Mutex<HashMap<ThreadId, PerpRollbackInfo>>> =
        Arc::new(Mutex::new(HashMap::new()));

    let mut tx_batch = TransactionBatch::new(
        TREE_DEPTH,
        rollback_safeguard.clone(),
        perp_rollback_safeguard.clone(),
    );
    tx_batch.init();

    // TODO: TESTING ==========================================================
    // println!("\nstate tree: {:?}", tx_batch.state_tree.lock().leaf_nodes);

    // println!("zero indexes: {:?}", tx_batch.state_tree.lock().zero_idxs);

    // let state_root = build_tree(TREE_DEPTH, &tx_batch.state_tree.lock().leaf_nodes, 0);
    // println!("state root: {:?}", state_root);

    println!("funding rates: {:?}", tx_batch.funding_rates);
    println!("funding prices: {:?}", tx_batch.funding_prices);
    println!("current_funding_idx: {:?}", tx_batch.current_funding_idx);

    // TODO: TESTING ==========================================================

    let session = Arc::clone(&tx_batch.firebase_session);
    let main_storage = Arc::clone(&tx_batch.main_storage);
    let backup_storage = Arc::clone(&tx_batch.backup_storage);
    let swap_output_json = Arc::clone(&tx_batch.swap_output_json);

    let state_tree = Arc::clone(&tx_batch.state_tree);

    let partial_fill_tracker = Arc::clone(&tx_batch.partial_fill_tracker);
    let perpetual_partial_fill_tracker = Arc::clone(&tx_batch.perpetual_partial_fill_tracker);

    // ? Spawn a thread to handle the state update
    tokio::spawn(async move {
        // ? This gets the request from the client_connection thread and sends a response back
        while let Some((grpc_message, response)) = transaction_mpsc_rx.recv().await {
            // ? This should perform the state update and send a response back to the client_connection thread

            match grpc_message.msg_type {
                MessageType::DepositMessage => {
                    let handle =
                        tx_batch.execute_transaction(grpc_message.deposit_message.unwrap());

                    let mut grpc_res = GrpcTxResponse::new(true);
                    grpc_res.tx_handle = Some(handle);

                    response
                        .send(grpc_res)
                        .expect("failed sending back the TxResponse in deposit");
                }
                MessageType::SwapMessage => {
                    let handle = tx_batch.execute_transaction(grpc_message.swap_message.unwrap());

                    let mut grpc_res = GrpcTxResponse::new(true);
                    grpc_res.tx_handle = Some(handle);

                    response
                        .send(grpc_res)
                        .expect("failed sending back the TxResponse in swap");
                }
                MessageType::WithdrawalMessage => {
                    let handle =
                        tx_batch.execute_transaction(grpc_message.withdrawal_message.unwrap());

                    let mut grpc_res = GrpcTxResponse::new(true);
                    grpc_res.tx_handle = Some(handle);

                    response
                        .send(grpc_res)
                        .expect("failed sending back the TxResponse in withdrawal");
                }
                MessageType::PerpSwapMessage => {
                    let handle = tx_batch
                        .execute_perpetual_transaction(grpc_message.perp_swap_message.unwrap());

                    let mut grpc_res = GrpcTxResponse::new(true);
                    grpc_res.perp_tx_handle = Some(handle);

                    response
                        .send(grpc_res)
                        .expect("failed sending back the TxResponse in perp swap");
                }
                MessageType::LiquidationMessage => {
                    let handle = tx_batch
                        .execute_liquidation_transaction(grpc_message.liquidation_message.unwrap());

                    let mut grpc_res = GrpcTxResponse::new(true);
                    grpc_res.liquidation_tx_handle = Some(handle);

                    response
                        .send(grpc_res)
                        .expect("failed sending back the TxResponse in perp swap");
                }
                MessageType::SplitNotes => {
                    let (notes_in, new_note, refund_note) =
                        grpc_message.split_notes_message.unwrap();
                    let zero_idxs = tx_batch.split_notes(notes_in, new_note, refund_note);

                    let mut grpc_res = GrpcTxResponse::new(true);
                    grpc_res.new_idxs = Some(zero_idxs);

                    response
                        .send(grpc_res)
                        .expect("failed sending back the TxResponse in split notes");
                }
                MessageType::MarginChange => {
                    let result = tx_batch
                        .change_position_margin(grpc_message.change_margin_message.unwrap());

                    let success: bool;
                    let margin_change_response: Option<(Option<MarginChangeResponse>, String)>;

                    match result {
                        Ok((new_idxs, position)) => {
                            success = true;

                            margin_change_response = Some((
                                Some(MarginChangeResponse {
                                    new_note_idx: new_idxs,
                                    position,
                                }),
                                "".to_string(),
                            ));
                        }
                        Err(e) => {
                            success = false;
                            margin_change_response = Some((None, e));
                        }
                    }

                    let mut grpc_res = GrpcTxResponse::new(success);
                    grpc_res.margin_change_response = margin_change_response;

                    response
                        .send(grpc_res)
                        .expect("failed sending back the TxResponse in margin change");
                }
                MessageType::OrderTabAction => {
                    let result = tx_batch.execute_order_tab_modification(
                        grpc_message.order_tab_action_message.unwrap(),
                    );

                    let mut grpc_res = GrpcTxResponse::new(true);
                    grpc_res.order_tab_action_response = Some(result);

                    response
                        .send(grpc_res)
                        .expect("failed sending back the TxResponse in order tab action");
                }
                MessageType::Rollback => {
                    tx_batch.rollback_transaction(grpc_message.rollback_info_message.unwrap());

                    let grpc_res = GrpcTxResponse::new(true);

                    response
                        .send(grpc_res)
                        .expect("failed sending back the TxResponse in rollback");
                }
                MessageType::FundingUpdate => {
                    if let Some(funding_update) = grpc_message.funding_update_message {
                        tx_batch.per_minute_funding_updates(funding_update);

                        let grpc_res = GrpcTxResponse::new(true);

                        response
                            .send(grpc_res)
                            .expect("failed sending back the TxResponse in funding update");
                    } else {
                        let funding_rates = tx_batch.funding_rates.clone();
                        let funding_prices = tx_batch.funding_prices.clone();

                        let mut grpc_res = GrpcTxResponse::new(true);
                        grpc_res.funding_info = Some((funding_rates, funding_prices));

                        response
                            .send(grpc_res)
                            .expect("failed sending back the TxResponse in funding update");
                    }
                }
                MessageType::IndexPriceUpdate => {
                    let oracle_updates = grpc_message.price_update_message.unwrap();

                    let updated_prices = tx_batch.update_index_prices(oracle_updates);

                    let grpc_res = GrpcTxResponse::new(updated_prices.is_ok());

                    response
                        .send(grpc_res)
                        .expect("failed sending back the TxResponse in funding update");
                }
                MessageType::FinalizeBatch => {
                    let success = tx_batch.finalize_batch().is_ok();

                    let grpc_res = GrpcTxResponse::new(success);

                    response
                        .send(grpc_res)
                        .expect("failed sending back the TxResponse in finalize batch");
                }

                MessageType::Undefined => {
                    println!("Undefined message type");
                }
            }
        }
    });

    // ? Spawn the server
    let addr: SocketAddr = "0.0.0.0:50052".parse()?;

    println!("Listening on {:?}", addr);

    // * =============================================================================================================================

    let (order_books, perp_order_books) = init_order_books();

    let privileged_ws_connections: Arc<TokioMutex<Vec<u64>>> =
        Arc::new(TokioMutex::new(Vec::new()));

    let ws_addr: SocketAddr = "0.0.0.0:50053".parse()?;
    println!("Listening for updates on {:?}", ws_addr);

    // Create the event loop and TCP listener we'll accept connections on.
    let try_socket = TcpListener::bind(&ws_addr).await;
    let listener = try_socket.expect("Failed to bind");

    let ws_connection_i: WsConnectionsMap = HashMap::new();
    let ws_connections = Arc::new(TokioMutex::new(ws_connection_i));

    let ws_conn_mutex = ws_connections.clone();

    let privileged_ws_connections_ = privileged_ws_connections.clone();

    // Handle incoming websocket connections
    tokio::spawn(async move {
        loop {
            let ws_conn_ = ws_conn_mutex.clone();
            let privileged_ws_connections_ = privileged_ws_connections_.clone();

            let (stream, _addr) = listener.accept().await.expect("accept failed");

            tokio::spawn(handle_connection(
                stream,
                ws_conn_,
                privileged_ws_connections_,
            ));
        }
    });

    let ws_conn_mutex = ws_connections.clone();
    // ? Start periodic updates
    start_periodic_updates(
        &order_books,
        &perp_order_books,
        &mpsc_tx,
        &session,
        &ws_conn_mutex,
        &privileged_ws_connections,
        &backup_storage,
        &state_tree,
    )
    .await;

    let transaction_service = EngineService {
        mpsc_tx,
        session,
        state_tree,
        partial_fill_tracker,
        perpetual_partial_fill_tracker,
        rollback_safeguard,
        perp_rollback_safeguard,
        order_books,
        perp_order_books,
        ws_connections,
        privileged_ws_connections,
        main_storage,
        backup_storage,
        swap_output_json,
        semaphore: Semaphore::new(25),
        is_paused: Arc::new(TokioMutex::new(false)),
    };

    // * =============================================================================================================================

    Server::builder()
        .concurrency_limit_per_connection(128)
        .add_service(EngineServer::new(transaction_service))
        .serve(addr)
        .await?;

    Ok(())
}

// =================================================================================================
