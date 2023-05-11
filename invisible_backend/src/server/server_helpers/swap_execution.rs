use std::thread::{JoinHandle, ThreadId};
use std::time::SystemTime;
use std::{collections::HashMap, sync::Arc};

use firestore_db_and_auth::ServiceSession;
use serde_json::json;
use tokio::sync::{oneshot, Mutex as TokioMutex};

use error_stack::Result;
use parking_lot::Mutex;
use tokio_tungstenite::tungstenite::Message;

use crate::matching_engine::orderbook::{Failed, Success};
use crate::matching_engine::orders::new_limit_order_request;
use crate::matching_engine::{
    domain::{Order, OrderSide as OBOrderSide},
    orderbook::OrderBook,
};
use crate::perpetual::{get_cross_price, scale_up_price};

use crate::server::server_helpers::brodcast_message;
use crate::transactions::limit_order::LimitOrder;
use crate::transactions::swap::OrderFillResponse;
use crate::transactions::transaction_helpers::db_updates::store_spot_fill;
use crate::transactions::{
    swap::{Swap, SwapResponse},
    transaction_helpers::rollbacks::{initiate_rollback, RollbackInfo},
};

use crate::utils::crypto_utils::Signature;
use crate::utils::storage::BackupStorage;
use crate::utils::{errors::TransactionExecutionError, notes::Note};

use tokio::sync::{mpsc::Sender as MpscSender, oneshot::Sender as OneshotSender};

use tokio::task::{JoinError, JoinHandle as TokioJoinHandle};

use super::super::grpc::{GrpcMessage, GrpcTxResponse, MessageType, RollbackMessage};
use super::super::server_helpers::get_order_side;
use super::{proccess_spot_matching_result, send_direct_message, WsConnectionsMap, WsIdsMap};

