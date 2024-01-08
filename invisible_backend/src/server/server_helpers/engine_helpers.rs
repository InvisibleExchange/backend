use error_stack::Report;
use num_bigint::BigUint;
use num_traits::{FromPrimitive, Zero};
use parking_lot::Mutex;
use serde_json::json;
use starknet::curve::AffinePoint;
use std::{collections::HashMap, str::FromStr, sync::Arc};
use tokio_tungstenite::tungstenite::Message;
use tonic::{Response, Status};

use crate::{
    matching_engine::orderbook::{Failed, OrderBook, Success},
    order_tab::OrderTab,
    perpetual::{perp_position::PerpPosition, OrderSide},
    server::grpc::{
        engine_proto::{
            CancelOrderResponse, DepositResponse, GrpcNote, MarginChangeRes,
            Signature as GrpcSignature, SplitNotesRes, SuccessResponse,
        },
        ChangeMarginMessage,
    },
    transaction_batch::TxOutputJson,
    transactions::swap::SwapResponse,
    trees::superficial_tree::SuperficialTree,
    utils::{
        errors::{
            send_cancel_order_error_reply, send_deposit_error_reply, send_split_notes_error_reply,
            send_withdrawal_error_reply, TransactionExecutionError,
        },
        storage::local_storage::MainStorage,
    },
};
use tokio::sync::Mutex as TokioMutex;

use crate::utils::crypto_utils::{hash_many, verify, EcPoint, Signature};

use crate::utils::notes::Note;

use super::{send_to_relay_server, WsConnectionsMap, PERP_MARKET_IDS};

pub fn verify_signature_format(sig: &Option<GrpcSignature>) -> Result<Signature, String> {
    // ? Verify the signature is defined and has a valid format
    let signature: Signature;
    if sig.is_none() {
        return Err("Signature is missing".to_string());
    }
    match Signature::try_from(sig.as_ref().unwrap().clone()) {
        Ok(sig) => signature = sig,
        Err(_e) => {
            return Err("Signature format is invalid".to_string());
        }
    }

    return Ok(signature);
}

pub fn verify_notes_existence(
    notes_in: &Vec<Note>,
    state_tree: &Arc<Mutex<SuperficialTree>>,
) -> Result<(), String> {
    let tree = state_tree.lock();

    for note in notes_in {
        let leaf_hash = tree.get_leaf_by_index(note.index);

        if leaf_hash != note.hash {
            return Err("Note does not exist".to_string());
        }
    }

    Ok(())
}

pub fn verify_tab_existence(
    tab: &Arc<Mutex<OrderTab>>,
    tab_state_tree: &Arc<Mutex<SuperficialTree>>,
) -> Result<(), String> {
    let tree = tab_state_tree.lock();

    let tab = tab.lock();

    let tab_hash = tree.get_leaf_by_index(tab.tab_idx as u64);

    if tab_hash != tab.hash {
        return Err("Order tab does not exist".to_string());
    }

    drop(tab);

    Ok(())
}

pub fn verify_position_existence(
    position: &PerpPosition,
    state_tree: &Arc<Mutex<SuperficialTree>>,
) -> Result<(), String> {
    if position.hash != position.hash_position() {
        return Err("Position hash not valid".to_string());
    }

    let tree = state_tree.lock();

    let leaf_hash = tree.get_leaf_by_index(position.index as u64);

    if leaf_hash != position.hash {
        return Err("Position does not exist".to_string());
    }

    Ok(())
}

pub fn verify_margin_change_signature(margin_change: &ChangeMarginMessage) -> Result<(), String> {
    // ? Verify the signature is defined and has a valid format
    let msg_hash = hash_margin_change_message(margin_change);

    if margin_change.margin_change >= 0 {
        let mut pub_key_sum: AffinePoint = AffinePoint::identity();

        let notes_in = margin_change.notes_in.as_ref().unwrap();
        for i in 0..notes_in.len() {
            let ec_point = AffinePoint::from(&notes_in[i].address);
            pub_key_sum = &pub_key_sum + &ec_point;
        }

        let pub_key: EcPoint = EcPoint::from(&pub_key_sum);

        let valid = verify(
            &pub_key.x.to_biguint().unwrap(),
            &msg_hash,
            &margin_change.signature,
        );

        if !valid {
            return Err("Signature is invalid".to_string());
        }
    } else {
        let valid = verify(
            &margin_change.position.position_header.position_address,
            &msg_hash,
            &margin_change.signature,
        );

        if !valid {
            return Err("Signature is invalid".to_string());
        }
    }

    Ok(())
}

