use std::{collections::HashMap, sync::Arc};

use super::super::server_helpers::engine_helpers::verify_signature_format;
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
use crate::matching_engine::orders::limit_order_cancel_request;
use crate::transaction_batch::TransactionBatch;
use crate::utils::crypto_utils::Signature;
use crate::utils::errors::send_cancel_order_error_reply;
use crate::{
    matching_engine::{
        domain::OrderSide as OBOrderSide, orderbook::OrderBook, orders::new_amend_order,
    },
    utils::errors::send_amend_order_error_reply,
};

use tokio::sync::Mutex as TokioMutex;
use tonic::{Request, Response, Status};

// * ===================================================================================================================================
//

pub async fn cancel_order_inner(
    tx_batch: &Arc<TokioMutex<TransactionBatch>>,
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

    let tx_batch_m = tx_batch.lock().await;
    let partial_fill_tracker = Arc::clone(&tx_batch_m.partial_fill_tracker);
    let perpetual_partial_fill_tracker = Arc::clone(&tx_batch_m.perpetual_partial_fill_tracker);
    drop(tx_batch_m);

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
    tx_batch: &Arc<TokioMutex<TransactionBatch>>,
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

    let tx_batch_m = tx_batch.lock().await;
    let transaction_output_json = Arc::clone(&tx_batch_m.transaction_output_json);
    let main_storage = Arc::clone(&tx_batch_m.main_storage);
    drop(tx_batch_m);

    if req.is_perp {
        if let Err(e) = execute_perp_swaps_after_amend_order(
            &tx_batch,
            &order_book_m,
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
            &tx_batch,
            &order_book_m,
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

    store_output_json(&transaction_output_json, &main_storage);

    let reply: AmendOrderResponse = AmendOrderResponse {
        successful: true,
        error_message: "".to_string(),
    };

    return Ok(Response::new(reply));
}
