use std::{collections::HashMap, sync::Arc};

use super::super::server_helpers::engine_helpers::store_output_json;
use crate::matching_engine::orderbook::OrderBook;
use crate::perpetual::perp_position::PerpPosition;
use crate::perpetual::COLLATERAL_TOKEN;
use crate::server::grpc::engine_proto::{
    GrpcPerpPosition, OnChainAddLiqReq, OnChainCloseMmReq, OnChainRegisterMmReq,
    OnChainRemoveLiqReq, OnChainScmmRes,
};
use crate::server::grpc::SCMMActionMessage;
use crate::transaction_batch::TransactionBatch;
use crate::utils::errors::send_regster_mm_error_reply;
use crate::utils::storage::local_storage::MainStorage;

use parking_lot::Mutex;
use tokio::sync::{Mutex as TokioMutex, Semaphore};
use tonic::{Response, Status};

pub async fn register_onchain_mm_inner(
    tx_batch: &Arc<TokioMutex<TransactionBatch>>,
    perp_order_books: &HashMap<u16, Arc<TokioMutex<OrderBook>>>,
    semaphore: &Semaphore,
    is_paused: &Arc<TokioMutex<bool>>,
    //
    req: OnChainRegisterMmReq,
) -> Result<Response<OnChainScmmRes>, Status> {
    let _permit = semaphore.acquire().await.unwrap();

    let lock = is_paused.lock().await;
    drop(lock);

    tokio::task::yield_now().await;

    if let Err(err) = verify_request(
        &perp_order_books,
        req.position.clone(),
        req.market_id,
        req.synthetic_token,
    )
    .await
    {
        return send_regster_mm_error_reply(err);
    }

    let tx_batch_m = tx_batch.lock().await;
    let swap_output_json = Arc::clone(&tx_batch_m.swap_output_json);
    let main_storage = Arc::clone(&tx_batch_m.main_storage);
    drop(tx_batch_m);

    let scmm_action_message = SCMMActionMessage {
        onchain_add_liq_req: None,
        onchain_register_mm_req: Some(req),
        onchain_remove_liq_req: None,
        onchain_close_mm_req: None,
    };

    let mut tx_batch_m = tx_batch.lock().await;
    let order_action_handle = tx_batch_m.execute_sc_mm_modification_inner(scmm_action_message);
    drop(tx_batch_m);

    let order_action_response = order_action_handle.join();

    return return_result(order_action_response, swap_output_json, main_storage);
}

pub async fn add_liquidity_mm_inner(
    tx_batch: &Arc<TokioMutex<TransactionBatch>>,
    perp_order_books: &HashMap<u16, Arc<TokioMutex<OrderBook>>>,
    semaphore: &Semaphore,
    is_paused: &Arc<TokioMutex<bool>>,
    //
    req: OnChainAddLiqReq,
) -> Result<Response<OnChainScmmRes>, Status> {
    let _permit = semaphore.acquire().await.unwrap();

    let lock = is_paused.lock().await;
    drop(lock);

    tokio::task::yield_now().await;

    if let Err(err) = verify_request(
        &perp_order_books,
        req.position.clone(),
        req.market_id,
        req.synthetic_token,
    )
    .await
    {
        return send_regster_mm_error_reply(err);
    }

    let tx_batch_m = tx_batch.lock().await;
    let swap_output_json = Arc::clone(&tx_batch_m.swap_output_json);
    let main_storage = Arc::clone(&tx_batch_m.main_storage);
    drop(tx_batch_m);

    let scmm_action_message = SCMMActionMessage {
        onchain_add_liq_req: Some(req),
        onchain_register_mm_req: None,
        onchain_remove_liq_req: None,
        onchain_close_mm_req: None,
    };

    let mut tx_batch_m = tx_batch.lock().await;
    let order_action_handle = tx_batch_m.execute_sc_mm_modification_inner(scmm_action_message);
    drop(tx_batch_m);

    let order_action_response = order_action_handle.join();

    return return_result(order_action_response, swap_output_json, main_storage);
}

