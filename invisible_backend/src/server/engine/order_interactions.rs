use firestore_db_and_auth::ServiceSession;
use parking_lot::Mutex;
use serde_json::Value;
use std::{collections::HashMap, sync::Arc, thread::ThreadId};

use super::super::server_helpers::WsConnectionsMap;
use super::super::{
    grpc::engine_proto::{
        AmendOrderRequest, AmendOrderResponse, CancelOrderMessage, CancelOrderResponse,
    },
    server_helpers::{
        amend_order_execution::{
            execute_perp_swaps_after_amend_order, execute_spot_swaps_after_amend_order,
        },
        engine_helpers::{handle_cancel_order_repsonse, store_output_json},
    },
};
use super::super::{
    grpc::{GrpcMessage, GrpcTxResponse},
    server_helpers::engine_helpers::verify_signature_format,
};
use crate::{
    matching_engine::orders::limit_order_cancel_request,
    perpetual::perp_helpers::perp_rollback::PerpRollbackInfo,
};
use crate::{
    matching_engine::{
        domain::OrderSide as OBOrderSide, orderbook::OrderBook, orders::new_amend_order,
    },
    utils::{
        errors::send_amend_order_error_reply,
        storage::local_storage::{BackupStorage, MainStorage},
    },
};

use crate::transactions::transaction_helpers::rollbacks::RollbackInfo;
use crate::utils::crypto_utils::Signature;
use crate::utils::{errors::send_cancel_order_error_reply, notes::Note};

use tokio::sync::{
    mpsc::Sender as MpscSender, oneshot::Sender as OneshotSender, Mutex as TokioMutex,
};
use tonic::{Request, Response, Status};

// * ===================================================================================================================================
//

pub async fn cancel_order_inner(
    partial_fill_tracker: &Arc<Mutex<HashMap<u64, (Option<Note>, u64)>>>,
    perpetual_partial_fill_tracker: &Arc<Mutex<HashMap<u64, (Option<Note>, u64, u64)>>>,
    order_books: &HashMap<u16, Arc<TokioMutex<OrderBook>>>,
    perp_order_books: &HashMap<u16, Arc<TokioMutex<OrderBook>>>,
    request: Request<CancelOrderMessage>,
) -> Result<Response<CancelOrderResponse>, Status> {
    tokio::task::yield_now().await;

    let req: CancelOrderMessage = request.into_inner();

    let market_id = req.market_id as u16;

    let order_book_m: &Arc<TokioMutex<OrderBook>>;
    if req.is_perp {
        let order_book_m_ = perp_order_books.get(&market_id);
        if order_book_m_.is_none() {
            return send_cancel_order_error_reply("Market not found".to_string());
        }

        order_book_m = order_book_m_.unwrap();
    } else {
        let order_book_m_ = order_books.get(&market_id);
        if order_book_m_.is_none() {
            return send_cancel_order_error_reply("Market not found".to_string());
        }

        order_book_m = order_book_m_.unwrap();
    }

    let order_side: OBOrderSide = if req.order_side {
        OBOrderSide::Bid
    } else {
        OBOrderSide::Ask
    };

    let cancel_request = limit_order_cancel_request(req.order_id, order_side, req.user_id);

    let mut order_book = order_book_m.lock().await;

    let res = order_book.process_order(cancel_request);

    return handle_cancel_order_repsonse(
        &res[0],
        req.is_perp,
        req.order_id,
        &partial_fill_tracker,
        &perpetual_partial_fill_tracker,
    );
}

//
// * ===================================================================================================================================
//

pub async fn amend_order_inner(
    mpsc_tx: &MpscSender<(GrpcMessage, OneshotSender<GrpcTxResponse>)>,
    session: &Arc<Mutex<ServiceSession>>,
    main_storage: &Arc<Mutex<MainStorage>>,
    backup_storage: &Arc<Mutex<BackupStorage>>,
    swap_output_json: &Arc<Mutex<Vec<serde_json::Map<String, Value>>>>,
    rollback_safeguard: &Arc<Mutex<HashMap<ThreadId, RollbackInfo>>>,
    perp_rollback_safeguard: &Arc<Mutex<HashMap<ThreadId, PerpRollbackInfo>>>,
    order_books: &HashMap<u16, Arc<TokioMutex<OrderBook>>>,
    perp_order_books: &HashMap<u16, Arc<TokioMutex<OrderBook>>>,
    ws_connections: &Arc<TokioMutex<WsConnectionsMap>>,
    privileged_ws_connections: &Arc<TokioMutex<Vec<u64>>>,
    request: Request<AmendOrderRequest>,
) -> Result<Response<AmendOrderResponse>, Status> {
    tokio::task::yield_now().await;

    let req: AmendOrderRequest = request.into_inner();

    // ? Verify the signature is defined and has a valid format
    let signature: Signature;
    match verify_signature_format(&req.signature) {
        Ok(sig) => signature = sig,
        Err(err) => {
            return send_amend_order_error_reply(err);
        }
    }

    let market_id = req.market_id as u16;

    let order_book_m: &Arc<TokioMutex<OrderBook>>;
    if req.is_perp {
        let order_book_m_ = perp_order_books.get(&market_id);
        if order_book_m_.is_none() {
            return send_amend_order_error_reply("Market not found".to_string());
        }

        order_book_m = order_book_m_.unwrap();
    } else {
        let order_book_m_ = order_books.get(&market_id);
        if order_book_m_.is_none() {
            return send_amend_order_error_reply("Market not found".to_string());
        }

        order_book_m = order_book_m_.unwrap();
    }

    let order_side: OBOrderSide = if req.order_side {
        OBOrderSide::Bid
    } else {
        OBOrderSide::Ask
    };

    let amend_request = new_amend_order(
        req.order_id,
        order_side,
        req.user_id,
        req.new_price,
        req.new_expiration,
        signature.clone(),
        req.match_only,
    );

    let mut order_book = order_book_m.lock().await;
    let mut processed_res = order_book.process_order(amend_request);
    drop(order_book);

    if req.is_perp {
        if let Err(e) = execute_perp_swaps_after_amend_order(
            &mpsc_tx,
            &perp_rollback_safeguard,
            &order_book_m,
            &session,
            &backup_storage,
            &ws_connections,
            &privileged_ws_connections,
            &mut processed_res,
            req.order_id,
            order_side,
            signature,
            req.user_id,
        )
        .await
        {
            return send_amend_order_error_reply(e);
        }
    } else {
        if let Err(e) = execute_spot_swaps_after_amend_order(
            &mpsc_tx,
            &rollback_safeguard,
            &order_book_m,
            &session,
            &backup_storage,
            processed_res,
            &ws_connections,
            &privileged_ws_connections,
            req.order_id,
            order_side,
            signature,
            req.user_id,
        )
        .await
        {
            return send_amend_order_error_reply(e);
        }
    }

    store_output_json(&swap_output_json, &main_storage);

    let reply: AmendOrderResponse = AmendOrderResponse {
        successful: true,
        error_message: "".to_string(),
    };

    return Ok(Response::new(reply));
}
