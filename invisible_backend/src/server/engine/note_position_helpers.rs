use parking_lot::Mutex;
use serde_json::Value;
use std::{collections::HashMap, sync::Arc};

use super::super::grpc::{ChangeMarginMessage, GrpcMessage, GrpcTxResponse, MessageType};
use super::super::server_helpers::WsConnectionsMap;
use super::super::{
    grpc::engine_proto::{MarginChangeReq, MarginChangeRes, SplitNotesReq, SplitNotesRes},
    server_helpers::engine_helpers::{handle_margin_change_repsonse, handle_split_notes_repsonse},
};
use crate::{matching_engine::orderbook::OrderBook, utils::storage::local_storage::MainStorage};

use crate::utils::{
    errors::{send_margin_change_error_reply, send_split_notes_error_reply},
    notes::Note,
};

use tokio::sync::{
    mpsc::Sender as MpscSender,
    oneshot::{self, Sender as OneshotSender},
    Mutex as TokioMutex, Semaphore,
};
use tokio::task::JoinHandle as TokioJoinHandle;
use tonic::{Request, Response, Status};

//
// * ===================================================================================================================================
// * SPLIT NOTES

pub async fn split_notes_inner(
    mpsc_tx: &MpscSender<(GrpcMessage, OneshotSender<GrpcTxResponse>)>,
    main_storage: &Arc<Mutex<MainStorage>>,
    swap_output_json: &Arc<Mutex<Vec<serde_json::Map<String, Value>>>>,
    semaphore: &Semaphore,
    is_paused: &Arc<TokioMutex<bool>>,
    //
    req: Request<SplitNotesReq>,
) -> Result<Response<SplitNotesRes>, Status> {
    let _permit = semaphore.acquire().await.unwrap();

    let lock = is_paused.lock().await;
    drop(lock);

    tokio::task::yield_now().await;

    let control_mpsc_tx = mpsc_tx.clone();
    let swap_output_json = swap_output_json.clone();
    let main_storage = main_storage.clone();

    let handle = tokio::spawn(async move {
        let req: SplitNotesReq = req.into_inner();

        let mut notes_in: Vec<Note> = Vec::new();
        for n in req.notes_in.iter() {
            let note = Note::try_from(n.clone());

            if let Ok(n) = note {
                notes_in.push(n);
            } else {
                return send_split_notes_error_reply("Invalid note".to_string());
            }
        }
        let new_note: Note;
        let mut refund_note: Option<Note> = None;
        if req.note_out.is_some() {
            let note_out = Note::try_from(req.note_out.unwrap());

            if let Ok(n) = note_out {
                new_note = n;
            } else {
                return send_split_notes_error_reply("Invalid note".to_string());
            }
        } else {
            return send_split_notes_error_reply("Invalid note".to_string());
        }
        if req.refund_note.is_some() {
            let refund_note_ = Note::try_from(req.refund_note.unwrap());

            if let Ok(n) = refund_note_ {
                refund_note = Some(n);
            } else {
                return send_split_notes_error_reply("Invalid note".to_string());
            }
        }

        let handle: TokioJoinHandle<GrpcTxResponse> = tokio::spawn(async move {
            let (resp_tx, resp_rx) = oneshot::channel();

            let mut grpc_message = GrpcMessage::new();
            grpc_message.msg_type = MessageType::SplitNotes;
            grpc_message.split_notes_message = Some((notes_in, new_note, refund_note));

            control_mpsc_tx
                .send((grpc_message, resp_tx))
                .await
                .ok()
                .unwrap();

            return resp_rx.await.unwrap();
        });

        return handle_split_notes_repsonse(handle, &swap_output_json, &main_storage).await;
    });

    match handle.await {
        Ok(res) => {
            return res;
        }
        Err(_e) => {
            return send_split_notes_error_reply(
                "Unexpected error occured splitting notes".to_string(),
            );
        }
    }
}

//
// * ===================================================================================================================================
// * EXECUTE WITHDRAWAL

pub async fn change_position_margin_inner(
    mpsc_tx: &MpscSender<(GrpcMessage, OneshotSender<GrpcTxResponse>)>,
    main_storage: &Arc<Mutex<MainStorage>>,
    swap_output_json: &Arc<Mutex<Vec<serde_json::Map<String, Value>>>>,
    perp_order_books: &HashMap<u16, Arc<TokioMutex<OrderBook>>>,
    ws_connections: &Arc<TokioMutex<WsConnectionsMap>>,
    semaphore: &Semaphore,
    is_paused: &Arc<TokioMutex<bool>>,
    //
    req: Request<MarginChangeReq>,
) -> Result<Response<MarginChangeRes>, Status> {
    let _permit = semaphore.acquire().await.unwrap();

    let lock = is_paused.lock().await;
    drop(lock);

    tokio::task::yield_now().await;

    let control_mpsc_tx = mpsc_tx.clone();
    let swap_output_json = swap_output_json.clone();
    let main_storage = main_storage.clone();
    let perp_order_books = perp_order_books.clone();
    let ws_connections = ws_connections.clone();

    let handle = tokio::spawn(async move {
        let req: MarginChangeReq = req.into_inner();

        let change_margin_message = ChangeMarginMessage::try_from(req).ok();

        if change_margin_message.is_none() {
            return send_margin_change_error_reply("Invalid change margin message".to_string());
        }

        let user_id = change_margin_message.as_ref().unwrap().user_id;

        let handle: TokioJoinHandle<GrpcTxResponse> = tokio::spawn(async move {
            let (resp_tx, resp_rx) = oneshot::channel();

            let mut grpc_message = GrpcMessage::new();
            grpc_message.msg_type = MessageType::MarginChange;
            grpc_message.change_margin_message = change_margin_message;

            control_mpsc_tx
                .send((grpc_message, resp_tx))
                .await
                .ok()
                .unwrap();

            return resp_rx.await.unwrap();
        });

        return handle_margin_change_repsonse(
            handle,
            user_id,
            &swap_output_json,
            &main_storage,
            &perp_order_books,
            &ws_connections,
        )
        .await;
    });

    match handle.await {
        Ok(res) => {
            return res;
        }
        Err(_e) => {
            println!("Unexpected error occured updating margin");

            return send_margin_change_error_reply(
                "Unexpected error occured updating margin".to_string(),
            );
        }
    }
}
