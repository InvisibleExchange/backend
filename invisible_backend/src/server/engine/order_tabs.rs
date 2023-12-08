use std::{collections::HashMap, sync::Arc};

use super::super::grpc::engine_proto::{CloseOrderTabReq, GrpcNote, GrpcOrderTab, OpenOrderTabReq};
use super::super::grpc::{
    engine_proto::{CloseOrderTabRes, OpenOrderTabRes},
    OrderTabActionMessage,
};
use super::super::server_helpers::engine_helpers::store_output_json;
use crate::matching_engine::orderbook::OrderBook;
use crate::{
    transaction_batch::TransactionBatch,
    utils::errors::{send_close_tab_error_reply, send_open_tab_error_reply},
};

use tokio::sync::{Mutex as TokioMutex, Semaphore};
use tonic::{Request, Response, Status};

//
// * ===================================================================================================================================
//

pub async fn open_order_tab_inner(
    tx_batch: &Arc<TokioMutex<TransactionBatch>>,
    order_books: &HashMap<u16, Arc<TokioMutex<OrderBook>>>,
    semaphore: &Semaphore,
    is_paused: &Arc<TokioMutex<bool>>,
    //
    req: Request<OpenOrderTabReq>,
) -> Result<Response<OpenOrderTabRes>, Status> {
    let _permit = semaphore.acquire().await.unwrap();

    let lock = is_paused.lock().await;
    drop(lock);

    tokio::task::yield_now().await;

    let req: OpenOrderTabReq = req.into_inner();

    let tx_batch_m = tx_batch.lock().await;
    let swap_output_json = Arc::clone(&tx_batch_m.swap_output_json);
    let main_storage = Arc::clone(&tx_batch_m.main_storage);
    drop(tx_batch_m);

    if req.order_tab.is_none() || req.order_tab.as_ref().unwrap().tab_header.is_none() {
        return send_open_tab_error_reply("Order tab is undefined".to_string());
    }
    let tab_header = req.order_tab.as_ref().unwrap().tab_header.as_ref().unwrap();

    // ? Verify the market_id exists
    if !order_books.contains_key(&(req.market_id as u16)) {
        return send_open_tab_error_reply(
            "No market found for given base and quote token".to_string(),
        );
    }

    // ? Get the relevant orderbook from the market_id
    let order_book = order_books
        .get(&(req.market_id as u16))
        .unwrap()
        .lock()
        .await;

    // ? Verify the base and quote asset match the market_id
    if order_book.order_asset != tab_header.base_token
        || order_book.price_asset != tab_header.quote_token
    {
        return send_open_tab_error_reply(
            "Base and quote asset do not match market_id".to_string(),
        );
    }

    let tab_action_message = OrderTabActionMessage {
        open_order_tab_req: Some(req),
        close_order_tab_req: None,
    };

    let mut tx_batch_m = tx_batch.lock().await;
    let order_action_handle = tx_batch_m.execute_order_tab_modification(tab_action_message);
    drop(tx_batch_m);

    let order_action_response = order_action_handle.join();

    match order_action_response {
        Ok(res) => match res.open_tab_response.unwrap() {
            Ok(order_tab) => {
                store_output_json(&swap_output_json, &main_storage);

                let order_tab = GrpcOrderTab::from(order_tab);
                let reply = OpenOrderTabRes {
                    successful: true,
                    error_message: "".to_string(),
                    order_tab: Some(order_tab),
                };

                return Ok(Response::new(reply));
            }
            Err(err) => {
                println!("Error in open order tab execution: {}", err);

                return send_open_tab_error_reply(
                    "Error occurred in the open order tab execution".to_string() + &err,
                );
            }
        },
        Err(_e) => {
            println!("Unknown Error in open order tab execution");

            return send_open_tab_error_reply(
                "Unknown Error occurred in the open order tab execution".to_string(),
            );
        }
    }
}

//
// * ===================================================================================================================================
//

//
// * ===================================================================================================================================
//

pub async fn close_order_tab_inner(
    tx_batch: &Arc<TokioMutex<TransactionBatch>>,
    semaphore: &Semaphore,
    is_paused: &Arc<TokioMutex<bool>>,
    //
    req: CloseOrderTabReq,
) -> Result<Response<CloseOrderTabRes>, Status> {
    let _permit = semaphore.acquire().await.unwrap();

    let lock = is_paused.lock().await;
    drop(lock);

    tokio::task::yield_now().await;

    let tx_batch_m = tx_batch.lock().await;
    let swap_output_json = Arc::clone(&tx_batch_m.swap_output_json);
    let main_storage = Arc::clone(&tx_batch_m.main_storage);
    drop(tx_batch_m);

    let tab_action_message = OrderTabActionMessage {
        open_order_tab_req: None,
        close_order_tab_req: Some(req),
    };

    let mut tx_batch_m = tx_batch.lock().await;
    let order_action_handle = tx_batch_m.execute_order_tab_modification(tab_action_message);
    drop(tx_batch_m);

    let order_action_response = order_action_handle.join();

    match order_action_response {
        Ok(res) => match res.close_tab_response.unwrap() {
            Ok((base_r_note, quote_r_note)) => {
                store_output_json(&swap_output_json, &main_storage);

                let base_return_note = Some(GrpcNote::from(base_r_note));
                let quote_return_note = Some(GrpcNote::from(quote_r_note));
                let reply = CloseOrderTabRes {
                    successful: true,
                    error_message: "".to_string(),
                    base_return_note,
                    quote_return_note,
                };

                return Ok(Response::new(reply));
            }
            Err(err) => {
                println!("Error in close order tab execution: {}", err);

                return send_close_tab_error_reply(
                    "Error occurred in the close order tab execution".to_string() + &err,
                );
            }
        },
        Err(_e) => {
            println!("Unknown Error in close order tab execution");

            return send_close_tab_error_reply(
                "Unknown Error occurred in the close order tab execution".to_string(),
            );
        }
    }
}
