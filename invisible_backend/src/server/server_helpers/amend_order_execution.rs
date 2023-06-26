use super::perp_swap_execution::{process_and_execute_perp_swaps, retry_failed_perp_swaps};
use super::swap_execution::{
    await_swap_handles, process_and_execute_spot_swaps, retry_failed_swaps,
};

use std::thread::ThreadId;

use std::{collections::HashMap, sync::Arc};

use firestore_db_and_auth::ServiceSession;
use tokio::sync::Mutex as TokioMutex;

use parking_lot::Mutex;

use crate::matching_engine::orderbook::{Failed, Success};
use crate::matching_engine::{
    domain::{Order, OrderSide as OBOrderSide},
    orderbook::OrderBook,
};
use crate::perpetual::perp_helpers::perp_rollback::PerpRollbackInfo;
use crate::perpetual::perp_order::PerpOrder;
use crate::transactions::limit_order::LimitOrder;
use crate::transactions::transaction_helpers::rollbacks::RollbackInfo;

use crate::utils::crypto_utils::Signature;
use crate::utils::storage::BackupStorage;

use tokio::sync::{mpsc::Sender as MpscSender, oneshot::Sender as OneshotSender};

use super::super::grpc::{GrpcMessage, GrpcTxResponse};
use super::WsConnectionsMap;

pub async fn execute_spot_swaps_after_amend_order(
    mpsc_tx: &MpscSender<(GrpcMessage, OneshotSender<GrpcTxResponse>)>,
    rollback_safeguard: &Arc<Mutex<HashMap<ThreadId, RollbackInfo>>>,
    order_book: &Arc<TokioMutex<OrderBook>>,
    session: &Arc<Mutex<ServiceSession>>,
    backup_storage: &Arc<Mutex<BackupStorage>>,
    mut processed_res: Vec<std::result::Result<Success, Failed>>,
    ws_connections: &Arc<TokioMutex<WsConnectionsMap>>,
    privileged_ws_connections: &Arc<TokioMutex<Vec<u64>>>,
    //
    order_id: u64,
    order_side: OBOrderSide,
    signature: Signature,
    user_id: u64,
) -> Result<(), String> {
    // This matches the orders and creates the swaps that can be executed
    let handles;

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
        Ok((h, _)) => {
            handles = h;
        }
        Err(err) => {
            return Err(err);
        }
    };

    

    let retry_messages;
    match await_swap_handles(ws_connections, privileged_ws_connections, handles, user_id).await {
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

        let w = order_wrapper.unwrap();
        let ord = &w.order.lock().order.clone();
        let limit_order: LimitOrder;
        if let Order::Spot(limit_order_) = ord {
            limit_order = limit_order_.clone();
        } else {
            return Err("Order not found".to_string());
        }

        if let Err(e) = retry_failed_swaps(
            &mpsc_tx,
            &rollback_safeguard,
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
    mpsc_tx: &MpscSender<(GrpcMessage, OneshotSender<GrpcTxResponse>)>,
    perp_rollback_safeguard: &Arc<Mutex<HashMap<ThreadId, PerpRollbackInfo>>>,
    perp_order_book: &Arc<TokioMutex<OrderBook>>,
    session: &Arc<Mutex<ServiceSession>>,
    backup_storage: &Arc<Mutex<BackupStorage>>,
    ws_connections: &Arc<TokioMutex<WsConnectionsMap>>,
    privileged_ws_connections: &Arc<TokioMutex<Vec<u64>>>,
    processed_res: &mut Vec<std::result::Result<Success, Failed>>,
    //
    order_id: u64,
    order_side: OBOrderSide,
    signature: Signature,
    user_id: u64,
) -> Result<(), String> {
    // This matches the orders and creates the swaps that can be executed
    let retry_messages;

    match process_and_execute_perp_swaps(
        mpsc_tx,
        perp_rollback_safeguard,
        perp_order_book,
        session,
        backup_storage,
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

        let w = order_wrapper.unwrap();
        let ord = &w.order.lock().order.clone();
        let perp_order: PerpOrder;
        if let Order::Perp(perp_order_) = ord {
            perp_order = perp_order_.clone();
        } else {
            return Err("Order not found".to_string());
        }
        drop(ord);

        if let Err(e) = retry_failed_perp_swaps(
            mpsc_tx,
            perp_rollback_safeguard,
            perp_order_book,
            session,
            backup_storage,
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