pub async fn execute_swap(
    swap: Swap,
    transaction_mpsc_tx: MpscSender<(GrpcMessage, OneshotSender<GrpcTxResponse>)>,
    rollback_safeguard: Arc<Mutex<HashMap<ThreadId, RollbackInfo>>>,
    order_book: Arc<TokioMutex<OrderBook>>,
    user_id_pair: (u64, u64),
    session: Arc<Mutex<ServiceSession>>,
    backup_storage: Arc<Mutex<BackupStorage>>,
) -> (
    Option<((Message, Message), (u64, u64), Message)>,
    Option<(Option<u64>, u64, u64)>,
) {
    // ? Store relevant values before the swap in case of failure (for rollbacks and orderbook reinsertions)
    let order_a_clone = swap.order_a.clone();
    let order_b_clone = swap.order_b.clone();

    let book__ = order_book.lock().await;
    let maker_order: LimitOrder;
    let maker_side: OBOrderSide;
    let taker_side: OBOrderSide;
    let taker_order_id: u64;
    if swap.fee_taken_a == 0 {
        maker_order = swap.order_a.clone();
        maker_side = get_order_side(
            &book__,
            swap.order_a.token_spent,
            swap.order_a.token_received,
        )
        .unwrap();
        taker_side = get_order_side(
            &book__,
            swap.order_a.token_received,
            swap.order_a.token_spent,
        )
        .unwrap();
        taker_order_id = swap.order_b.order_id;
    } else {
        maker_order = swap.order_b.clone();
        maker_side = get_order_side(
            &book__,
            swap.order_b.token_spent,
            swap.order_b.token_received,
        )
        .unwrap();
        taker_side = get_order_side(
            &book__,
            swap.order_b.token_received,
            swap.order_b.token_spent,
        )
        .unwrap();
        taker_order_id = swap.order_a.order_id;
    };
    let maker_order_id = maker_order.order_id;
    drop(book__);

    let fee_taken_a = swap.fee_taken_a;
    let fee_taken_b = swap.fee_taken_b;

    let base_asset: u64;
    let quote_asset: u64;
    let book__ = order_book.lock().await;
    let side_a = get_order_side(
        &book__,
        order_a_clone.token_spent,
        order_a_clone.token_received,
    )
    .unwrap();
    base_asset = book__.order_asset;
    quote_asset = book__.price_asset;
    drop(book__);

    // ? The qty and price being traded
    let qty: u64;
    let price: u64;
    if side_a == OBOrderSide::Bid {
        qty = swap.spent_amount_b;
        let p = get_cross_price(
            base_asset,
            quote_asset,
            swap.spent_amount_b,
            swap.spent_amount_a,
            None,
        );

        price = scale_up_price(p, base_asset);
    } else {
        qty = swap.spent_amount_a;
        let p = get_cross_price(
            base_asset,
            quote_asset,
            swap.spent_amount_a,
            swap.spent_amount_b,
            None,
        );

        price = scale_up_price(p, base_asset);
    };

    let tx_mpsc_tx = transaction_mpsc_tx.clone();
    let handle: TokioJoinHandle<
        JoinHandle<Result<(Option<SwapResponse>, Option<Vec<u64>>), TransactionExecutionError>>,
    > = tokio::spawn(async move {
        let (resp_tx, resp_rx) = oneshot::channel();

        let grpc_message = GrpcMessage {
            msg_type: MessageType::SwapMessage,
            deposit_message: None,
            swap_message: Some(swap),
            withdrawal_message: None,
            perp_swap_message: None,
            split_notes_message: None,
            change_margin_message: None,
            rollback_info_message: None,
            funding_update_message: None,
            price_update_message: None,
        };

        tx_mpsc_tx.send((grpc_message, resp_tx)).await.ok().unwrap();
        let res = resp_rx.await.unwrap();

        return res.tx_handle.unwrap();
    });

    let swap_handle = handle.await.unwrap();

    let thread_id = swap_handle.thread().id();

    let swap_response = swap_handle.join();

    match swap_response {
        Ok(res1) => match res1 {
            Ok(response) => {
                println!("swap executed successfuly in the beckend engine\n");

                let mut book = order_book.lock().await;

                if maker_side == OBOrderSide::Bid {
                    book.bid_queue
                        .reduce_pending_order(maker_order.order_id, qty, false);
                } else {
                    book.ask_queue
                        .reduce_pending_order(maker_order.order_id, qty, false);
                }

                let swap_res = response.0.unwrap();
                let fill_res_a =
                    OrderFillResponse::from_swap_response(&swap_res, fee_taken_a, true);
                let fill_res_b =
                    OrderFillResponse::from_swap_response(&swap_res, fee_taken_b, false);

                // ? Return the swap response to be sent over the websocket in the engine
                let json_msg_a = json!({
                    "message_id": "SWAP_RESULT",
                    "order_id": order_a_clone.order_id,
                    "market_id": book.market_id,
                    "swap_response": serde_json::to_value(fill_res_a).unwrap(),
                });
                let msg_a = Message::Text(json_msg_a.to_string());

                let json_msg_b = json!({
                    "message_id": "SWAP_RESULT",
                    "order_id": order_b_clone.order_id,
                    "market_id": book.market_id,
                    "swap_response": serde_json::to_value(fill_res_b).unwrap(),
                });
                let msg_b = Message::Text(json_msg_b.to_string());

                // Get the order time in seconds since UNIX_EPOCH
                let ts = SystemTime::now();
                let timestamp = ts
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .expect("Time went backwards")
                    .as_secs();

                // ? Store the fill info in the datatbase
                store_spot_fill(
                    &session,
                    backup_storage,
                    qty,
                    price,
                    user_id_pair.0,
                    user_id_pair.1,
                    base_asset,
                    quote_asset,
                    taker_side == OBOrderSide::Bid,
                    timestamp,
                );

                let json_msg = json!({
                    "message_id": "SWAP_FILLED",
                    "type": "spot",
                    "asset": base_asset,
                    "amount": qty,
                    "price": price,
                    "is_buy": taker_side == OBOrderSide::Bid,
                    "timestamp": timestamp,
                    "user_id_a": user_id_pair.0,
                    "user_id_b": user_id_pair.1,
                });

                let fill_msg = Message::Text(json_msg.to_string());

                return (Some(((msg_a, msg_b), user_id_pair, fill_msg)), None);
            }
            Err(err) => {
                // println!("\n{:?}", err);

                let should_rollback = rollback_safeguard.lock().contains_key(&thread_id);

                if should_rollback {
                    let notes_in_a: (u64, Option<Vec<Note>>) =
                        (order_a_clone.order_id, Some(order_a_clone.notes_in.clone()));
                    let notes_in_b: (u64, Option<Vec<Note>>) =
                        (order_b_clone.order_id, Some(order_b_clone.notes_in.clone()));

                    let rollback_message = RollbackMessage {
                        tx_type: "swap".to_string(),
                        notes_in_a,
                        notes_in_b,
                    };

                    initiate_rollback(transaction_mpsc_tx, thread_id, rollback_message).await;
                }

                let mut maker_order_id_ = None;
                if let TransactionExecutionError::Swap(swap_execution_error) = err.current_context()
                {
                    let mut book = order_book.lock().await;

                    // println!("swap execution error: {:?}", swap_execution_error);

                    if let Some(invalid_order_id) = swap_execution_error.invalid_order {
                        // ? only add the order back into the orderbook if not eql invalid_order_id
                        if maker_order.order_id.eq(&invalid_order_id) {
                            if maker_side == OBOrderSide::Bid {
                                book.bid_queue
                                    .reduce_pending_order(maker_order.order_id, 0, true);
                            } else {
                                book.ask_queue
                                    .reduce_pending_order(maker_order.order_id, 0, true);
                            }
                        }
                        // ? else forcefully cancel that order since it is invalid
                        else {
                            if maker_side == OBOrderSide::Bid {
                                book.bid_queue
                                    .restore_pending_order(Order::Spot(maker_order), qty);
                            } else {
                                book.ask_queue
                                    .restore_pending_order(Order::Spot(maker_order), qty);
                            }

                            if taker_order_id == invalid_order_id {
                                return (None, None);
                            }
                        }
                    } else {
                        if maker_side == OBOrderSide::Bid {
                            book.bid_queue
                                .restore_pending_order(Order::Spot(maker_order), qty);
                        } else {
                            book.ask_queue
                                .restore_pending_order(Order::Spot(maker_order), qty);
                        }

                        maker_order_id_ = Some(maker_order_id);
                    }
                }

                return (None, Some((maker_order_id_, taker_order_id, qty)));

                // Todo: Could save these errors somewhere and have some kind of analytics
            }
        },
        Err(_e) => {
            let should_rollback = rollback_safeguard.lock().contains_key(&thread_id);

            if should_rollback {
                let notes_in_a: (u64, Option<Vec<Note>>) =
                    (order_a_clone.order_id, Some(order_a_clone.notes_in.clone()));
                let notes_in_b: (u64, Option<Vec<Note>>) =
                    (order_b_clone.order_id, Some(order_b_clone.notes_in.clone()));

                let rollback_message = RollbackMessage {
                    tx_type: "swap".to_string(),
                    notes_in_a,
                    notes_in_b,
                };

                initiate_rollback(transaction_mpsc_tx, thread_id, rollback_message).await;
            }

            let mut book = order_book.lock().await;
            if maker_side == OBOrderSide::Bid {
                book.bid_queue
                    .restore_pending_order(Order::Spot(maker_order), qty);
            } else {
                book.ask_queue
                    .restore_pending_order(Order::Spot(maker_order), qty);
            }

            return (None, Some((Some(maker_order_id), taker_order_id, qty)));
        }
    }
}