fn hash_margin_change_message(margin_change: &ChangeMarginMessage) -> BigUint {
    //

    if margin_change.margin_change >= 0 {
        let mut hash_inputs: Vec<&BigUint> = margin_change
            .notes_in
            .as_ref()
            .unwrap()
            .iter()
            .map(|note| &note.hash)
            .collect::<Vec<&BigUint>>();

        let z = BigUint::zero();
        let refund_hash = if margin_change.refund_note.is_some() {
            &margin_change.refund_note.as_ref().unwrap().hash
        } else {
            &z
        };
        hash_inputs.push(refund_hash);

        hash_inputs.push(&margin_change.position.hash);

        let hash = hash_many(&hash_inputs);

        return hash;
    } else {
        let mut hash_inputs = vec![];

        let p = BigUint::from_str(
            "3618502788666131213697322783095070105623107215331596699973092056135872020481",
        )
        .unwrap();

        let margin_change_amount =
            p - BigUint::from_u64(margin_change.margin_change.abs() as u64).unwrap();
        hash_inputs.push(&margin_change_amount);

        let fields_hash = &margin_change.close_order_fields.as_ref().unwrap().hash();
        hash_inputs.push(fields_hash);

        hash_inputs.push(&margin_change.position.hash);

        let hash = hash_many(&hash_inputs);

        return hash;
    }
}

pub fn store_output_json(
    transaction_output_json_: &Arc<Mutex<TxOutputJson>>,
    main_storage_: &Arc<Mutex<MainStorage>>,
) {
    let mut transaction_output_json = transaction_output_json_.lock();

    if !transaction_output_json.tx_micro_batch.is_empty() {
        let mut main_storage = main_storage_.lock();

        main_storage.store_micro_batch(&transaction_output_json.tx_micro_batch);
        main_storage.store_state_updates(&transaction_output_json.state_updates);

        transaction_output_json.tx_micro_batch.clear();
        drop(transaction_output_json);
        drop(main_storage);
    } else {
        drop(transaction_output_json);
    }

    return;
}

// * ===========================================================================================================================0
// * HANDLE GRPC_TX RESPONSE

pub async fn handle_split_notes_repsonse(
    zero_idxs: Result<Vec<u64>, String>,
    transaction_output_json: &Arc<Mutex<TxOutputJson>>,
    main_storage: &Arc<Mutex<MainStorage>>,
) -> Result<Response<SplitNotesRes>, Status> {
    match zero_idxs {
        Ok(zero_idxs) => {
            store_output_json(transaction_output_json, main_storage);

            let reply = SplitNotesRes {
                successful: true,
                error_message: "".to_string(),
                zero_idxs,
            };

            return Ok(Response::new(reply));
        }
        Err(e) => {
            return send_split_notes_error_reply(e.to_string());
        }
    }
}

// & MARGIN CHANGE  ——————————————————————————————————————————————————————————-
pub async fn handle_margin_change_repsonse(
    margin_change_response: (u64, crate::perpetual::perp_position::PerpPosition),
    user_id: u64,
    transaction_output_json: &Arc<Mutex<TxOutputJson>>,
    main_storage: &Arc<Mutex<MainStorage>>,
    perp_order_books: &HashMap<u16, Arc<TokioMutex<OrderBook>>>,
    ws_connections: &Arc<TokioMutex<WsConnectionsMap>>,
) -> Result<Response<MarginChangeRes>, Status> {
    let reply: MarginChangeRes;

    let position = margin_change_response.1;

    let market_id = PERP_MARKET_IDS
        .get(&position.position_header.synthetic_token.to_string())
        .unwrap();
    let mut perp_book = perp_order_books.get(market_id).unwrap().lock().await;
    perp_book.update_order_positions(user_id, &Some(position.clone()));
    drop(perp_book);

    store_output_json(&transaction_output_json, &main_storage);

    // TODO: Is this necessary (sending all positions to the relay server)?
    let pos = Some((
        position.position_header.position_address.to_string(),
        position.index,
        position.position_header.synthetic_token,
        position.order_side == OrderSide::Long,
        position.liquidation_price,
    ));
    let msg = json!({
        "message_id": "NEW_POSITIONS",
        "position1":  pos,
        "position2":  null
    });
    let msg = Message::Text(msg.to_string());

    if let Err(_) = send_to_relay_server(ws_connections, msg).await {
        println!("Error sending perp swap fill update message")
    };

    reply = MarginChangeRes {
        successful: true,
        error_message: "".to_string(),
        return_collateral_index: margin_change_response.0,
    };

    return Ok(Response::new(reply));
}

