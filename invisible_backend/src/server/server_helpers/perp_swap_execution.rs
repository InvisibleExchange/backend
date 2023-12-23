use std::sync::Arc;
use std::time::SystemTime;

use async_recursion::async_recursion;
use firestore_db_and_auth::ServiceSession;
use serde_json::json;
use tokio::sync::Mutex as TokioMutex;

use parking_lot::Mutex;
use tokio::sync::oneshot::Sender;
use tokio_tungstenite::tungstenite::Message;

use crate::matching_engine::orderbook::{Failed, Success};
use crate::matching_engine::orders::new_limit_order_request;
use crate::matching_engine::{
    domain::{Order, OrderSide as OBOrderSide},
    orderbook::OrderBook,
};
use crate::perpetual::perp_helpers::db_updates::store_perp_fill;
use crate::perpetual::perp_helpers::perp_swap_outptut::PerpOrderFillResponse;
use crate::perpetual::perp_position::PerpPosition;
use crate::perpetual::{get_cross_price, scale_up_price, PositionEffectType, COLLATERAL_TOKEN};
use crate::perpetual::{perp_order::PerpOrder, perp_swap::PerpSwap, OrderSide};
use crate::transaction_batch::TransactionBatch;

use crate::utils::crypto_utils::Signature;
use crate::utils::storage::backup_storage::BackupStorage;
use crate::utils::{errors::PerpSwapExecutionError, notes::Note};

use super::{
    broadcast_message, proccess_perp_matching_result, send_direct_message, send_to_relay_server,
    WsConnectionsMap,
};

type SwapErrorInfo = (Option<u64>, u64, u64, String);

