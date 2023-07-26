use num_bigint::BigUint;
use num_traits::{FromPrimitive, Zero};
use parking_lot::Mutex;
use serde_json::{json, Map, Value};
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
        ChangeMarginMessage, GrpcTxResponse,
    },
    trees::superficial_tree::SuperficialTree,
    utils::{
        errors::{
            send_cancel_order_error_reply, send_deposit_error_reply,
            send_margin_change_error_reply, send_split_notes_error_reply,
            send_withdrawal_error_reply, TransactionExecutionError,
        },
        storage::MainStorage,
    },
};
use tokio::sync::Mutex as TokioMutex;
use tokio::task::JoinHandle as TokioJoinHandle;

use crate::utils::crypto_utils::{pedersen_on_vec, verify, EcPoint, Signature};

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
    perp_state_tree: &Arc<Mutex<SuperficialTree>>,
) -> Result<(), String> {
    if position.hash != position.hash_position() {
        return Err("Position hash not valid".to_string());
    }

    let tree = perp_state_tree.lock();

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
            &margin_change.position.position_address,
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

        let hash = pedersen_on_vec(&hash_inputs);

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

        let hash = pedersen_on_vec(&hash_inputs);

        return hash;
    }
}

pub fn store_output_json(
    swap_output_json_: &Arc<Mutex<Vec<serde_json::Map<String, Value>>>>,
    main_storage_: &Arc<Mutex<MainStorage>>,
) {
    let mut swap_output_json = swap_output_json_.lock();
    if !swap_output_json.is_empty() {
        let main_storage = main_storage_.lock();
        main_storage.store_micro_batch(&swap_output_json);
        swap_output_json.clear();
        drop(swap_output_json);
        drop(main_storage);
    } else {
        drop(swap_output_json);
    }
}

// * ===========================================================================================================================0
// * HANDLE GRPC_TX RESPONSE

pub async fn handle_split_notes_repsonse(
    handle: TokioJoinHandle<GrpcTxResponse>,
    swap_output_json: &Arc<Mutex<Vec<Map<String, Value>>>>,
    main_storage: &Arc<Mutex<MainStorage>>,
) -> Result<Response<SplitNotesRes>, Status> {
    if let Ok(grpc_res) = handle.await {
        match grpc_res.new_idxs.unwrap() {
            Ok(zero_idxs) => {
                store_output_json(swap_output_json, main_storage);

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
    } else {
        return send_split_notes_error_reply(
            "Unexpected error occured splitting notes".to_string(),
        );
    }
}

// & MARGIN CHANGE  ——————————————————————————————————————————————————————————-
pub async fn handle_margin_change_repsonse(
    handle: TokioJoinHandle<GrpcTxResponse>,
    user_id: u64,
    swap_output_json: &Arc<Mutex<Vec<Map<String, Value>>>>,
    main_storage: &Arc<Mutex<MainStorage>>,
    perp_order_books: &HashMap<u16, Arc<TokioMutex<OrderBook>>>,
    ws_connections: &Arc<TokioMutex<WsConnectionsMap>>,
) -> Result<Response<MarginChangeRes>, Status> {
    if let Ok(grpc_res) = handle.await {
        match grpc_res.margin_change_response {
            Some((margin_change_response_, err_msg)) => {
                let reply: MarginChangeRes;
                if let Some(margin_change_response) = margin_change_response_ {
                    //

                    let market_id = PERP_MARKET_IDS
                        .get(&margin_change_response.position.synthetic_token.to_string())
                        .unwrap();
                    let mut perp_book = perp_order_books.get(market_id).unwrap().lock().await;
                    perp_book.update_order_positions(
                        user_id,
                        &Some(margin_change_response.position.clone()),
                    );
                    drop(perp_book);

                    store_output_json(&swap_output_json, &main_storage);

                    let pos = Some((
                        margin_change_response.position.position_address.to_string(),
                        margin_change_response.position.index,
                        margin_change_response.position.synthetic_token,
                        margin_change_response.position.order_side == OrderSide::Long,
                        margin_change_response.position.liquidation_price,
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
                        return_collateral_index: margin_change_response.new_note_idx,
                    };
                } else {
                    reply = MarginChangeRes {
                        successful: false,
                        error_message: err_msg,
                        return_collateral_index: 0,
                    };
                }

                return Ok(Response::new(reply));
            }
            None => {
                return send_margin_change_error_reply(
                    "Unknown error in split_notes, this should have been bypassed".to_string(),
                );
            }
        }
    } else {
        return send_margin_change_error_reply(
            "Unexpected error occured updating margin".to_string(),
        );
    }
}

// & WITHDRAWALS ——————————————————————————————————————————————————————————-
pub async fn handle_withdrawal_repsonse(
    handle: TokioJoinHandle<GrpcTxResponse>,
    swap_output_json: &Arc<Mutex<Vec<Map<String, Value>>>>,
    main_storage: &Arc<Mutex<MainStorage>>,
) -> Result<Response<SuccessResponse>, Status> {
    let withdrawl_handle = handle.await.unwrap();

    let withdrawal_response = withdrawl_handle.tx_handle.unwrap().join();

    match withdrawal_response {
        Ok(res) => match res {
            Ok(_res) => {
                store_output_json(&swap_output_json, &main_storage);

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
        },
        Err(_e) => {
            return send_withdrawal_error_reply(
                "Unknown Error occured in the withdrawal execution".to_string(),
            );
        }
    }
}

// & DEPOSITS  ——————————————————————————————————————————————————————————-
pub async fn handle_deposit_repsonse(
    handle: TokioJoinHandle<GrpcTxResponse>,
    swap_output_json: &Arc<Mutex<Vec<Map<String, Value>>>>,
    main_storage: &Arc<Mutex<MainStorage>>,
) -> Result<Response<DepositResponse>, Status> {
    let deposit_handle = handle.await.unwrap();

    let deposit_response = deposit_handle.tx_handle.unwrap().join();

    match deposit_response {
        Ok(res1) => match res1 {
            Ok(response) => {
                store_output_json(&swap_output_json, &main_storage);

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
        },
        Err(_e) => {
            return send_deposit_error_reply(
                "Unknown Error occured in the deposit execution".to_string(),
            );
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
