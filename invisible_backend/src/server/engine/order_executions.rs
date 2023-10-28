use firestore_db_and_auth::ServiceSession;
use parking_lot::Mutex;
use serde_json::Value;
use std::thread::JoinHandle;
use std::{collections::HashMap, sync::Arc, thread::ThreadId};

use super::super::server_helpers::engine_helpers::{
    verify_notes_existence, verify_position_existence, verify_signature_format,
};
use super::super::{
    grpc::{
        engine_proto::{
            GrpcPerpPosition, LimitOrderMessage, LiquidationOrderMessage, LiquidationOrderResponse,
            OrderResponse, PerpOrderMessage,
        },
        GrpcMessage, GrpcTxResponse, MessageType,
    },
    server_helpers::{
        engine_helpers::store_output_json,
        get_market_id_and_order_side,
        perp_swap_execution::{
            process_and_execute_perp_swaps, process_perp_order_request, retry_failed_perp_swaps,
        },
        swap_execution::{
            handle_swap_execution_results, process_and_execute_spot_swaps,
            process_limit_order_request, retry_failed_swaps,
        },
        WsConnectionsMap, PERP_MARKET_IDS,
    },
};

use crate::perpetual::{perp_helpers::perp_rollback::PerpRollbackInfo, perp_order::PerpOrder};
use crate::server::server_helpers::engine_helpers::verify_tab_existence;
use crate::{
    matching_engine::{domain::OrderSide as OBOrderSide, orderbook::OrderBook},
    perpetual::{
        liquidations::{
            liquidation_engine::LiquidationSwap, liquidation_order::LiquidationOrder,
            liquidation_output::LiquidationResponse,
        },
        PositionEffectType,
    },
    trees::superficial_tree::SuperficialTree,
    utils::{
        errors::{send_liquidation_order_error_reply, PerpSwapExecutionError},
        storage::local_storage::{BackupStorage, MainStorage},
    },
};

use crate::transactions::{limit_order::LimitOrder, transaction_helpers::rollbacks::RollbackInfo};
use crate::utils::crypto_utils::Signature;
use crate::utils::errors::send_order_error_reply;

use error_stack::Report;
use tokio::sync::{
    mpsc::Sender as MpscSender,
    oneshot::{self, Sender as OneshotSender},
    Mutex as TokioMutex, Semaphore,
};
use tokio::task::JoinHandle as TokioJoinHandle;
use tonic::{Request, Response, Status};

//
// * ===================================================================================================================================
// * EXECUTE LIMIT ORDER