pub async fn execute_perp_swap(
    perp_swap: PerpSwap,
    tx_batch: &Arc<TokioMutex<TransactionBatch>>,
    perp_order_book: &Arc<TokioMutex<OrderBook>>,
    user_id_pair: (u64, u64),
    session: &Arc<Mutex<ServiceSession>>,
    backup_storage: &Arc<Mutex<BackupStorage>>,
) -> (
    Option<(
        (Message, Message),
        (u64, u64),
        (Option<PerpPosition>, Option<PerpPosition>),
        Message,
    )>,
    Option<SwapErrorInfo>,
) {
    // ? Stores the values of order a in case of rollbacks/failures
    let order_a_clone = perp_swap.order_a.clone();

    // ? Stores the values of order b in case of rollbacks/failures
    let order_b_clone = perp_swap.order_b.clone();

    let fee_taken_a = perp_swap.fee_taken_a;
    let fee_taken_b = perp_swap.fee_taken_b;

    let maker_side: OrderSide;
    let taker_side: OrderSide;
    let maker_order_id: u64;
    let taker_order_id: u64;
    let maker_order: PerpOrder;
    if perp_swap.fee_taken_a == 0 {
        maker_order_id = perp_swap.order_a.order_id;
        maker_side = perp_swap.order_a.order_side.clone();
        taker_side = perp_swap.order_b.order_side.clone();
        taker_order_id = order_b_clone.order_id;
        maker_order = perp_swap.order_a.clone();
    } else {
        maker_order_id = perp_swap.order_b.order_id;
        maker_side = perp_swap.order_b.order_side.clone();
        taker_side = perp_swap.order_a.order_side.clone();
        taker_order_id = order_a_clone.order_id;
        maker_order = perp_swap.order_b.clone();
    };

    // ? The qty being traded
    let qty = perp_swap.spent_synthetic;
    let p: f64 = get_cross_price(
        perp_swap.order_a.synthetic_token,
        COLLATERAL_TOKEN,
        perp_swap.spent_synthetic,
        perp_swap.spent_collateral,
        None,
    );
    let price = scale_up_price(p, perp_swap.order_a.synthetic_token);
    let synthetic_token = perp_swap.order_a.synthetic_token;

    let mut tx_batch_m = tx_batch.lock().await;
    let perp_swap_handle = tx_batch_m.execute_perpetual_transaction(perp_swap);
    drop(tx_batch_m);

    let perp_swap_response = perp_swap_handle.join();

    let mut book = perp_order_book.lock().await;
    match perp_swap_response {
        Ok(res1) => match res1 {
            Ok(response) => {
                if maker_side == OrderSide::Long {
                    book.bid_queue
                        .reduce_pending_order(maker_order_id, qty, false);
                } else {
                    book.ask_queue
                        .reduce_pending_order(maker_order_id, qty, false);
                }

                // ? Update the order positions
                book.update_order_positions(user_id_pair.0, &response.position_a);
                book.update_order_positions(user_id_pair.1, &response.position_b);

                let fill_res_a = PerpOrderFillResponse::from_swap_response(
                    &response,
                    true,
                    qty,
                    order_a_clone.synthetic_token,
                    fee_taken_a,
                );
                let fill_res_b = PerpOrderFillResponse::from_swap_response(
                    &response,
                    false,
                    qty,
                    order_b_clone.synthetic_token,
                    fee_taken_b,
                );

                // ? This is used to update the positions in orders from the same user
                let position_pair = (response.position_a, response.position_b);

                // ? Return the swap response to be sent over the websocket in the engine
                let json_msg1 = json!({
                    "message_id": "PERPETUAL_SWAP",
                    "order_id": order_a_clone.order_id,
                    "swap_response": serde_json::to_value(fill_res_a).unwrap(),

                });
                let msg1 = Message::Text(json_msg1.to_string());
                let json_msg2 = json!({
                    "message_id": "PERPETUAL_SWAP",
                    "order_id": order_b_clone.order_id,
                    "swap_response": serde_json::to_value(fill_res_b).unwrap(),
                });
                let msg2 = Message::Text(json_msg2.to_string());

                // Get the order time in seconds since UNIX_EPOCH
                let ts = SystemTime::now();
                let timestamp = ts
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .expect("Time went backwards")
                    .as_secs();

                store_perp_fill(
                    &session,
                    &backup_storage,
                    qty,
                    price,
                    user_id_pair.0,
                    user_id_pair.1,
                    synthetic_token,
                    taker_side == OrderSide::Long,
                    timestamp,
                );

                let json_msg = json!({
                    "message_id": "SWAP_FILLED",
                    "type": "perpetual",
                    "asset": synthetic_token,
                    "amount": qty,
                    "price": price,
                    "is_buy": taker_side == OrderSide::Long,
                    "timestamp": timestamp,
                    "user_id_a": user_id_pair.0,
                    "user_id_b": user_id_pair.1,
                });

                let fill_msg = Message::Text(json_msg.to_string());

                return (
                    Some(((msg1, msg2), user_id_pair, position_pair, fill_msg)),
                    None,
                );
            }
            Err(err) => {
                // ? Reinsert the orders back into the orderbook
                let PerpSwapExecutionError {
                    err_msg,
                    invalid_order,
                } = err.current_context();

                let mut maker_order_id_ = None;
                if let Some(invalid_order_id) = invalid_order {
                    // ? only add the order back into the orderbook if not eql invalid_order_id
                    if maker_order_id.eq(invalid_order_id) {
                        if maker_side == OrderSide::Long {
                            book.bid_queue.reduce_pending_order(maker_order_id, 0, true);
                        } else {
                            book.ask_queue.reduce_pending_order(maker_order_id, 0, true);
                        }

                        maker_order_id_ = Some(maker_order_id);
                    }
                    // ? else forcefully cancel that order since it is invalid
                    else {
                        if maker_side == OrderSide::Long {
                            book.bid_queue
                                .restore_pending_order(Order::Perp(maker_order), qty);
                        } else {
                            book.ask_queue
                                .restore_pending_order(Order::Perp(maker_order), qty);
                        }

                        if taker_order_id == *invalid_order_id {
                            return (None, Some((None, 0, 0, err_msg.to_owned())));
                        }
                    }
                } else {
                    if maker_side == OrderSide::Long {
                        book.bid_queue
                            .restore_pending_order(Order::Perp(maker_order), qty);
                    } else {
                        book.ask_queue
                            .restore_pending_order(Order::Perp(maker_order), qty);
                    }

                    maker_order_id_ = Some(maker_order_id);
                }

                return (
                    None,
                    Some((maker_order_id_, taker_order_id, qty, err_msg.to_owned())),
                );
            }
        },
        Err(_) => {
            if maker_side == OrderSide::Long {
                book.bid_queue
                    .restore_pending_order(Order::Perp(maker_order), qty);
            } else {
                book.ask_queue
                    .restore_pending_order(Order::Perp(maker_order), qty);
            }

            return (
                None,
                Some((
                    Some(maker_order_id),
                    taker_order_id,
                    qty,
                    "Error executing perpetual swap".to_string(),
                )),
            );
        }
    }
}