pub async fn process_and_execute_spot_swaps(
    mpsc_tx: &MpscSender<(GrpcMessage, OneshotSender<GrpcTxResponse>)>,
    rollback_safeguard: &Arc<Mutex<HashMap<ThreadId, RollbackInfo>>>,
    order_book: &Arc<TokioMutex<OrderBook>>,
    session: &Arc<Mutex<ServiceSession>>,
    backup_storage: &Arc<Mutex<BackupStorage>>,
    processed_res: &mut Vec<std::result::Result<Success, Failed>>,
) -> std::result::Result<
    (
        Vec<
            std::result::Result<
                (
                    Option<((Message, Message), (u64, u64), Message)>,
                    Option<(Option<u64>, u64, u64)>,
                ),
                JoinError,
            >,
        >,
        u64,
    ),
    String,
> {
    // ? Parse processed_res into swaps and get the new order_id
    let res = proccess_spot_matching_result(processed_res);

    if let Err(err) = &res {
        return Err(err.current_context().err_msg.to_string());
    }
    let processed_result = res.unwrap();

    // ? Execute the swaps if any orders were matched
    let mut handles = Vec::new();
    if let Some(swaps) = processed_result.swaps {
        for (swap, user_id_a, user_id_b) in swaps {
            let mpsc_tx = mpsc_tx.clone();
            let rollback_safeguard_clone = rollback_safeguard.clone();
            let order_book = order_book.clone();
            let session = session.clone();
            let backup_storage = backup_storage.clone();

            let handle = tokio::spawn(execute_swap(
                swap,
                mpsc_tx,
                rollback_safeguard_clone,
                order_book,
                (user_id_a, user_id_b),
                session,
                backup_storage,
            ));

            let res = handle.await;

            handles.push(res);
        }
    }

    return Ok((handles, processed_result.new_order_id));
}

pub async fn process_limit_order_request(
    order_book: &Arc<TokioMutex<OrderBook>>,
    limit_order: LimitOrder,
    side: OBOrderSide,
    signature: Signature,
    user_id: u64,
    is_market: bool,
    is_retry: bool, // if the order has been matched before but the swap failed for some reason
    retry_qty: u64, // the qty that has been matched before in the swap that failed
    taker_order_id: u64, // the order_id of the order that has been matched before in the swap that failed
    failed_counterpart_ids: Option<Vec<u64>>, // the maker orderIds that were matched with the taker_order_id but failed because its incompatible
) -> Vec<std::result::Result<Success, Failed>> {
    // ? Create a new OrderRequest object
    let order_ = Order::Spot(limit_order);
    let order_request = new_limit_order_request(
        side,
        order_,
        signature,
        SystemTime::now(),
        is_market,
        user_id,
    );

    // ? Insert the order into the book and get back the matched results if any
    let mut order_book_m = order_book.lock().await;
    let processed_res = if !is_retry {
        order_book_m.process_order(order_request)
    } else {
        order_book_m.retry_order(
            order_request,
            retry_qty,
            taker_order_id,
            failed_counterpart_ids,
        )
    };
    drop(order_book_m);

    return processed_res;
}

