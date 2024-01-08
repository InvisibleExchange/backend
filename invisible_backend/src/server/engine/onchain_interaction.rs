use std::sync::Arc;

use super::super::grpc::engine_proto::{
    DepositMessage, DepositResponse, SuccessResponse, WithdrawalMessage,
};

use crate::{
    server::{
        grpc::engine_proto::EscapeMessage,
        server_helpers::engine_helpers::{
            handle_deposit_repsonse, handle_withdrawal_repsonse, store_output_json,
        },
    },
    transaction_batch::TransactionBatch,
};

use crate::transactions::{deposit::Deposit, withdrawal::Withdrawal};
use crate::utils::errors::{send_deposit_error_reply, send_withdrawal_error_reply};

use tokio::sync::{Mutex as TokioMutex, Semaphore};
use tonic::{Request, Response, Status};

//
// * ===================================================================================================================================
// * EXECUTE WITHDRAWAL

pub async fn execute_deposit_inner(
    tx_batch: &Arc<TokioMutex<TransactionBatch>>,
    semaphore: &Semaphore,
    is_paused: &Arc<TokioMutex<bool>>,
    //
    request: Request<DepositMessage>,
) -> Result<Response<DepositResponse>, Status> {
    let _permit = semaphore.acquire().await.unwrap();

    let lock = is_paused.lock().await;
    drop(lock);

    tokio::task::yield_now().await;

    let tx_batch_m = tx_batch.lock().await;
    let transaction_output_json = Arc::clone(&tx_batch_m.transaction_output_json);
    let main_storage = Arc::clone(&tx_batch_m.main_storage);
    drop(tx_batch_m);

    let req: DepositMessage = request.into_inner();

    let deposit: Deposit;
    match Deposit::try_from(req) {
        Ok(d) => deposit = d,
        Err(_e) => {
            return send_deposit_error_reply(
                "Erroc unpacking the swap message (verify the format is correct)".to_string(),
            );
        }
    };

    let mut tx_batch_m = tx_batch.lock().await;
    let deposit_handle = tx_batch_m.execute_transaction(deposit);
    drop(tx_batch_m);

    let deposit_response = deposit_handle.join();

    if let Err(_e) = deposit_response {
        return send_deposit_error_reply(
            "Unknown Error occured in the deposit execution".to_string(),
        );
    }

    return handle_deposit_repsonse(deposit_response.unwrap(), &transaction_output_json, &main_storage)
        .await;
}

//
// * ===================================================================================================================================
// * EXECUTE WITHDRAWAL

pub async fn execute_withdrawal_inner(
    tx_batch: &Arc<TokioMutex<TransactionBatch>>,
    semaphore: &Semaphore,
    is_paused: &Arc<TokioMutex<bool>>,
    request: Request<WithdrawalMessage>,
) -> Result<Response<SuccessResponse>, Status> {
    let _permit = semaphore.acquire().await.unwrap();

    let lock = is_paused.lock().await;
    drop(lock);

    tokio::task::yield_now().await;

    let tx_batch_m = tx_batch.lock().await;
    let transaction_output_json = Arc::clone(&tx_batch_m.transaction_output_json);
    let main_storage = Arc::clone(&tx_batch_m.main_storage);
    drop(tx_batch_m);

    let req: WithdrawalMessage = request.into_inner();

    let withdrawal: Withdrawal;
    match Withdrawal::try_from(req) {
        Ok(w) => withdrawal = w,
        Err(_e) => {
            return send_withdrawal_error_reply(
                "Erroc unpacking the withdrawal message (verify the format is correct)".to_string(),
            );
        }
    };

    let mut tx_batch_m = tx_batch.lock().await;
    let withdrawal_handle = tx_batch_m.execute_transaction(withdrawal);
    drop(tx_batch_m);

    let withdrawal_response = withdrawal_handle.join();

    if let Err(_e) = withdrawal_response {
        return send_withdrawal_error_reply(
            "Unknown Error occured in the withdrawal execution".to_string(),
        );
    }

    return handle_withdrawal_repsonse(
        withdrawal_response.unwrap(),
        &transaction_output_json,
        &main_storage,
    )
    .await;
}

//
// * ===================================================================================================================================
// * EXECUTE ESCAPE

pub async fn execute_escape_inner(
    tx_batch: &Arc<TokioMutex<TransactionBatch>>,
    semaphore: &Semaphore,
    is_paused: &Arc<TokioMutex<bool>>,
    request: Request<EscapeMessage>,
) -> Result<Response<SuccessResponse>, Status> {
    let _permit = semaphore.acquire().await.unwrap();

    let lock = is_paused.lock().await;
    drop(lock);

    tokio::task::yield_now().await;

    let escape_message: EscapeMessage = request.into_inner();

    let mut tx_batch_m = tx_batch.lock().await;

    tx_batch_m.execute_forced_escape(escape_message);
    store_output_json(&tx_batch_m.transaction_output_json, &tx_batch_m.main_storage);

    drop(tx_batch_m);

    let reply = SuccessResponse {
        successful: true,
        error_message: "".to_string(),
    };

    return Ok(Response::new(reply));
}