pub async fn submit_limit_order_inner(
    mpsc_tx: &MpscSender<(GrpcMessage, OneshotSender<GrpcTxResponse>)>,
    session: &Arc<Mutex<ServiceSession>>,
    main_storage: &Arc<Mutex<MainStorage>>,
    backup_storage: &Arc<Mutex<BackupStorage>>,
    swap_output_json: &Arc<Mutex<Vec<serde_json::Map<String, Value>>>>,
    state_tree: &Arc<Mutex<SuperficialTree>>,
    rollback_safeguard: &Arc<Mutex<HashMap<ThreadId, RollbackInfo>>>,
    order_books: &HashMap<u16, Arc<TokioMutex<OrderBook>>>,
    ws_connections: &Arc<TokioMutex<WsConnectionsMap>>,
    privileged_ws_connections: &Arc<TokioMutex<Vec<u64>>>,
    semaphore: &Semaphore,
    is_paused: &Arc<TokioMutex<bool>>,
    request: Request<LimitOrderMessage>,
) -> Result<Response<OrderResponse>, Status> {
    let _permit = semaphore.acquire().await.unwrap();

    let lock = is_paused.lock().await;
    drop(lock);

    tokio::task::yield_now().await;

    // let now = Instant::now();

    let req: LimitOrderMessage = request.into_inner();

    let user_id = req.user_id;
    let is_market: bool = req.is_market;

    // ? Verify the signature is defined and has a valid format
    let signature: Signature;
    match verify_signature_format(&req.signature) {
        Ok(sig) => signature = sig,
        Err(err) => {
            return send_order_error_reply(err);
        }
    }

    // ? Try to parse the grpc input as a LimitOrder
    let limit_order: LimitOrder;
    match LimitOrder::try_from(req) {
        Ok(lo) => limit_order = lo,
        Err(_e) => {
            return send_order_error_reply(
                "Error unpacking the limit order (verify the format is correct)".to_string(),
            );
        }
    };

    // ? Try to get the market_id and order_side from the limit_order
    let res = get_market_id_and_order_side(limit_order.token_spent, limit_order.token_received);
    if res.is_none() {
        return send_order_error_reply("Market (token pair) not found".to_string());
    }
    let (market_id, side) = res.unwrap();

    if limit_order.spot_note_info.is_some() {
        // ? Verify the notes spent exist in the state tree
        if let Err(err_msg) = verify_notes_existence(
            &limit_order.spot_note_info.as_ref().unwrap().notes_in,
            &state_tree,
        ) {
            return send_order_error_reply(err_msg);
        }
    } else {
        if limit_order.order_tab.is_none() {
            return send_order_error_reply(
                "Order tab is not defined for this limit order".to_string(),
            );
        }

        // ? Verify the order tab exist in the state tree
        if let Err(err_msg) =
            verify_tab_existence(&limit_order.order_tab.as_ref().unwrap(), &state_tree)
        {
            return send_order_error_reply(err_msg);
        }
    }

    // ? ------------------------------------------------------------------------------------
    // ? Insert the order into the orderbook and see if there is a hit
    let mut processed_res = process_limit_order_request(
        order_books.get(&market_id).clone().unwrap(),
        limit_order.clone(),
        side,
        signature.clone(),
        user_id,
        is_market,
        false,
        0,
        0,
        None,
    )
    .await;

    // ? ------------------------------------------------------------------------------------
    // ? If there are any hits, process and execute the swaps
    let reults;
    let new_order_id;
    match process_and_execute_spot_swaps(
        &mpsc_tx,
        &rollback_safeguard,
        order_books.get(&market_id).clone().unwrap(),
        &session,
        &backup_storage,
        &mut processed_res,
    )
    .await
    {
        Ok((h, oid)) => {
            reults = h;
            new_order_id = oid;
        }
        Err(err) => {
            return send_order_error_reply(err);
        }
    };

    // ? ------------------------------------------------------------------------------------
    // ? Handle the result of the swap executions
    let retry_messages;
    match handle_swap_execution_results(
        &ws_connections,
        &privileged_ws_connections,
        reults,
        user_id,
    )
    .await
    {
        Ok(rm) => retry_messages = rm,
        Err(e) => {
            return send_order_error_reply(e);
        }
    };

    // ? ------------------------------------------------------------------------------------
    // ? Retry the order in case it fails
    if retry_messages.len() > 0 {
        if let Err(e) = retry_failed_swaps(
            &mpsc_tx,
            &rollback_safeguard,
            order_books.get(&market_id).clone().unwrap(),
            &session,
            &backup_storage,
            limit_order,
            side,
            signature,
            user_id,
            is_market,
            &ws_connections,
            &privileged_ws_connections,
            retry_messages,
            None,
        )
        .await
        {
            return send_order_error_reply(e);
        }
    }

    store_output_json(&swap_output_json, &main_storage);

    // Send a successul reply to the caller
    let reply = OrderResponse {
        successful: true,
        error_message: "".to_string(),
        order_id: new_order_id,
    };

    return Ok(Response::new(reply));
}

//
// * ===================================================================================================================================
// * EXECUTE PERPETUAL ORDER