pub async fn process_and_execute_perp_swaps(
    tx_batch: &Arc<TokioMutex<TransactionBatch>>,
    perp_order_book: &Arc<TokioMutex<OrderBook>>,
    session: &Arc<Mutex<ServiceSession>>,
    backup_storage: &Arc<Mutex<BackupStorage>>,
    ws_connections: &Arc<TokioMutex<WsConnectionsMap>>,
    privileged_ws_connections: &Arc<TokioMutex<Vec<u64>>>,
    response_sender: Option<Sender<Vec<(Option<PerpPosition>, Option<PerpPosition>)>>>,
    processed_res: &mut Vec<std::result::Result<Success, Failed>>,
    user_id: u64,
) -> std::result::Result<(Vec<SwapErrorInfo>, u64), String> {
    // ? Parse processed_res into swaps and get the new order_id
    let res = proccess_perp_matching_result(processed_res);

    if let Err(err) = &res {
        return Err(err.current_context().err_msg.to_string());
    }
    let processed_result = res.unwrap();

    // ? Execute the swaps if any orders were matched
    let mut retry_messages = Vec::new();
    let mut updated_positions: Vec<(Option<PerpPosition>, Option<PerpPosition>)> = Vec::new();
    let is_open_channel = response_sender.is_some(); // Wheter we should send a response back through the channel
    if let Some(mut swaps) = processed_result.perp_swaps {
        loop {
            if swaps.len() == 0 {
                break;
            }

            let (swap, user_id_a, user_id_b) = swaps.pop().unwrap();

            // let handle = tokio::spawn(execute_perp_swap(
            let res = execute_perp_swap(
                swap,
                tx_batch,
                perp_order_book,
                (user_id_a, user_id_b),
                session,
                backup_storage,
            )
            .await;

            let (retry_msg, position_pair) =
                await_perp_handle(ws_connections, privileged_ws_connections, res, user_id).await;

            if let Some(msg) = retry_msg {
                retry_messages.push(msg);
            } else if let Some((pos_a, pos_b)) = position_pair {
                if is_open_channel {
                    updated_positions.push((pos_a.clone(), pos_b.clone()))
                }

                _update_order_positions_in_swaps(&mut swaps, user_id_a, pos_a, user_id_b, pos_b);
            }
        }

        //
        if let Some(sender) = response_sender {
            if let Err(e) = sender.send(updated_positions) {
                println!("error sending swap response through channel: {:?}", e)
            };
        }
    }

    return Ok((retry_messages, processed_result.new_order_id));
}