use async_recursion::async_recursion;

type SwapExecutionResultMessage = std::result::Result<
    (
        Option<((Message, Message), (u64, u64), Message)>,
        Option<(Option<u64>, u64, u64)>,
    ),
    JoinError,
>;

pub async fn await_swap_handles(
    ws_connections: &Arc<TokioMutex<WsConnectionsMap>>,
    ws_ids: &Arc<TokioMutex<WsIdsMap>>,
    messages: Vec<SwapExecutionResultMessage>,
) -> std::result::Result<Vec<(Option<u64>, u64, u64)>, String> {
    // ? Wait for the swaps to finish
    let mut retry_messages = Vec::new();
    for msg_ in messages {
        if let Err(_) = msg_ {
            continue;
        }

        if msg_.as_ref().unwrap().0.is_some() {
            // If the swap was successful, send the messages to the users
            let ((msg_a, msg_b), (user_id_a, user_id_b), fill_msg) = msg_.unwrap().0.unwrap();

            // ? Send a message to the user_id websocket
            if let Err(_) = send_direct_message(ws_connections, ws_ids, user_id_a, msg_a).await {
                println!("Error sending swap message")
            };

            // ? Send a message to the user_id websocket
            if let Err(_) = send_direct_message(ws_connections, ws_ids, user_id_b, msg_b).await {
                println!("Error sending swap message")
            };

            // ? Send the swap fill to anyone who's listening
            if let Err(_) = brodcast_message(&ws_connections, fill_msg).await {
                println!("Error sending swap fill message")
            };
        } else if msg_.as_ref().unwrap().1.is_some() {
            // If there was an error in the swap, retry the order

            retry_messages.push(msg_.unwrap().1.unwrap());
        }
    }

    Ok(retry_messages)
}

#[async_recursion]
pub async fn retry_failed_swaps(
    mpsc_tx: &MpscSender<(GrpcMessage, OneshotSender<GrpcTxResponse>)>,
    rollback_safeguard: &Arc<Mutex<HashMap<ThreadId, RollbackInfo>>>,
    order_book: &Arc<TokioMutex<OrderBook>>,
    session: &Arc<Mutex<ServiceSession>>,
    backup_storage: &Arc<Mutex<BackupStorage>>,
    limit_order: LimitOrder,
    side: OBOrderSide,
    signature: Signature,
    user_id: u64,
    is_market: bool,
    ws_connections: &Arc<TokioMutex<WsConnectionsMap>>,
    ws_ids: &Arc<TokioMutex<WsIdsMap>>,
    retry_messages: Vec<(Option<u64>, u64, u64)>,
    failed_counterpart_ids: Option<Vec<u64>>,
) -> std::result::Result<(), String> {
    for msg_ in retry_messages {
        let (maker_order_id, taker_order_id, qty) = msg_;

        let mut failed_ids = if failed_counterpart_ids.is_some() {
            failed_counterpart_ids.clone().unwrap()
        } else {
            Vec::new()
        };
        if maker_order_id.is_some() {
            failed_ids.push(maker_order_id.unwrap());
        }

        let mut processed_res = process_limit_order_request(
            order_book,
            limit_order.clone(),
            side,
            signature.clone(),
            user_id,
            is_market,
            true,
            qty,
            taker_order_id,
            if failed_ids.len() > 0 {
                Some(failed_ids.clone())
            } else {
                None
            },
        )
        .await;

        let new_handles;
        match process_and_execute_spot_swaps(
            mpsc_tx,
            rollback_safeguard,
            order_book,
            session,
            backup_storage,
            &mut processed_res,
        )
        .await
        {
            Ok((h, _oid)) => {
                new_handles = h;
            }
            Err(err) => {
                return Err(err);
            }
        };

        let retry_messages;
        match await_swap_handles(ws_connections, ws_ids, new_handles).await {
            Ok(rm) => retry_messages = rm,
            Err(e) => return Err(e),
        };

        if retry_messages.len() > 0 {
            return retry_failed_swaps(
                &mpsc_tx,
                &rollback_safeguard,
                order_book,
                &session,
                &backup_storage,
                limit_order,
                side,
                signature,
                user_id,
                is_market,
                ws_connections,
                ws_ids,
                retry_messages,
                None,
            )
            .await;
        }
    }

    Ok(())
}
