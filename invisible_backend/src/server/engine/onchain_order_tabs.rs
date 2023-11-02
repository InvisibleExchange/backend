use std::{collections::HashMap, sync::Arc};

use super::super::grpc::engine_proto::GrpcOrderTab;
use super::super::grpc::OrderTabActionMessage;
use super::super::server_helpers::engine_helpers::store_output_json;
use crate::matching_engine::orderbook::OrderBook;
use crate::perpetual::COLLATERAL_TOKEN;
use crate::server::grpc::engine_proto::{
    AddLiqOrderTabRes, GrpcNote, GrpcPerpPosition, OnChainAddLiqTabReq, OnChainRegisterMmReq,
    OnChainRegisterMmRes, OnChainRemoveLiqTabReq, PositionRemoveLiqRes, RemoveLiqOrderTabRes,
    TabRemoveLiqRes,
};
use crate::transaction_batch::TransactionBatch;
use crate::utils::errors::{
    send_add_liq_tab_error_reply, send_regster_mm_error_reply, send_remove_liq_tab_error_reply,
};

use tokio::sync::{Mutex as TokioMutex, Semaphore};
use tonic::{Request, Response, Status};

pub async fn onchain_register_mm_inner(
    tx_batch: &Arc<TokioMutex<TransactionBatch>>,
    order_books: &HashMap<u16, Arc<TokioMutex<OrderBook>>>,
    perp_order_books: &HashMap<u16, Arc<TokioMutex<OrderBook>>>,
    semaphore: &Semaphore,
    is_paused: &Arc<TokioMutex<bool>>,
    //
    req: Request<OnChainRegisterMmReq>,
) -> Result<Response<OnChainRegisterMmRes>, Status> {
    let _permit = semaphore.acquire().await.unwrap();

    let lock = is_paused.lock().await;
    drop(lock);

    tokio::task::yield_now().await;

    let req: OnChainRegisterMmReq = req.into_inner();

    let is_perp = req.position.is_some();

    let base_token;
    let quote_token;
    if req.order_tab.is_some() {
        if req.order_tab.is_none() {
            return send_regster_mm_error_reply("Order tab is undefined".to_string());
        }
        let tab_header = &req.order_tab.as_ref().unwrap().tab_header;

        base_token = tab_header.as_ref().unwrap().base_token;
        quote_token = tab_header.as_ref().unwrap().quote_token;
    } else {
        if req.position.is_none() {
            return send_regster_mm_error_reply("Position is undefined".to_string());
        }
        let pos_header = &req.position.as_ref().unwrap().position_header;

        base_token = pos_header.as_ref().unwrap().synthetic_token;
        quote_token = COLLATERAL_TOKEN;
    }

    // ? Verify the market_id exists
    let order_book;
    if is_perp {
        if !perp_order_books.contains_key(&(req.market_id as u16)) {
            return send_regster_mm_error_reply(
                "No market found for given base and quote token".to_string(),
            );
        }

        // ? Get the relevant orderbook from the market_id
        order_book = perp_order_books
            .get(&(req.market_id as u16))
            .unwrap()
            .lock()
            .await;
    } else {
        if !order_books.contains_key(&(req.market_id as u16)) {
            return send_regster_mm_error_reply(
                "No market found for given base and quote token".to_string(),
            );
        }

        // ? Get the relevant orderbook from the market_id
        order_book = order_books
            .get(&(req.market_id as u16))
            .unwrap()
            .lock()
            .await;
    }

    // ? Verify the base and quote asset match the market_id
    if order_book.order_asset != base_token
        || order_book.price_asset != quote_token
        || base_token != req.base_token
    {
        return send_regster_mm_error_reply(
            "Base and quote asset do not match market_id".to_string(),
        );
    }

    let tx_batch_m = tx_batch.lock().await;
    let swap_output_json = Arc::clone(&tx_batch_m.swap_output_json);
    let main_storage = Arc::clone(&tx_batch_m.main_storage);
    drop(tx_batch_m);

    let tab_action_message = OrderTabActionMessage {
        open_order_tab_req: None,
        close_order_tab_req: None,
        onchain_add_liq_req: None,
        onchain_register_mm_req: Some(req),
        onchain_remove_liq_req: None,
    };

    let mut tx_batch_m = tx_batch.lock().await;
    let order_action_handle = tx_batch_m.execute_order_tab_modification(tab_action_message);
    drop(tx_batch_m);

    let order_action_response = order_action_handle.join();

    match order_action_response {
        Ok(res) => match res.register_mm_response.unwrap() {
            Ok((order_tab, position, vlp_note)) => {
                store_output_json(&swap_output_json, &main_storage);

                let order_tab = order_tab.map(|tab| GrpcOrderTab::from(tab));
                let position = position.map(|pos| GrpcPerpPosition::from(pos));
                let vlp_note = GrpcNote::from(vlp_note);
                let reply = OnChainRegisterMmRes {
                    successful: true,
                    error_message: "".to_string(),
                    order_tab,
                    position,
                    vlp_note: Some(vlp_note),
                };

                return Ok(Response::new(reply));
            }
            Err(err) => {
                println!("Error in open order tab execution: {}", err);

                return send_regster_mm_error_reply(
                    "Error occurred in the register mm execution".to_string() + &err,
                );
            }
        },
        Err(_e) => {
            println!("Unknown Error in open order tab execution");

            return send_regster_mm_error_reply(
                "Unknown Error occurred in the register mm execution".to_string(),
            );
        }
    }
}