pub async fn process_perp_order_request(
    perp_order_book: &Arc<TokioMutex<OrderBook>>,
    perp_order: PerpOrder,
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
    let order_ = Order::Perp(perp_order);
    let order_request = new_limit_order_request(
        side,
        order_,
        signature,
        SystemTime::now(),
        is_market,
        user_id,
    );

    // ? Insert the order into the book and get back the matched results if any
    let mut order_book_m = perp_order_book.lock().await;
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

type HandleResult = (
    Option<(
        (Message, Message),
        (u64, u64),
        (Option<PerpPosition>, Option<PerpPosition>),
        Message,
    )>,
    Option<SwapErrorInfo>,
);

pub async fn await_perp_handle(
    ws_connections: &Arc<TokioMutex<WsConnectionsMap>>,
    privileged_ws_connections: &Arc<TokioMutex<Vec<u64>>>,
    handle_res: HandleResult,
    user_id: u64,
) -> (
    Option<SwapErrorInfo>,
    Option<(Option<PerpPosition>, Option<PerpPosition>)>,
) {
    // ? Wait for the swaps to finish

    // If the swap was successful, send the messages to the users
    if handle_res.0.is_some() {
        let ((msg_a, msg_b), (user_id_a, user_id_b), position_pair, fill_msg) =
            handle_res.0.unwrap();

        // ? Send a message to the user_id websocket
        if let Err(_) = send_direct_message(ws_connections, user_id_a, msg_a).await {
            println!("Error sending swap message")
        };

        // ? Send a message to the user_id websocket
        if let Err(_) = send_direct_message(ws_connections, user_id_b, msg_b).await {
            println!("Error sending swap message")
        };

        // ? Send a filled swap to anyone who's listening
        if let Err(_) =
            broadcast_message(ws_connections, privileged_ws_connections, fill_msg.clone()).await
        {
            println!("Error sending perp swap fill update message")
        };

        // ? Send the new_positions to the relay server
        let position1 = if position_pair.0.is_some() {
            Some((
                position_pair
                    .0
                    .as_ref()
                    .unwrap()
                    .position_header
                    .position_address
                    .to_string(),
                position_pair.0.as_ref().unwrap().index,
                position_pair
                    .0
                    .as_ref()
                    .unwrap()
                    .position_header
                    .synthetic_token,
                position_pair.0.as_ref().unwrap().order_side == OrderSide::Long,
                position_pair.0.as_ref().unwrap().liquidation_price,
            ))
        } else {
            None
        };
        let position2 = if position_pair.1.is_some() {
            Some((
                position_pair
                    .1
                    .as_ref()
                    .unwrap()
                    .position_header
                    .position_address
                    .to_string(),
                position_pair.1.as_ref().unwrap().index,
                position_pair
                    .1
                    .as_ref()
                    .unwrap()
                    .position_header
                    .synthetic_token,
                position_pair.1.as_ref().unwrap().order_side == OrderSide::Long,
                position_pair.1.as_ref().unwrap().liquidation_price,
            ))
        } else {
            None
        };
        if position1.is_some() || position2.is_some() {
            let msg = json!({
                "message_id": "NEW_POSITIONS",
                "position1": position1,
                "position2": position2,
            });
            let msg = Message::Text(msg.to_string());

            if let Err(_) = send_to_relay_server(ws_connections, msg).await {
                println!("Error sending perp swap fill update message")
            };
        }

        return (None, Some(position_pair));
    // If the taker swap failed, try matching it again with another order
    } else if handle_res.1.is_some() {
        let error_res = handle_res.1.unwrap();

        let msg = json!({
            "message_id": "PERP_SWAP_ERROR",
            "error_message": error_res.3,
        });
        let msg = Message::Text(msg.to_string());

        // ? Send a message to the user_id websocket
        if let Err(_) = send_direct_message(ws_connections, user_id, msg).await {
            println!("Error sending perp swap message")
        };

        if error_res.0 == None && error_res.1 == 0 && error_res.2 == 0 {
            return (None, None);
        }

        return (Some(error_res), None);
    }

    (None, None)
}

#[async_recursion]
pub async fn retry_failed_perp_swaps(
    tx_batch: &Arc<TokioMutex<TransactionBatch>>,
    perp_order_book: &Arc<TokioMutex<OrderBook>>,
    session: &Arc<Mutex<ServiceSession>>,
    backup_storage: &Arc<Mutex<BackupStorage>>,
    perp_order: PerpOrder,
    side: OBOrderSide,
    signature: Signature,
    user_id: u64,
    is_market: bool,
    ws_connections: &Arc<TokioMutex<WsConnectionsMap>>,
    privileged_ws_connections: &Arc<TokioMutex<Vec<u64>>>,
    retry_messages: Vec<SwapErrorInfo>,
    failed_counterpart_ids: Option<Vec<u64>>,
) -> std::result::Result<(), String> {
    // ? Retry the failed swaps
    let mut failed_ids = failed_counterpart_ids.unwrap_or_default();
    let mut new_retry_messages = Vec::new();

    for msg_ in retry_messages {
        let (maker_order_id, taker_order_id, qty, _) = msg_;

        if maker_order_id.is_some() {
            failed_ids.push(maker_order_id.unwrap());
        }

        let mut processed_res = process_perp_order_request(
            perp_order_book,
            perp_order.clone(),
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

        match process_and_execute_perp_swaps(
            tx_batch,
            perp_order_book,
            session,
            backup_storage,
            ws_connections,
            privileged_ws_connections,
            None,
            &mut processed_res,
            user_id,
        )
        .await
        {
            Ok((msgs, _oid)) => {
                new_retry_messages.extend(msgs);
            }
            Err(err) => {
                return Err(err);
            }
        };
    }

    if new_retry_messages.len() > 0 {
        retry_failed_perp_swaps(
            tx_batch,
            perp_order_book,
            session,
            backup_storage,
            perp_order.clone(),
            side,
            signature.clone(),
            user_id,
            is_market,
            ws_connections,
            privileged_ws_connections,
            new_retry_messages,
            Some(failed_ids),
        )
        .await?;
    }

    Ok(())
}

//
// * ======================= ==================== ===================== =========================== ====================================
//

// * HELPERS  ---------------------------------------------------------------

fn _get_notes_in(
    order_a: &PerpOrder,
    order_b: &PerpOrder,
) -> ((u64, Option<Vec<Note>>), (u64, Option<Vec<Note>>)) {
    let notes_in_a: (u64, Option<Vec<Note>>);
    let open_order_fields = &order_a.open_order_fields;
    if open_order_fields.is_some() {
        notes_in_a = (
            order_a.order_id,
            Some(
                open_order_fields
                    .as_ref()
                    .unwrap()
                    .notes_in
                    .iter()
                    .map(|x| Note::from(x.clone()))
                    .collect::<Vec<Note>>()
                    .clone(),
            ),
        );
    } else {
        notes_in_a = (0, None);
    }

    let notes_in_b: (u64, Option<Vec<Note>>);
    let open_order_fields = &order_b.open_order_fields;
    if open_order_fields.is_some() {
        notes_in_b = (
            order_b.order_id,
            Some(
                open_order_fields
                    .as_ref()
                    .unwrap()
                    .notes_in
                    .iter()
                    .map(|x| Note::from(x.clone()))
                    .collect::<Vec<Note>>()
                    .clone(),
            ),
        );
    } else {
        notes_in_b = (0, None);
    }

    return (notes_in_a, notes_in_b);
}

fn _update_order_positions_in_swaps(
    swaps: &mut Vec<(PerpSwap, u64, u64)>,
    user_id_a: u64,
    new_position_a: Option<PerpPosition>,
    user_id_b: u64,
    new_position_b: Option<PerpPosition>,
) {
    for (swap, uid_a, uid_b) in swaps.iter_mut() {
        // ? Check if any orders in the swap are from the user_a
        if *uid_a == user_id_a && swap.order_a.position_effect_type != PositionEffectType::Open {
            // ? Update the position in the order with the new position_a
            if let Some(new_position_a_) = &new_position_a {
                swap.order_a.position = Some(new_position_a_.clone());
            }
        } else if *uid_a == user_id_b
            && swap.order_a.position_effect_type != PositionEffectType::Open
        {
            // ? Update the position in the order with the new position_b
            if let Some(new_position_b_) = &new_position_b {
                swap.order_a.position = Some(new_position_b_.clone());
            }
        }

        // ? Check if any orders in the swap are from the user_b
        if *uid_b == user_id_a && swap.order_b.position_effect_type != PositionEffectType::Open {
            // ? Update the position in the order with the new position_a

            if let Some(new_position_a_) = &new_position_a {
                swap.order_b.position = Some(new_position_a_.clone());
            }
        } else if *uid_b == user_id_b
            && swap.order_b.position_effect_type != PositionEffectType::Open
        {
            // ? Update the position in the order with the new position_b
            if let Some(new_position_b_) = &new_position_b {
                swap.order_b.position = Some(new_position_b_.clone());
            }
        }
    }
}
