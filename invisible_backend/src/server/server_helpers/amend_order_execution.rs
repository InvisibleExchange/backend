use super::perp_swap_execution::{process_and_execute_perp_swaps, retry_failed_perp_swaps};
use super::swap_execution::{
    handle_swap_execution_results, process_and_execute_spot_swaps, retry_failed_swaps,
};

use std::sync::Arc;

use tokio::sync::Mutex as TokioMutex;

use crate::matching_engine::orderbook::{Failed, Success};
use crate::matching_engine::{
    domain::{Order, OrderSide as OBOrderSide},
    orderbook::OrderBook,
};
use crate::perpetual::perp_order::PerpOrder;
use crate::transaction_batch::TransactionBatch;
use crate::transactions::limit_order::LimitOrder;

use crate::utils::crypto_utils::Signature;

use super::WsConnectionsMap;

pub async fn execute_spot_swaps_after_amend_order(
    tx_batch: &Arc<TokioMutex<TransactionBatch>>,
    order_book: &Arc<TokioMutex<OrderBook>>,
    mut processed_res: Vec<std::result::Result<Success, Failed>>,
    ws_connections: &Arc<TokioMutex<WsConnectionsMap>>,
    privileged_ws_connections: &Arc<TokioMutex<Vec<u64>>>,
    //
    order_id: u64,
    order_side: OBOrderSide,
    signature: Signature,
    user_id: u64,
) -> Result<(), String> {
    let tx_batch_m = tx_batch.lock().await;
    let session = Arc::clone(&tx_batch_m.firebase_session);
    let backup_storage = Arc::clone(&tx_batch_m.backup_storage);
    drop(tx_batch_m);

    // This matches the orders and creates the swaps that can be executed
    let handles;
    match process_and_execute_spot_swaps(
        tx_batch,
        order_book,
        &session,
        &backup_storage,
        &mut processed_res,
    )
    .await
    {
        Ok((h, _)) => {
            handles = h;
        }
        Err(err) => {
            return Err(err);
        }
    };

    let retry_messages;
    match handle_swap_execution_results(ws_connections, privileged_ws_connections, handles, user_id)
        .await
    {
        Ok(rm) => retry_messages = rm,
        Err(e) => return Err(e),
    };

    if retry_messages.len() > 0 {
        let order_book_ = order_book.lock().await;
        let order_wrapper = order_book_.get_order(order_id);
        drop(order_book_);

        if order_wrapper.is_none() {
            return Err("Order not found".to_string());
        }

        let wrapper = order_wrapper.unwrap();
        let limit_order: LimitOrder;
        if let Order::Spot(limit_order_) = wrapper.order {
            limit_order = limit_order_.clone();
        } else {
            return Err("Order not found".to_string());
        }

        if let Err(e) = retry_failed_swaps(
            tx_batch,
            order_book,
            &session,
            &backup_storage,
            limit_order.clone(),
            order_side,
            signature,
            user_id,
            true,
            &ws_connections,
            &privileged_ws_connections,
            retry_messages,
            None,
        )
        .await
        {
            return Err(e);
        }
    }

    Ok(())
}

pub async fn execute_perp_swaps_after_amend_order(
    tx_batch: &Arc<TokioMutex<TransactionBatch>>,
    perp_order_book: &Arc<TokioMutex<OrderBook>>,
    ws_connections: &Arc<TokioMutex<WsConnectionsMap>>,
    privileged_ws_connections: &Arc<TokioMutex<Vec<u64>>>,
    processed_res: &mut Vec<std::result::Result<Success, Failed>>,
    //
    order_id: u64,
    order_side: OBOrderSide,
    signature: Signature,
    user_id: u64,
) -> Result<(), String> {
    let tx_batch_m = tx_batch.lock().await;
    let session = Arc::clone(&tx_batch_m.firebase_session);
    let backup_storage = Arc::clone(&tx_batch_m.backup_storage);
    drop(tx_batch_m);

    // This matches the orders and creates the swaps that can be executed
    let retry_messages;
    match process_and_execute_perp_swaps(
        tx_batch,
        perp_order_book,
        &session,
        &backup_storage,
        ws_connections,
        privileged_ws_connections,
        processed_res,
        user_id,
    )
    .await
    {
        Ok((h, _)) => {
            retry_messages = h;
        }
        Err(err) => {
            return Err(err);
        }
    };

    if retry_messages.len() > 0 {
        let order_book_ = perp_order_book.lock().await;
        let order_wrapper = order_book_.get_order(order_id);
        drop(order_book_);

        if order_wrapper.is_none() {
            return Err("Order not found".to_string());
        }

        let wrapper = order_wrapper.unwrap();
        let perp_order: PerpOrder;
        if let Order::Perp(perp_order_) = wrapper.order {
            perp_order = perp_order_.clone();
        } else {
            return Err("Order not found".to_string());
        }

        if let Err(e) = retry_failed_perp_swaps(
            tx_batch,
            perp_order_book,
            &session,
            &backup_storage,
            perp_order.clone(),
            order_side,
            signature,
            user_id,
            true,
            ws_connections,
            privileged_ws_connections,
            retry_messages,
            None,
        )
        .await
        {
            return Err(e);
        }
    }

    Ok(())
}
