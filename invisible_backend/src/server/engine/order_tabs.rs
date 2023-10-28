use parking_lot::Mutex;
use serde_json::Value;
use std::{collections::HashMap, sync::Arc, thread::JoinHandle};

use super::super::grpc::{
    engine_proto::{CloseOrderTabReq, GrpcNote, GrpcOrderTab, OpenOrderTabReq},
    OrderTabActionResponse,
};
use super::super::grpc::{
    engine_proto::{CloseOrderTabRes, OpenOrderTabRes},
    OrderTabActionMessage,
};
use super::super::{
    grpc::{GrpcMessage, GrpcTxResponse, MessageType},
    server_helpers::engine_helpers::store_output_json,
};
use crate::utils::errors::{send_close_tab_error_reply, send_open_tab_error_reply};
use crate::{matching_engine::orderbook::OrderBook, utils::storage::local_storage::MainStorage};

use tokio::sync::{
    mpsc::Sender as MpscSender,
    oneshot::{self, Sender as OneshotSender},
    Mutex as TokioMutex, Semaphore,
};
use tokio::task::JoinHandle as TokioJoinHandle;
use tonic::{Request, Response, Status};

//
// * ===================================================================================================================================
//

pub async fn open_order_tab_inner(
    mpsc_tx: &MpscSender<(GrpcMessage, OneshotSender<GrpcTxResponse>)>,
    main_storage: &Arc<Mutex<MainStorage>>,
    swap_output_json: &Arc<Mutex<Vec<serde_json::Map<String, Value>>>>,
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

    let transaction_mpsc_tx = mpsc_tx.clone();

    let handle: TokioJoinHandle<JoinHandle<OrderTabActionResponse>> = tokio::spawn(async move {
        let (resp_tx, resp_rx) = oneshot::channel();

        let mut grpc_message = GrpcMessage::new();
        grpc_message.msg_type = MessageType::OrderTabAction;
        grpc_message.order_tab_action_message = Some(OrderTabActionMessage {
            open_order_tab_req: Some(req),
            close_order_tab_req: None,
            onchain_add_liq_req: None,
            onchain_register_mm_req: None,
            onchain_remove_liq_req: None,
        });

        transaction_mpsc_tx
            .send((grpc_message, resp_tx))
            .await
            .ok()
            .unwrap();
        let res = resp_rx.await.unwrap();

        return res.order_tab_action_response.unwrap();
    });

    let order_action_handle = handle.await.unwrap();
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
    mpsc_tx: &MpscSender<(GrpcMessage, OneshotSender<GrpcTxResponse>)>,
    main_storage: &Arc<Mutex<MainStorage>>,
    swap_output_json: &Arc<Mutex<Vec<serde_json::Map<String, Value>>>>,
    semaphore: &Semaphore,
    is_paused: &Arc<TokioMutex<bool>>,
    //
    req: Request<CloseOrderTabReq>,
) -> Result<Response<CloseOrderTabRes>, Status> {
    let _permit = semaphore.acquire().await.unwrap();

    let lock = is_paused.lock().await;
    drop(lock);

    tokio::task::yield_now().await;

    let req: CloseOrderTabReq = req.into_inner();

    let transaction_mpsc_tx = mpsc_tx.clone();

    let handle: TokioJoinHandle<JoinHandle<OrderTabActionResponse>> = tokio::spawn(async move {
        let (resp_tx, resp_rx) = oneshot::channel();

        let mut grpc_message = GrpcMessage::new();
        grpc_message.msg_type = MessageType::OrderTabAction;
        grpc_message.order_tab_action_message = Some(OrderTabActionMessage {
            open_order_tab_req: None,
            close_order_tab_req: Some(req),
            onchain_add_liq_req: None,
            onchain_register_mm_req: None,
            onchain_remove_liq_req: None,
        });

        transaction_mpsc_tx
            .send((grpc_message, resp_tx))
            .await
            .ok()
            .unwrap();
        let res = resp_rx.await.unwrap();

        return res.order_tab_action_response.unwrap();
    });

    let order_action_handle = handle.await.unwrap();
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