pub async fn submit_perpetual_order_inner(
    mpsc_tx: &MpscSender<(GrpcMessage, OneshotSender<GrpcTxResponse>)>,
    session: &Arc<Mutex<ServiceSession>>,
    main_storage: &Arc<Mutex<MainStorage>>,
    backup_storage: &Arc<Mutex<BackupStorage>>,
    swap_output_json: &Arc<Mutex<Vec<serde_json::Map<String, Value>>>>,
    state_tree: &Arc<Mutex<SuperficialTree>>,
    perp_rollback_safeguard: &Arc<Mutex<HashMap<ThreadId, PerpRollbackInfo>>>,
    perp_order_books: &HashMap<u16, Arc<TokioMutex<OrderBook>>>,
    ws_connections: &Arc<TokioMutex<WsConnectionsMap>>,
    privileged_ws_connections: &Arc<TokioMutex<Vec<u64>>>,
    semaphore: &Semaphore,
    is_paused: &Arc<TokioMutex<bool>>,
    request: Request<PerpOrderMessage>,
) -> Result<Response<OrderResponse>, Status> {
    let _permit = semaphore.acquire().await.unwrap();

    let lock = is_paused.lock().await;
    drop(lock);

    tokio::task::yield_now().await;

    let req: PerpOrderMessage = request.into_inner();

    let user_id = req.user_id;
    let is_market: bool = req.is_market;

    // ? Verify the signature is defined and has a valid format
    let signature: Signature;
    match verify_signature_format(&req.signature) {
        Ok(sig) => signature = sig,
        Err(err) => {
            return send_order_error_reply(err);
        }
    }

    // ? Try to parse the grpc input as a LimitOrder
    let perp_order: PerpOrder;
    match PerpOrder::try_from(req) {
        Ok(po) => perp_order = po,
        Err(_e) => {
            return send_order_error_reply(
                "Error unpacking the limit order (verify the format is correct)".to_string(),
            );
        }
    };

    // ? market for perpetuals can be just the synthetic token
    let market = PERP_MARKET_IDS.get(&perp_order.synthetic_token.to_string());
    if market.is_none() {
        return send_order_error_reply(
            "Market (token pair) does not exist for this token".to_string(),
        );
    }

    // ? Verify the notes spent and position modified exist in the state tree
    if perp_order.position_effect_type == PositionEffectType::Open {
        if let Err(err_msg) = verify_notes_existence(
            &perp_order.open_order_fields.as_ref().unwrap().notes_in,
            &state_tree,
        ) {
            return send_order_error_reply(err_msg);
        }
    } else {
        if let Err(err_msg) =
            verify_position_existence(&perp_order.position.as_ref().unwrap(), &state_tree)
        {
            return send_order_error_reply(err_msg);
        }
    }

    let side: OBOrderSide = perp_order.order_side.clone().into();

    let mut processed_res = process_perp_order_request(
        perp_order_books.get(&market.unwrap()).clone().unwrap(),
        perp_order.clone(),
        side,
        signature.clone(),
        user_id,
        is_market,
        false,
        0,
        0,
        None,
    )
    .await;

    // This matches the orders and creates the swaps that can be executed
    let retry_messages;
    let new_order_id;
    match process_and_execute_perp_swaps(
        &mpsc_tx,
        &perp_rollback_safeguard,
        perp_order_books.get(&market.unwrap()).clone().unwrap(),
        &session,
        &backup_storage,
        &ws_connections,
        &privileged_ws_connections,
        &mut processed_res,
        user_id,
    )
    .await
    {
        Ok((h, oid)) => {
            retry_messages = h;
            new_order_id = oid;
        }
        Err(err) => {
            return send_order_error_reply(err);
        }
    };

    if let Err(e) = retry_failed_perp_swaps(
        &mpsc_tx,
        &perp_rollback_safeguard,
        perp_order_books.get(&market.unwrap()).clone().unwrap(),
        &session,
        &backup_storage,
        perp_order,
        side,
        signature,
        user_id,
        is_market,
        &ws_connections,
        &privileged_ws_connections,
        retry_messages,
        None,
    )
    .await
    {
        return send_order_error_reply(e);
    }

    store_output_json(&swap_output_json, &main_storage);

    // Send a successful reply to the caller
    let reply = OrderResponse {
        successful: true,
        error_message: "".to_string(),
        order_id: new_order_id,
    };

    return Ok(Response::new(reply));
}