pub async fn add_liquidity_mm_inner(
    tx_batch: &Arc<TokioMutex<TransactionBatch>>,
    order_books: &HashMap<u16, Arc<TokioMutex<OrderBook>>>,
    perp_order_books: &HashMap<u16, Arc<TokioMutex<OrderBook>>>,
    semaphore: &Semaphore,
    is_paused: &Arc<TokioMutex<bool>>,
    //
    req: Request<OnChainAddLiqTabReq>,
) -> Result<Response<AddLiqOrderTabRes>, Status> {
    let _permit = semaphore.acquire().await.unwrap();

    let lock = is_paused.lock().await;
    drop(lock);

    tokio::task::yield_now().await;

    let req: OnChainAddLiqTabReq = req.into_inner();

    let is_perp = req.position_add_liquidity_req.is_some();

    let base_token;
    let quote_token;
    if !is_perp {
        let tab_req = &req.tab_add_liquidity_req;

        if tab_req.is_none() || tab_req.as_ref().unwrap().order_tab.is_none() {
            return send_add_liq_tab_error_reply("Order tab is undefined".to_string());
        }
        let tab_header = &tab_req
            .as_ref()
            .unwrap()
            .order_tab
            .as_ref()
            .unwrap()
            .tab_header;

        base_token = tab_header.as_ref().unwrap().base_token;
        quote_token = tab_header.as_ref().unwrap().quote_token;
    } else {
        let pos_req = &req.position_add_liquidity_req;

        if pos_req.is_none() || pos_req.as_ref().unwrap().position.is_none() {
            return send_add_liq_tab_error_reply("Position is undefined".to_string());
        }
        let pos_header = &pos_req
            .as_ref()
            .unwrap()
            .position
            .as_ref()
            .unwrap()
            .position_header;

        base_token = pos_header.as_ref().unwrap().synthetic_token;
        quote_token = COLLATERAL_TOKEN;
    }

    println!("market_id: {}", req.market_id);
    println!("order_books keys: {:?}", order_books.keys());

    // ? Verify the market_id exists
    let order_book;
    if is_perp {
        if !perp_order_books.contains_key(&(req.market_id as u16)) {
            return send_add_liq_tab_error_reply(
                "No market found for given base and quote token".to_string(),
            );
        }

        // ? Get the relevant orderbook from the market_id
        order_book = perp_order_books
            .get(&(req.market_id as u16))
            .unwrap()
            .lock()
            .await;
    } else {
        if !order_books.contains_key(&(req.market_id as u16)) {
            return send_add_liq_tab_error_reply(
                "No market found for given base and quote token".to_string(),
            );
        }

        // ? Get the relevant orderbook from the market_id
        order_book = order_books
            .get(&(req.market_id as u16))
            .unwrap()
            .lock()
            .await;
    }

    // ? Verify the base and quote asset match the market_id
    if order_book.order_asset != base_token
        || order_book.price_asset != quote_token
        || base_token != req.base_token
    {
        return send_add_liq_tab_error_reply(
            "Base and quote asset do not match market_id".to_string(),
        );
    }

    let tx_batch_m = tx_batch.lock().await;
    let swap_output_json = Arc::clone(&tx_batch_m.swap_output_json);
    let main_storage = Arc::clone(&tx_batch_m.main_storage);
    drop(tx_batch_m);

    let tab_action_message = OrderTabActionMessage {
        open_order_tab_req: None,
        close_order_tab_req: None,
        onchain_add_liq_req: Some(req),
        onchain_register_mm_req: None,
        onchain_remove_liq_req: None,
    };

    let mut tx_batch_m = tx_batch.lock().await;
    let order_action_handle = tx_batch_m.execute_order_tab_modification(tab_action_message);
    drop(tx_batch_m);

    let order_action_response = order_action_handle.join();

    match order_action_response {
        Ok(res) => match res.add_liq_response.unwrap() {
            Ok((order_tab, position, vlp_note)) => {
                store_output_json(&swap_output_json, &main_storage);

                let order_tab = order_tab.map(|tab| GrpcOrderTab::from(tab));
                let position = position.map(|pos| GrpcPerpPosition::from(pos));
                let vlp_note = GrpcNote::from(vlp_note);
                let reply = AddLiqOrderTabRes {
                    successful: true,
                    error_message: "".to_string(),
                    order_tab,
                    position,
                    vlp_note: Some(vlp_note),
                };

                return Ok(Response::new(reply));
            }
            Err(err) => {
                println!("Error in open order tab execution: {}", err);

                return send_add_liq_tab_error_reply(
                    "Error occurred in the open order tab execution".to_string() + &err,
                );
            }
        },
        Err(_e) => {
            println!("Unknown Error in open order tab execution");

            return send_add_liq_tab_error_reply(
                "Unknown Error occurred in the open order tab execution".to_string(),
            );
        }
    }
}

