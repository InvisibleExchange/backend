use invisible_backend::server::{
    grpc::engine_proto::engine_server::EngineServer,
    server_helpers::periodic_updates::start_periodic_updates,
};
use invisible_backend::transaction_batch::batch_functions::batch_transition::TREE_DEPTH;
use invisible_backend::transaction_batch::TransactionBatch;

use std::{collections::HashMap, net::SocketAddr, sync::Arc};
use tokio::net::TcpListener;

use invisible_backend::server::{
    engine::EngineService,
    server_helpers::{handle_connection, init_order_books, WsConnectionsMap},
};

use tokio::sync::{Mutex as TokioMutex, Semaphore};
use tonic::transport::Server;

// use engine_proto::engine_server::EngineServer;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // env_logger::init();

    // * ======================================================================

    let mut tx_batch = TransactionBatch::new(TREE_DEPTH);
    tx_batch.init();

    // TODO: TESTING ==========================================================
    println!("\nstate tree: {:?}", tx_batch.state_tree.lock().leaf_nodes);

    // println!("funding rates: {:?}", tx_batch.funding_rates);
    // println!("funding prices: {:?}", tx_batch.funding_prices);

    // TODO: TESTING ==========================================================

    let transaction_batch = Arc::new(TokioMutex::new(tx_batch));

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
        &transaction_batch,
        &order_books,
        &perp_order_books,
        &ws_conn_mutex,
        &privileged_ws_connections,
    )
    .await;

    let transaction_service = EngineService {
        transaction_batch,
        order_books,
        perp_order_books,
        ws_connections,
        privileged_ws_connections,
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