//
// * ===================================================================================================================================
//

pub async fn submit_liquidation_order_inner(
    mpsc_tx: &MpscSender<(GrpcMessage, OneshotSender<GrpcTxResponse>)>,
    main_storage: &Arc<Mutex<MainStorage>>,
    swap_output_json: &Arc<Mutex<Vec<serde_json::Map<String, Value>>>>,
    perp_order_books: &HashMap<u16, Arc<TokioMutex<OrderBook>>>,
    semaphore: &Semaphore,
    is_paused: &Arc<TokioMutex<bool>>,
    request: Request<LiquidationOrderMessage>,
) -> Result<Response<LiquidationOrderResponse>, Status> {
    let _permit = semaphore.acquire().await.unwrap();

    let lock = is_paused.lock().await;
    drop(lock);

    tokio::task::yield_now().await;

    let req: LiquidationOrderMessage = request.into_inner();

    // ? Verify the signature is defined and has a valid format
    let signature: Signature;
    match verify_signature_format(&req.signature) {
        Ok(sig) => signature = sig,
        Err(err) => {
            return send_liquidation_order_error_reply(err);
        }
    }

    // ? Try to parse the grpc input as a LimitOrder
    let liquidation_order: LiquidationOrder;
    match LiquidationOrder::try_from(req) {
        Ok(lo) => liquidation_order = lo,
        Err(_e) => {
            return send_liquidation_order_error_reply(
                "Error unpacking the liquidation order (verify the format is correct)".to_string(),
            );
        }
    };

    // ? market for perpetuals can be just the synthetic token
    let market = PERP_MARKET_IDS.get(&liquidation_order.synthetic_token.to_string());
    if market.is_none() {
        return send_liquidation_order_error_reply(
            "Market (token pair) does not exist for this token".to_string(),
        );
    }

    let mut perp_orderbook = perp_order_books
        .get(&market.unwrap())
        .clone()
        .unwrap()
        .lock()
        .await;
    let market_price;
    match perp_orderbook.get_market_price() {
        Ok(mp) => market_price = mp,
        Err(e) => {
            return send_liquidation_order_error_reply(e);
        }
    };
    drop(perp_orderbook);

    let liquidation_swap = LiquidationSwap::new(liquidation_order, signature, market_price);

    // TODO ==================================================================================

    let transaction_mpsc_tx = mpsc_tx.clone();

    let handle: TokioJoinHandle<
        JoinHandle<Result<LiquidationResponse, Report<PerpSwapExecutionError>>>,
    > = tokio::spawn(async move {
        let (resp_tx, resp_rx) = oneshot::channel();

        let mut grpc_message = GrpcMessage::new();
        grpc_message.msg_type = MessageType::LiquidationMessage;
        grpc_message.liquidation_message = Some(liquidation_swap);

        transaction_mpsc_tx
            .send((grpc_message, resp_tx))
            .await
            .ok()
            .unwrap();
        let res = resp_rx.await.unwrap();

        return res.liquidation_tx_handle.unwrap();
    });

    let liquidation_handle = handle.await.unwrap();

    let liquidation_response = liquidation_handle.join();

    match liquidation_response {
        Ok(res1) => match res1 {
            Ok(response) => {
                store_output_json(&swap_output_json, &main_storage);

                // TODO Send message to the user whose position was liquidated ?

                println!("Position liquidated successfully!!!!!!!!!\n");

                let reply = LiquidationOrderResponse {
                    successful: true,
                    error_message: "".to_string(),
                    new_position: Some(GrpcPerpPosition::from(response.new_position)),
                };

                return Ok(Response::new(reply));
            }
            Err(err) => {
                let error_message_response: String = err.current_context().err_msg.to_string();

                println!("Position liquidation failed {:?}\n", error_message_response);

                return send_liquidation_order_error_reply(error_message_response);
            }
        },
        Err(_e) => {
            println!("Unknown Error in liquidation execution");

            return send_liquidation_order_error_reply(
                "Unknown Error occurred in the liquidation execution".to_string(),
            );
        }
    }
}
