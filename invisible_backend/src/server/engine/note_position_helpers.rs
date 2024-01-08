use std::{collections::HashMap, sync::Arc};

use super::super::grpc::ChangeMarginMessage;
use super::super::server_helpers::WsConnectionsMap;
use super::super::{
    grpc::engine_proto::{MarginChangeReq, MarginChangeRes, SplitNotesReq, SplitNotesRes},
    server_helpers::engine_helpers::{handle_margin_change_repsonse, handle_split_notes_repsonse},
};
use crate::matching_engine::orderbook::OrderBook;
use crate::transaction_batch::TransactionBatch;

use crate::utils::{
    errors::{send_margin_change_error_reply, send_split_notes_error_reply},
    notes::Note,
};

use tokio::sync::{Mutex as TokioMutex, Semaphore};
use tonic::{Request, Response, Status};

//
// * ===================================================================================================================================
// * SPLIT NOTES

pub async fn split_notes_inner(
    tx_batch: &Arc<TokioMutex<TransactionBatch>>,
    semaphore: &Semaphore,
    is_paused: &Arc<TokioMutex<bool>>,
    //
    req: Request<SplitNotesReq>,
) -> Result<Response<SplitNotesRes>, Status> {
    let _permit = semaphore.acquire().await.unwrap();

    let lock = is_paused.lock().await;
    drop(lock);

    tokio::task::yield_now().await;

    let tx_batch_m = tx_batch.lock().await;
    let transaction_output_json = Arc::clone(&tx_batch_m.transaction_output_json);
    let main_storage = Arc::clone(&tx_batch_m.main_storage);
    drop(tx_batch_m);

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

    let mut tx_batch_m = tx_batch.lock().await;
    let zero_idxs = tx_batch_m.split_notes(notes_in, new_note, refund_note);
    drop(tx_batch_m);

    return handle_split_notes_repsonse(zero_idxs, &transaction_output_json, &main_storage).await;
}

//
// * ===================================================================================================================================
// * EXECUTE WITHDRAWAL

pub async fn change_position_margin_inner(
    tx_batch: &Arc<TokioMutex<TransactionBatch>>,
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

    let tx_batch_m = tx_batch.lock().await;
    let transaction_output_json = Arc::clone(&tx_batch_m.transaction_output_json);
    let main_storage = Arc::clone(&tx_batch_m.main_storage);
    drop(tx_batch_m);

    let perp_order_books = perp_order_books.clone();
    let ws_connections = ws_connections.clone();

    let req: MarginChangeReq = req.into_inner();

    let change_margin_message = ChangeMarginMessage::try_from(req).ok();

    if change_margin_message.is_none() {
        return send_margin_change_error_reply("Invalid change margin message".to_string());
    }

    let user_id = change_margin_message.as_ref().unwrap().user_id;

    let tx_batch_m = tx_batch.lock().await;
    let result = tx_batch_m.change_position_margin(change_margin_message.unwrap());
    drop(tx_batch_m);

    if let Err(_e) = result {
        return send_margin_change_error_reply(
            "Unknown Error occured in the withdrawal execution".to_string(),
        );
    }

    return handle_margin_change_repsonse(
        result.unwrap(),
        user_id,
        &transaction_output_json,
        &main_storage,
        &perp_order_books,
        &ws_connections,
    )
    .await;
}