// & WITHDRAWALS ——————————————————————————————————————————————————————————-
pub async fn handle_withdrawal_repsonse(
    withdrawal_response: Result<
        (Option<SwapResponse>, Option<Vec<u64>>),
        Report<TransactionExecutionError>,
    >,
    transaction_output_json: &Arc<Mutex<TxOutputJson>>,
    main_storage: &Arc<Mutex<MainStorage>>,
) -> Result<Response<SuccessResponse>, Status> {
    match withdrawal_response {
        Ok(_res) => {
            store_output_json(&transaction_output_json, &main_storage);

            let reply = SuccessResponse {
                successful: true,
                error_message: "".to_string(),
            };

            return Ok(Response::new(reply));
        }
        Err(err) => {
            println!("\n{:?}", err);

            // let should_rollback =
            //  self.rollback_safeguard.lock().contains_key(&thread_id);

            let error_message_response: String;
            if let TransactionExecutionError::Withdrawal(withdrawal_execution_error) =
                err.current_context()
            {
                error_message_response = withdrawal_execution_error.err_msg.clone();
            } else {
                error_message_response = err.current_context().to_string();
            }

            return send_withdrawal_error_reply(error_message_response);
        }
    }
}

// & DEPOSITS  ——————————————————————————————————————————————————————————-
pub async fn handle_deposit_repsonse(
    deposit_response: Result<
        (
            Option<crate::transactions::swap::SwapResponse>,
            Option<Vec<u64>>,
        ),
        error_stack::Report<crate::utils::errors::TransactionExecutionError>,
    >,
    transaction_output_json: &Arc<Mutex<TxOutputJson>>,
    main_storage: &Arc<Mutex<MainStorage>>,
) -> Result<Response<DepositResponse>, Status> {
    match deposit_response {
        Ok(response) => {
            store_output_json(&transaction_output_json, &main_storage);

            let reply = DepositResponse {
                successful: true,
                zero_idxs: response.1.unwrap(),
                error_message: "".to_string(),
            };

            return Ok(Response::new(reply));
        }
        Err(err) => {
            println!("\n{:?}", err);

            let error_message_response: String;
            if let TransactionExecutionError::Deposit(deposit_execution_error) =
                err.current_context()
            {
                error_message_response = deposit_execution_error.err_msg.clone();
            } else {
                error_message_response = err.current_context().to_string();
            }

            return send_deposit_error_reply(error_message_response);
        }
    }
}

// & CANCEL ORDER  ——————————————————————————————————————————————————————————-
pub fn handle_cancel_order_repsonse(
    res: &Result<Success, Failed>,
    is_perp: bool,
    order_id: u64,
    partial_fill_tracker: &Arc<Mutex<HashMap<u64, (Option<Note>, u64)>>>,
    perpetual_partial_fill_tracker: &Arc<Mutex<HashMap<u64, (Option<Note>, u64, u64)>>>,
) -> Result<Response<CancelOrderResponse>, Status> {
    match &res {
        Ok(Success::Cancelled { .. }) => {
            let pfr_note: Option<GrpcNote>;
            if is_perp {
                let mut perpetual_partial_fill_tracker_m = perpetual_partial_fill_tracker.lock();

                let pfr_info = perpetual_partial_fill_tracker_m.remove(&order_id);

                pfr_note = if pfr_info.is_some() && pfr_info.as_ref().unwrap().0.is_some() {
                    Some(GrpcNote::from(pfr_info.unwrap().0.unwrap()))
                } else {
                    None
                };
            } else {
                let mut partial_fill_tracker_m = partial_fill_tracker.lock();

                let pfr_info = partial_fill_tracker_m.remove(&(order_id));
                pfr_note = if pfr_info.is_some() && pfr_info.as_ref().unwrap().0.is_some() {
                    Some(GrpcNote::from(
                        pfr_info.as_ref().unwrap().0.as_ref().unwrap().clone(),
                    ))
                } else {
                    None
                };
            }

            let reply: CancelOrderResponse = CancelOrderResponse {
                successful: true,
                pfr_note,
                error_message: "".to_string(),
            };

            return Ok(Response::new(reply));
        }
        Err(Failed::OrderNotFound(_)) => {
            // println!("order not found: {:?}", id);

            return send_cancel_order_error_reply("Order not found".to_string());
        }
        Err(Failed::ValidationFailed(err)) => {
            return send_cancel_order_error_reply("Validation failed: ".to_string() + err);
        }
        _ => {
            return send_cancel_order_error_reply("Unknown error".to_string());
        }
    }
}