pub async fn remove_liquidity_mm_inner(
    tx_batch: &Arc<TokioMutex<TransactionBatch>>,
    order_books: &HashMap<u16, Arc<TokioMutex<OrderBook>>>,
    perp_order_books: &HashMap<u16, Arc<TokioMutex<OrderBook>>>,
    semaphore: &Semaphore,
    is_paused: &Arc<TokioMutex<bool>>,
    //
    req: Request<OnChainRemoveLiqTabReq>,
) -> Result<Response<RemoveLiqOrderTabRes>, Status> {
    let _permit = semaphore.acquire().await.unwrap();

    let lock = is_paused.lock().await;
    drop(lock);

    tokio::task::yield_now().await;

    let req: OnChainRemoveLiqTabReq = req.into_inner();

    let is_perp = req.position_remove_liquidity_req.is_some();

    let base_token;
    let quote_token;
    if req.tab_remove_liquidity_req.is_some() {
        let tab_req = &req.tab_remove_liquidity_req;

        if tab_req.is_none() || tab_req.as_ref().unwrap().order_tab.is_none() {
            return send_remove_liq_tab_error_reply("Order tab is undefined".to_string());
        }
        let tab_header = &tab_req
            .as_ref()
            .unwrap()
            .order_tab
            .as_ref()
            .unwrap()
            .tab_header;

        base_token = tab_header.as_ref().unwrap().base_token;
        quote_token = tab_header.as_ref().unwrap().quote_token;
    } else {
        let pos_req = &req.position_remove_liquidity_req;

        if pos_req.is_none() || pos_req.as_ref().unwrap().position.is_none() {
            return send_remove_liq_tab_error_reply("Position is undefined".to_string());
        }
        let pos_header = &pos_req
            .as_ref()
            .unwrap()
            .position
            .as_ref()
            .unwrap()
            .position_header;

        base_token = pos_header.as_ref().unwrap().synthetic_token;
        quote_token = COLLATERAL_TOKEN;
    }

    // ? Verify the market_id exists
    let order_book;
    if is_perp {
        if !perp_order_books.contains_key(&(req.market_id as u16)) {
            return send_remove_liq_tab_error_reply(
                "No market found for given base and quote token".to_string(),
            );
        }

        // ? Get the relevant orderbook from the market_id
        order_book = perp_order_books
            .get(&(req.market_id as u16))
            .unwrap()
            .lock()
            .await;
    } else {
        if !order_books.contains_key(&(req.market_id as u16)) {
            return send_remove_liq_tab_error_reply(
                "No market found for given base and quote token".to_string(),
            );
        }

        // ? Get the relevant orderbook from the market_id
        order_book = order_books
            .get(&(req.market_id as u16))
            .unwrap()
            .lock()
            .await;
    }

    // ? Verify the base and quote asset match the market_id
    if order_book.order_asset != base_token
        || order_book.price_asset != quote_token
        || base_token != req.base_token
    {
        return send_remove_liq_tab_error_reply(
            "Base and quote asset do not match market_id".to_string(),
        );
    }

    let tx_batch_m = tx_batch.lock().await;
    let swap_output_json = Arc::clone(&tx_batch_m.swap_output_json);
    let main_storage = Arc::clone(&tx_batch_m.main_storage);
    drop(tx_batch_m);

    let tab_action_message = OrderTabActionMessage {
        open_order_tab_req: None,
        close_order_tab_req: None,
        onchain_add_liq_req: None,
        onchain_register_mm_req: None,
        onchain_remove_liq_req: Some(req),
    };

    let mut tx_batch_m = tx_batch.lock().await;
    let order_action_handle = tx_batch_m.execute_order_tab_modification(tab_action_message);
    drop(tx_batch_m);

    let order_action_response = order_action_handle.join();

    match order_action_response {
        Ok(res) => match res.remove_liq_response.unwrap() {
            Ok((tab_res, position_res)) => {
                store_output_json(&swap_output_json, &main_storage);

                let tab_res = tab_res.map(|(tab, base_note, quote_note)| TabRemoveLiqRes {
                    order_tab: tab.map(|t| GrpcOrderTab::from(t)),
                    base_return_note: Some(GrpcNote::from(base_note)),
                    quote_return_note: Some(GrpcNote::from(quote_note)),
                });

                let position_res =
                    position_res.map(|(pos, collateral_note)| PositionRemoveLiqRes {
                        position: pos.map(|p| GrpcPerpPosition::from(p)),
                        collateral_return_note: Some(GrpcNote::from(collateral_note)),
                    });

                let reply = RemoveLiqOrderTabRes {
                    successful: true,
                    error_message: "".to_string(),
                    tab_res,
                    position_res,
                };

                return Ok(Response::new(reply));
            }
            Err(err) => {
                println!("Error in open order tab execution: {}", err);

                return send_remove_liq_tab_error_reply(
                    "Error occurred in the open order tab execution".to_string() + &err,
                );
            }
        },
        Err(_e) => {
            println!("Unknown Error in open order tab execution");

            return send_remove_liq_tab_error_reply(
                "Unknown Error occurred in the open order tab execution".to_string(),
            );
        }
    }
}
