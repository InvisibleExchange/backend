use parking_lot::Mutex;
use serde_json::Value;
use std::sync::Arc;

use super::super::grpc::engine_proto::{
    DepositMessage, DepositResponse, SuccessResponse, WithdrawalMessage,
};

use crate::server::{
    grpc::{GrpcMessage, GrpcTxResponse, MessageType},
    server_helpers::engine_helpers::{handle_deposit_repsonse, handle_withdrawal_repsonse},
};
use crate::utils::storage::local_storage::MainStorage;

use crate::transactions::{deposit::Deposit, withdrawal::Withdrawal};
use crate::utils::errors::{send_deposit_error_reply, send_withdrawal_error_reply};

use tokio::sync::{
    mpsc::Sender as MpscSender,
    oneshot::{self, Sender as OneshotSender},
    Mutex as TokioMutex, Semaphore,
};
use tokio::task::JoinHandle as TokioJoinHandle;
use tonic::{Request, Response, Status};

//
// * ===================================================================================================================================
// * EXECUTE WITHDRAWAL

pub async fn execute_deposit_inner(
    mpsc_tx: &MpscSender<(GrpcMessage, OneshotSender<GrpcTxResponse>)>,
    main_storage: &Arc<Mutex<MainStorage>>,
    swap_output_json: &Arc<Mutex<Vec<serde_json::Map<String, Value>>>>,
    semaphore: &Semaphore,
    is_paused: &Arc<TokioMutex<bool>>,
    //
    request: Request<DepositMessage>,
) -> Result<Response<DepositResponse>, Status> {
    let _permit = semaphore.acquire().await.unwrap();

    let lock = is_paused.lock().await;
    drop(lock);

    tokio::task::yield_now().await;

    let transaction_mpsc_tx = mpsc_tx.clone();
    let swap_output_json = swap_output_json.clone();
    let main_storage = main_storage.clone();

    let handle = tokio::spawn(async move {
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

        let handle: TokioJoinHandle<GrpcTxResponse> = tokio::spawn(async move {
            let (resp_tx, resp_rx) = oneshot::channel();

            let mut grpc_message = GrpcMessage::new();
            grpc_message.msg_type = MessageType::DepositMessage;
            grpc_message.deposit_message = Some(deposit);

            transaction_mpsc_tx
                .send((grpc_message, resp_tx))
                .await
                .ok()
                .unwrap();
            return resp_rx.await.unwrap();
        });

        return handle_deposit_repsonse(handle, &swap_output_json, &main_storage).await;
    });

    match handle.await {
        Ok(res) => {
            return res;
        }
        Err(_e) => {
            return send_deposit_error_reply(
                "Unknown Error occured in the withdrawal execution".to_string(),
            );
        }
    }
}

//
// * ===================================================================================================================================
// * EXECUTE WITHDRAWAL

pub async fn execute_withdrawal_inner(
    mpsc_tx: &MpscSender<(GrpcMessage, OneshotSender<GrpcTxResponse>)>,
    main_storage: &Arc<Mutex<MainStorage>>,
    swap_output_json: &Arc<Mutex<Vec<serde_json::Map<String, Value>>>>,
    semaphore: &Semaphore,
    is_paused: &Arc<TokioMutex<bool>>,
    request: Request<WithdrawalMessage>,
) -> Result<Response<SuccessResponse>, Status> {
    let _permit = semaphore.acquire().await.unwrap();

    let lock = is_paused.lock().await;
    drop(lock);

    tokio::task::yield_now().await;

    let transaction_mpsc_tx = mpsc_tx.clone();
    let swap_output_json = swap_output_json.clone();
    let main_storage = main_storage.clone();

    let handle = tokio::spawn(async move {
        let req: WithdrawalMessage = request.into_inner();

        let withdrawal: Withdrawal;
        match Withdrawal::try_from(req) {
            Ok(w) => withdrawal = w,
            Err(_e) => {
                return send_withdrawal_error_reply(
                    "Erroc unpacking the withdrawal message (verify the format is correct)"
                        .to_string(),
                );
            }
        };

        let handle: TokioJoinHandle<GrpcTxResponse> = tokio::spawn(async move {
            let (resp_tx, resp_rx) = oneshot::channel();

            let mut grpc_message = GrpcMessage::new();
            grpc_message.msg_type = MessageType::WithdrawalMessage;
            grpc_message.withdrawal_message = Some(withdrawal);

            transaction_mpsc_tx
                .send((grpc_message, resp_tx))
                .await
                .ok()
                .unwrap();
            return resp_rx.await.unwrap();
        });

        return handle_withdrawal_repsonse(handle, &swap_output_json, &main_storage).await;
    });

    match handle.await {
        Ok(res) => {
            return res;
        }
        Err(_e) => {
            return send_withdrawal_error_reply(
                "Unknown Error occured in the withdrawal execution".to_string(),
            );
        }
    }
}
