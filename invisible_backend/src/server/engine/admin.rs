use std::{collections::HashMap, sync::Arc, time::Instant};

use super::super::grpc::engine_proto::{
    EmptyReq, FinalizeBatchResponse, OracleUpdateReq, RestoreOrderBookMessage,
    SpotOrderRestoreMessage, SuccessResponse,
};

use crate::transaction_batch::TransactionBatch;
use crate::{
    matching_engine::orderbook::OrderBook, transaction_batch::tx_batch_structs::OracleUpdate,
};

use crate::utils::errors::send_oracle_update_error_reply;

use tokio::sync::{Mutex as TokioMutex, Semaphore};
use tonic::{Request, Response, Status};

pub async fn finalize_batch_inner(
    tx_batch: &Arc<TokioMutex<TransactionBatch>>,
    semaphore: &Semaphore,
    is_paused: &Arc<TokioMutex<bool>>,
    //
    _: Request<EmptyReq>,
) -> Result<Response<FinalizeBatchResponse>, Status> {
    let _permit = semaphore.acquire().await.unwrap();

    let lock = is_paused.lock().await;

    tokio::task::yield_now().await;

    let now = Instant::now();

    let mut tx_batch_m = tx_batch.lock().await;
    let success = tx_batch_m.finalize_batch().is_ok();
    drop(tx_batch_m);

    println!("time: {:?}", now.elapsed());

    if success {
        println!("batch finalized sucessfuly");
    } else {
        println!("batch finalization failed");
    }

    drop(lock);

    return Ok(Response::new(FinalizeBatchResponse {}));
}

pub async fn update_index_price_inner(
    tx_batch: &Arc<TokioMutex<TransactionBatch>>,
    //
    request: Request<OracleUpdateReq>,
) -> Result<Response<SuccessResponse>, Status> {
    tokio::task::yield_now().await;

    let req: OracleUpdateReq = request.into_inner();

    let mut oracle_updates: Vec<OracleUpdate> = Vec::new();
    for update in req.oracle_price_updates {
        match OracleUpdate::try_from(update) {
            Ok(oracle_update) => oracle_updates.push(oracle_update),
            Err(err) => {
                return send_oracle_update_error_reply(format!(
                    "Error occurred while parsing the oracle update: {:?}",
                    err.current_context()
                ));
            }
        }
    }

    let mut tx_batch_m = tx_batch.lock().await;
    let updated_prices = tx_batch_m.update_index_prices(oracle_updates);
    drop(tx_batch_m);

    match updated_prices {
        Ok(_) => {
            let reply = SuccessResponse {
                successful: true,
                error_message: "".to_string(),
            };

            return Ok(Response::new(reply));
        }
        Err(err) => {
            return send_oracle_update_error_reply(format!(
                "Error occurred while updating the index price: {:?}",
                err.current_context()
            ));
        }
    }
}

pub async fn restore_orderbook_inner(
    order_books: &HashMap<u16, Arc<TokioMutex<OrderBook>>>,
    perp_order_books: &HashMap<u16, Arc<TokioMutex<OrderBook>>>,
    //
    request: Request<RestoreOrderBookMessage>,
) -> Result<Response<SuccessResponse>, Status> {
    tokio::task::yield_now().await;

    let req: RestoreOrderBookMessage = request.into_inner();

    let spot_order_messages: Vec<SpotOrderRestoreMessage> = req.spot_order_restore_messages;

    for message in spot_order_messages {
        let market_id = message.market_id as u16;

        let bid_order_restore_messages = message.bid_order_restore_messages;
        let ask_order_restore_messages = message.ask_order_restore_messages;

        let order_book_ = order_books.get(&market_id);
        if let Some(order_book) = order_book_ {
            let mut order_book = order_book.lock().await;

            order_book
                .restore_spot_order_book(bid_order_restore_messages, ask_order_restore_messages);
        }
    }

    // ? ===========================================================================================

    let perp_order_messages = req.perp_order_restore_messages;

    for message in perp_order_messages {
        let market_id = message.market_id as u16;

        let bid_order_restore_messages = message.bid_order_restore_messages;
        let ask_order_restore_messages = message.ask_order_restore_messages;

        let order_book_ = perp_order_books.get(&market_id);
        if let Some(order_book) = order_book_ {
            let mut order_book = order_book.lock().await;

            order_book
                .restore_perp_order_book(bid_order_restore_messages, ask_order_restore_messages);
        }
    }

    let reply = SuccessResponse {
        successful: true,
        error_message: "".to_string(),
    };

    return Ok(Response::new(reply));
}