pub async fn remove_liquidity_mm_inner(
    tx_batch: &Arc<TokioMutex<TransactionBatch>>,
    perp_order_books: &HashMap<u16, Arc<TokioMutex<OrderBook>>>,
    semaphore: &Semaphore,
    is_paused: &Arc<TokioMutex<bool>>,
    //
    req: OnChainRemoveLiqReq,
) -> Result<Response<OnChainScmmRes>, Status> {
    let _permit = semaphore.acquire().await.unwrap();

    let lock = is_paused.lock().await;
    drop(lock);

    tokio::task::yield_now().await;

    if let Err(err) = verify_request(
        &perp_order_books,
        req.position.clone(),
        req.market_id,
        req.synthetic_token,
    )
    .await
    {
        return send_regster_mm_error_reply(err);
    }

    let tx_batch_m = tx_batch.lock().await;
    let swap_output_json = Arc::clone(&tx_batch_m.swap_output_json);
    let main_storage = Arc::clone(&tx_batch_m.main_storage);
    drop(tx_batch_m);

    let scmm_action_message = SCMMActionMessage {
        onchain_add_liq_req: None,
        onchain_register_mm_req: None,
        onchain_remove_liq_req: Some(req),
        onchain_close_mm_req: None,
    };

    let mut tx_batch_m = tx_batch.lock().await;
    let order_action_handle = tx_batch_m.execute_sc_mm_modification_inner(scmm_action_message);
    drop(tx_batch_m);

    let order_action_response = order_action_handle.join();

    return return_result(order_action_response, swap_output_json, main_storage);
}

pub async fn close_onchain_mm_inner(
    tx_batch: &Arc<TokioMutex<TransactionBatch>>,
    perp_order_books: &HashMap<u16, Arc<TokioMutex<OrderBook>>>,
    semaphore: &Semaphore,
    is_paused: &Arc<TokioMutex<bool>>,
    //
    req: OnChainCloseMmReq,
) -> Result<Response<OnChainScmmRes>, Status> {
    let _permit = semaphore.acquire().await.unwrap();

    let lock = is_paused.lock().await;
    drop(lock);

    tokio::task::yield_now().await;

    if let Err(err) = verify_request(
        &perp_order_books,
        req.position.clone(),
        req.market_id,
        req.synthetic_token,
    )
    .await
    {
        return send_regster_mm_error_reply(err);
    }

    let tx_batch_m = tx_batch.lock().await;
    let swap_output_json = Arc::clone(&tx_batch_m.swap_output_json);
    let main_storage = Arc::clone(&tx_batch_m.main_storage);
    drop(tx_batch_m);

    let scmm_action_message = SCMMActionMessage {
        onchain_add_liq_req: None,
        onchain_register_mm_req: None,
        onchain_remove_liq_req: None,
        onchain_close_mm_req: Some(req),
    };

    let mut tx_batch_m = tx_batch.lock().await;
    let order_action_handle = tx_batch_m.execute_sc_mm_modification_inner(scmm_action_message);
    drop(tx_batch_m);

    let order_action_response = order_action_handle.join();

    return return_result(order_action_response, swap_output_json, main_storage);
}

// * ================================================================================================

async fn verify_request(
    perp_order_books: &HashMap<u16, Arc<TokioMutex<OrderBook>>>,
    req_position: Option<GrpcPerpPosition>,
    req_market_id: u32,
    req_synthetic_token: u32,
) -> Result<(), String> {
    if req_position.is_none() {
        return Err("Position is undefined".to_string());
    }
    let pos_header = &req_position.as_ref().unwrap().position_header;

    let synthetic_token = pos_header.as_ref().unwrap().synthetic_token;
    // ? Verify the market_id exists

    if !perp_order_books.contains_key(&(req_market_id as u16)) {
        return Err("No market found for given base and quote token".to_string());
    }

    // ? Get the relevant orderbook from the market_id
    let order_book = perp_order_books
        .get(&(req_market_id as u16))
        .unwrap()
        .lock()
        .await;

    // ? Verify the base and quote asset match the market_id
    if order_book.order_asset != synthetic_token
        || order_book.price_asset != COLLATERAL_TOKEN
        || synthetic_token != req_synthetic_token
    {
        return Err("Base and quote asset do not match market_id".to_string());
    }

    return Ok(());
}

fn return_result(
    order_action_response: Result<
        Result<PerpPosition, String>,
        Box<dyn std::any::Any + std::marker::Send>,
    >,
    swap_output_json: Arc<Mutex<Vec<serde_json::Map<String, serde_json::Value>>>>,
    main_storage: Arc<Mutex<MainStorage>>,
) -> Result<Response<OnChainScmmRes>, Status> {
    match order_action_response {
        Ok(res) => match res {
            Ok(position) => {
                store_output_json(&swap_output_json, &main_storage);

                let position = GrpcPerpPosition::from(position);
                let reply = OnChainScmmRes {
                    successful: true,
                    error_message: "".to_string(),
                    position: Some(position),
                };

                return Ok(Response::new(reply));
            }
            Err(err) => {
                println!("Error in smart contract mm update execution: {}", err);

                return send_regster_mm_error_reply(
                    "Error occurred in smart contract mm update execution".to_string() + &err,
                );
            }
        },
        Err(_e) => {
            println!("Unknown Error in smart contract mm update execution");

            return send_regster_mm_error_reply(
                "Unknown Error occurred in smart contract mm update execution".to_string(),
            );
        }
    }
}
