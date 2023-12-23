use firestore_db_and_auth::ServiceSession;
use std::{collections::HashMap, sync::Arc};

use crossbeam::thread;

use num_bigint::BigUint;
use parking_lot::{Mutex, MutexGuard};
use serde_json::{Map, Value};

use error_stack::{Report, Result};

use crate::utils::storage::backup_storage::BackupStorage;
use crate::{
    transaction_batch::{tx_batch_structs::SwapFundingInfo, LeafNodeType},
    transactions::transaction_helpers::swap_helpers::unblock_order,
    trees::superficial_tree::SuperficialTree,
    utils::{
        errors::{send_perp_swap_error, PerpSwapExecutionError},
        notes::Note,
    },
};

use self::{
    close_order::execute_close_order,
    modify_order::execute_modify_order,
    open_order::{check_valid_collateral_token, execute_open_order, get_init_margin},
};

use super::{
    perp_helpers::{
        db_updates::update_db_after_perp_swap,
        perp_state_updates::{
            return_collateral_on_position_close, update_perpetual_state,
            update_state_after_swap_first_fill, update_state_after_swap_later_fills,
        },
        perp_swap_helpers::{
            block_until_prev_fill_finished, finalize_updates, reverify_existances,
        },
        perp_swap_outptut::{PerpSwapOutput, TxExecutionThreadOutput},
    },
    perp_order::PerpOrder,
    perp_position::PerpPosition,
    OrderSide, PositionEffectType, COLLATERAL_TOKEN,
};
use crate::utils::crypto_utils::Signature;

pub mod close_order;
pub mod modify_order;
pub mod open_order;

type ExecutionResult = (TxExecutionThreadOutput, TxExecutionThreadOutput);
pub fn execute_perp_swap_transaction(
    state_tree: &Arc<Mutex<SuperficialTree>>,
    blocked_perp_order_ids: &Arc<Mutex<HashMap<u64, bool>>>,
    perpetual_partial_fill_tracker: &Arc<Mutex<HashMap<u64, (Option<Note>, u64, u64)>>>, // (pfr_note, amount_filled, spent_margin)
    partialy_filled_positions: &Arc<Mutex<HashMap<String, (PerpPosition, u64)>>>, // (position, synthetic filled)
    index_price: u64,
    swap_funding_info: SwapFundingInfo,
    //
    order_a: &PerpOrder,
    order_b: &PerpOrder,
    signature_a: &Option<Signature>,
    signature_b: &Option<Signature>,
    spent_synthetic: u64,
    spent_collateral: u64,
    fee_taken_a: u64,
    fee_taken_b: u64,
) -> Result<ExecutionResult, PerpSwapExecutionError> {
    let perp_swap_execution_handle = thread::scope(move |s| {
        // ? ORDER A ------------------------------------------------------------------------------------------------
        let state_tree__ = state_tree.clone();
        let blocked_perp_order_ids__ = blocked_perp_order_ids.clone();
        let perpetual_partial_fill_tracker__ = perpetual_partial_fill_tracker.clone();
        let partialy_filled_positions__ = partialy_filled_positions.clone();
        let swap_funding_info__ = swap_funding_info.clone();

        let order_handle_a = s.spawn(move |_| {
            let execution_output: TxExecutionThreadOutput;

            // ? In case of sequential partial fills block threads updating the same order id untill previous thread is finsihed and fetch the previous partial fill info
            let partial_fill_info = block_until_prev_fill_finished(
                &perpetual_partial_fill_tracker__,
                &blocked_perp_order_ids__,
                order_a.order_id,
            )?;

            match order_a.position_effect_type {
                PositionEffectType::Open => {
                    // ? Check the collateral token is valid
                    check_valid_collateral_token(&order_a)?;

                    order_a.verify_order_signature(&signature_a.as_ref().unwrap(), None)?;

                    // Get the zero indexes from the tree
                    let mut state_tree = state_tree__.lock();
                    let perp_zero_idx = state_tree.first_zero_idx();
                    drop(state_tree);

                    let init_margin = get_init_margin(&order_a, spent_synthetic);

                    let (
                        //
                        prev_position,
                        position,
                        prev_pfr_note_,
                        new_pfr_info,
                        new_spent_sythetic,
                        prev_funding_idx,
                        is_fully_filled,
                    ) = execute_open_order(
                        &state_tree__,
                        &partialy_filled_positions__,
                        &order_a,
                        fee_taken_a,
                        perp_zero_idx,
                        swap_funding_info__.current_funding_idx,
                        spent_synthetic,
                        spent_collateral,
                        init_margin,
                        partial_fill_info,
                    )?;

                    execution_output = TxExecutionThreadOutput {
                        prev_pfr_note: prev_pfr_note_,
                        new_pfr_info,
                        is_fully_filled,
                        prev_position,
                        position_index: position.index,
                        position: Some(position),
                        prev_funding_idx,
                        collateral_returned: 0,
                        return_collateral_note: None,
                        synthetic_amount_filled: new_spent_sythetic,
                    }
                }
                PositionEffectType::Modify => {
                    //

                    // ? Verify the position hash is valid and exists in the state
                    let prev_position = verify_position_existence(
                        &state_tree__,
                        &partialy_filled_positions__,
                        &order_a.position,
                        order_a.order_id,
                    )?;

                    let (
                        position,
                        new_pfr_info,
                        new_spent_sythetic,
                        prev_funding_idx,
                        is_fully_filled,
                    ) = execute_modify_order(
                        &swap_funding_info__,
                        index_price,
                        fee_taken_a,
                        &partialy_filled_positions__,
                        &order_a,
                        &signature_a.as_ref().unwrap(),
                        spent_collateral,
                        spent_synthetic,
                        &prev_position,
                        partial_fill_info,
                    )?;

                    execution_output = TxExecutionThreadOutput {
                        prev_pfr_note: None,
                        new_pfr_info,
                        is_fully_filled,
                        prev_position: Some(prev_position),
                        position_index: position.index,
                        position: Some(position),
                        prev_funding_idx,
                        collateral_returned: 0,
                        return_collateral_note: None,
                        synthetic_amount_filled: new_spent_sythetic,
                    }
                }
                PositionEffectType::Close => {
                    //

                    // ? Verify the position hash is valid and exists in the state
                    let prev_position = verify_position_existence(
                        &state_tree__,
                        &partialy_filled_positions__,
                        &order_a.position,
                        order_a.order_id,
                    )?;

                    let (
                        position_index,
                        position,
                        new_pfr_info,
                        collateral_returned,
                        new_spent_sythetic,
                        prev_funding_idx,
                        is_fully_filled,
                    ) = execute_close_order(
                        &swap_funding_info__,
                        &partialy_filled_positions__,
                        &order_a,
                        &signature_a.as_ref().unwrap(),
                        fee_taken_a,
                        spent_collateral,
                        spent_synthetic,
                        &prev_position,
                        partial_fill_info,
                    )?;

                    execution_output = TxExecutionThreadOutput {
                        prev_pfr_note: None,
                        new_pfr_info,
                        is_fully_filled,
                        prev_position: Some(prev_position),
                        position,
                        position_index,
                        prev_funding_idx,
                        collateral_returned,
                        return_collateral_note: None,
                        synthetic_amount_filled: new_spent_sythetic,
                    }
                }
            }

            return Ok(execution_output);
        });

        // ? ORDER B -----------------------------------------------------------------------------------------------
        let state_tree__ = state_tree.clone();
        let blocked_perp_order_ids__ = blocked_perp_order_ids.clone();
        let perpetual_partial_fill_tracker__ = perpetual_partial_fill_tracker.clone();
        let partialy_filled_positions__ = partialy_filled_positions.clone();
        let swap_funding_info__ = swap_funding_info.clone();

        let order_handle_b = s.spawn(move |_| {
            let execution_output: TxExecutionThreadOutput;

            // ? In case of sequential partial fills block threads updating the same order id untill previous thread is finsihed and fetch the previous partial fill info
            let partial_fill_info = block_until_prev_fill_finished(
                &perpetual_partial_fill_tracker__,
                &blocked_perp_order_ids__,
                order_b.order_id,
            )?;

            match order_b.position_effect_type {
                PositionEffectType::Open => {
                    // ? Check the collateral token is valid
                    check_valid_collateral_token(&order_b)?;

                    order_b.verify_order_signature(&signature_b.as_ref().unwrap(), None)?;

                    // Get the zero indexes from the tree
                    let mut state_tree = state_tree__.lock();
                    let perp_zero_idx = state_tree.first_zero_idx();
                    drop(state_tree);

                    let init_margin = get_init_margin(&order_b, spent_synthetic);

                    let (
                        //
                        prev_position,
                        position,
                        prev_pfr_note_,
                        new_pfr_info,
                        new_spent_sythetic,
                        prev_funding_idx,
                        is_fully_filled,
                    ) = execute_open_order(
                        &state_tree__,
                        &partialy_filled_positions__,
                        &order_b,
                        fee_taken_b,
                        perp_zero_idx,
                        swap_funding_info__.current_funding_idx,
                        spent_synthetic,
                        spent_collateral,
                        init_margin,
                        partial_fill_info,
                    )?;

                    execution_output = TxExecutionThreadOutput {
                        prev_pfr_note: prev_pfr_note_,
                        new_pfr_info,
                        is_fully_filled,
                        prev_position,
                        position_index: position.index,
                        position: Some(position),
                        prev_funding_idx,
                        collateral_returned: 0,
                        return_collateral_note: None,
                        synthetic_amount_filled: new_spent_sythetic,
                    }
                }
                PositionEffectType::Modify => {
                    //

                    // ? Verify the position hash is valid and exists in the state
                    let prev_position = verify_position_existence(
                        &state_tree__,
                        &partialy_filled_positions__,
                        &order_b.position,
                        order_b.order_id,
                    )?;

                    let (
                        position,
                        new_pfr_info,
                        new_spent_sythetic,
                        prev_funding_idx,
                        is_fully_filled,
                    ) = execute_modify_order(
                        &swap_funding_info__,
                        index_price,
                        fee_taken_b,
                        &partialy_filled_positions__,
                        &order_b,
                        &signature_b.as_ref().unwrap(),
                        spent_collateral,
                        spent_synthetic,
                        &prev_position,
                        partial_fill_info,
                    )?;

                    execution_output = TxExecutionThreadOutput {
                        prev_pfr_note: None,
                        new_pfr_info,
                        is_fully_filled,
                        prev_position: Some(prev_position),
                        position_index: position.index,
                        position: Some(position),
                        prev_funding_idx,
                        collateral_returned: 0,
                        return_collateral_note: None,
                        synthetic_amount_filled: new_spent_sythetic,
                    }
                }
                PositionEffectType::Close => {
                    //

                    // ? Verify the position hash is valid and exists in the state
                    let prev_position = verify_position_existence(
                        &state_tree__,
                        &partialy_filled_positions__,
                        &order_b.position,
                        order_b.order_id,
                    )?;

                    let (
                        position_index,
                        position,
                        new_pfr_info,
                        collateral_returned,
                        new_spent_sythetic,
                        prev_funding_idx,
                        is_fully_filled,
                    ) = execute_close_order(
                        &swap_funding_info__,
                        &partialy_filled_positions__,
                        &order_b,
                        &signature_b.as_ref().unwrap(),
                        fee_taken_b,
                        spent_collateral,
                        spent_synthetic,
                        &prev_position,
                        partial_fill_info,
                    )?;

                    execution_output = TxExecutionThreadOutput {
                        prev_pfr_note: None,
                        new_pfr_info,
                        is_fully_filled,
                        prev_position: Some(prev_position),
                        position,
                        position_index,
                        prev_funding_idx,
                        collateral_returned,
                        return_collateral_note: None,
                        synthetic_amount_filled: new_spent_sythetic,
                    }
                }
            }

            return Ok(execution_output);
        });

        // ? Get the result of thread_a execution or return an error if it failed
        let execution_output_a = order_handle_a
            .join()
            .or_else(|_| {
                // ? Un unknown error occured executing order a thread
                Err(send_perp_swap_error(
                    "Unknow Error Occured".to_string(),
                    None,
                    None,
                ))
            })?
            .or_else(|err: Report<PerpSwapExecutionError>| {
                let mut blocked_perp_order_ids = blocked_perp_order_ids.lock();
                blocked_perp_order_ids.remove(&order_a.order_id);
                drop(blocked_perp_order_ids);

                // ? An error occured executing order a threads
                Err(err)
            })?;

        let execution_output_b = order_handle_b
            .join()
            .or_else(|_| {
                // ? Un unknown error occured executing order a thread
                Err(send_perp_swap_error(
                    "Unknow Error Occured".to_string(),
                    None,
                    None,
                ))
            })?
            .or_else(|err: Report<PerpSwapExecutionError>| {
                // ? An error occured executing order a thread
                Err(err)
            })?;

        return Ok((execution_output_a, execution_output_b));
    });

    let execution_result = perp_swap_execution_handle
        .or_else(|e| {
            unblock_order(&blocked_perp_order_ids, order_a.order_id, order_b.order_id);

            Err(send_perp_swap_error(
                "Unknown Error Occurred".to_string(),
                None,
                Some(format!("error occured executing perp swap:  {:?}", e)),
            ))
        })?
        .or_else(|err: Report<PerpSwapExecutionError>| {
            unblock_order(&blocked_perp_order_ids, order_a.order_id, order_b.order_id);

            Err(err)
        })?;

    return Ok(execution_result);
}

pub fn update_state_and_finalize(
    state_tree: &Arc<Mutex<SuperficialTree>>,
    updated_state_hashes: &Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
    blocked_perp_order_ids: &Arc<Mutex<HashMap<u64, bool>>>,
    perpetual_partial_fill_tracker: &Arc<Mutex<HashMap<u64, (Option<Note>, u64, u64)>>>, // (pfr_note, amount_filled, spent_margin)
    partialy_filled_positions: &Arc<Mutex<HashMap<String, (PerpPosition, u64)>>>, // (position, synthetic filled)
    session: &Arc<Mutex<ServiceSession>>,
    backup_storage: &Arc<Mutex<BackupStorage>>,
    //
    execution_result: &mut ExecutionResult,
    order_a: &PerpOrder,
    order_b: &PerpOrder,
) -> Result<(), PerpSwapExecutionError> {
    let execution_output_a = &mut execution_result.0;
    let execution_output_b = &mut execution_result.1;

    let result = thread::scope(move |s| {
        reverify_existances(
            &state_tree,
            &order_a,
            &execution_output_a.prev_pfr_note,
            &order_b,
            &execution_output_b.prev_pfr_note,
        )?;

        // ! State updates after order a
        let session__ = session.clone();
        let backup_storage__ = backup_storage.clone();
        let state_tree__ = state_tree.clone();
        let updated_state_hashes__ = updated_state_hashes.clone();
        let perpetual_partial_fill_tracker__ = perpetual_partial_fill_tracker.clone();
        let partialy_filled_positions__ = partialy_filled_positions.clone();
        let blocked_perp_order_ids__ = blocked_perp_order_ids.clone();

        let update_handle_a = s.spawn(move |_| {
            let execution_output_a_clone = execution_output_a.clone();

            if order_a.position_effect_type == PositionEffectType::Open {
                let new_pfr_note = &execution_output_a_clone.new_pfr_info.0;

                if execution_output_a_clone.prev_pfr_note.is_none() {
                    update_state_after_swap_first_fill(
                        &state_tree__,
                        &updated_state_hashes__,
                        &order_a.open_order_fields.as_ref().unwrap().notes_in,
                        &order_a.open_order_fields.as_ref().unwrap().refund_note,
                        new_pfr_note.as_ref(),
                    );
                } else {
                    update_state_after_swap_later_fills(
                        &state_tree__,
                        &updated_state_hashes__,
                        execution_output_a_clone.prev_pfr_note.unwrap(),
                        new_pfr_note.as_ref(),
                    );
                }
            } else if order_a.position_effect_type == PositionEffectType::Close {
                let mut tree = state_tree__.lock();
                let idx = tree.first_zero_idx();
                drop(tree);

                let return_collateral_note: Note = return_collateral_on_position_close(
                    &state_tree__,
                    &updated_state_hashes__,
                    idx,
                    execution_output_a.collateral_returned,
                    COLLATERAL_TOKEN,
                    &order_a
                        .close_order_fields
                        .as_ref()
                        .unwrap()
                        .dest_received_address,
                    &order_a
                        .close_order_fields
                        .as_ref()
                        .unwrap()
                        .dest_received_blinding,
                );

                execution_output_a.return_collateral_note = Some(return_collateral_note);
            }

            // ! Update perpetual state for order A
            update_perpetual_state(
                &state_tree__,
                &updated_state_hashes__,
                &order_a.position_effect_type,
                execution_output_a.position_index,
                execution_output_a.position.as_ref(),
            );

            finalize_updates(
                &order_a,
                &perpetual_partial_fill_tracker__,
                &partialy_filled_positions__,
                &blocked_perp_order_ids__,
                &execution_output_a.new_pfr_info,
                &execution_output_a.position,
                execution_output_a.synthetic_amount_filled,
                execution_output_a.is_fully_filled,
            );

            // ? Update the database
            update_db_after_perp_swap(
                &session__,
                &backup_storage__,
                &order_a,
                &execution_output_a.prev_pfr_note,
                &execution_output_a.new_pfr_info.0,
                &execution_output_a.return_collateral_note,
                &execution_output_a.position,
            );
        });

        // ! State updates after order b
        let session__ = session.clone();
        let backup_storage__ = backup_storage.clone();
        let state_tree__ = state_tree.clone();
        let updated_state_hashes__ = updated_state_hashes.clone();
        let perpetual_partial_fill_tracker__ = perpetual_partial_fill_tracker.clone();
        let partialy_filled_positions__ = partialy_filled_positions.clone();
        let blocked_perp_order_ids__ = blocked_perp_order_ids.clone();

        let update_handle_b = s.spawn(move |_| {
            let execution_output_b_clone = execution_output_b.clone();

            if order_b.position_effect_type == PositionEffectType::Open {
                let new_pfr_note = &execution_output_b_clone.new_pfr_info.0;

                if execution_output_b_clone.prev_pfr_note.is_none() {
                    update_state_after_swap_first_fill(
                        &state_tree__,
                        &updated_state_hashes__,
                        &order_b.open_order_fields.as_ref().unwrap().notes_in,
                        &order_b.open_order_fields.as_ref().unwrap().refund_note,
                        new_pfr_note.as_ref(),
                    );
                } else {
                    update_state_after_swap_later_fills(
                        &state_tree__,
                        &updated_state_hashes__,
                        execution_output_b_clone.prev_pfr_note.unwrap(),
                        new_pfr_note.as_ref(),
                    );
                }
            } else if order_b.position_effect_type == PositionEffectType::Close {
                let mut tree = state_tree__.lock();
                let idx = tree.first_zero_idx();
                drop(tree);

                let return_collateral_note_: Note = return_collateral_on_position_close(
                    &state_tree__,
                    &updated_state_hashes__,
                    idx,
                    execution_output_b.collateral_returned,
                    COLLATERAL_TOKEN,
                    &order_b
                        .close_order_fields
                        .as_ref()
                        .unwrap()
                        .dest_received_address,
                    &order_b
                        .close_order_fields
                        .as_ref()
                        .unwrap()
                        .dest_received_blinding,
                );

                execution_output_b.return_collateral_note = Some(return_collateral_note_);
            }

            // ! Update perpetual state for order B
            update_perpetual_state(
                &state_tree__,
                &updated_state_hashes__,
                &order_b.position_effect_type,
                execution_output_b.position_index,
                execution_output_b.position.as_ref(),
            );

            finalize_updates(
                &order_b,
                &perpetual_partial_fill_tracker__,
                &partialy_filled_positions__,
                &blocked_perp_order_ids__,
                &execution_output_b.new_pfr_info,
                &execution_output_b.position,
                execution_output_b.synthetic_amount_filled,
                execution_output_b.is_fully_filled,
            );

            // ? Update the database
            update_db_after_perp_swap(
                &session__,
                &backup_storage__,
                &order_b,
                &execution_output_b.prev_pfr_note,
                &execution_output_b.new_pfr_info.0,
                &execution_output_b.return_collateral_note,
                &execution_output_b.position,
            );
        });

        // ? Run the update state thread_a or return an error
        update_handle_a.join().or_else(|_| {
            // ? Un unknown error occured executing order a thread
            Err(send_perp_swap_error(
                "Unknown Error Occurred".to_string(),
                None,
                None,
            ))
        })?;

        // ? Run the update state thread_b or return an error
        update_handle_b.join().or_else(|_| {
            // ? Un unknown error occured executing order b thread
            Err(send_perp_swap_error(
                "Unknown Error Occurred".to_string(),
                None,
                None,
            ))
        })?;

        Ok(())
    });

    result
        .or_else(|e| {
            println!("error occured finalizing spot_swap :  {:?}", e);

            unblock_order(&blocked_perp_order_ids, order_a.order_id, order_b.order_id);

            Err(send_perp_swap_error(
                "Unknow Error Occured".to_string(),
                None,
                Some(format!("error occured finalizing spot_swap:  {:?}", e)),
            ))
        })?
        .or_else(|err: Report<PerpSwapExecutionError>| {
            println!("error occured finalizing spot_swap:  {:?}", err);

            unblock_order(&blocked_perp_order_ids, order_a.order_id, order_b.order_id);

            Err(err)
        })?;

    Ok(())
}

pub fn update_json_output(
    swap_output: PerpSwapOutput,
    execution_output_a: &TxExecutionThreadOutput,
    execution_output_b: &TxExecutionThreadOutput,
    swap_output_json: &mut MutexGuard<Vec<serde_json::Map<String, Value>>>,
    current_funding_idx: u32,
    order_a_side: &OrderSide,
) {
    // ? Write to json output (make sure order_a is long and order_b is short - for cairo)
    let json_output: Map<String, Value>;

    let is_first_fill_a = execution_output_a.prev_pfr_note.is_none();
    let is_first_fill_b = execution_output_b.prev_pfr_note.is_none();

    let mut new_pfr_note_hash_a = None;
    let mut new_pfr_idx_a: u64 = 0;
    let mut new_pfr_note_hash_b = None;
    let mut new_pfr_idx_b: u64 = 0;
    if let Some(new_pfr_info) = &execution_output_a.new_pfr_info.0 {
        new_pfr_idx_a = new_pfr_info.index;
        new_pfr_note_hash_a = Some(new_pfr_info.hash.to_string());
    };
    if let Some(new_pfr_info) = &execution_output_b.new_pfr_info.0 {
        new_pfr_idx_b = new_pfr_info.index;
        new_pfr_note_hash_b = Some(new_pfr_info.hash.to_string());
    };

    let mut new_position_hash_a = None;
    let mut new_position_hash_b = None;
    if let Some(position) = &execution_output_a.position {
        new_position_hash_a = Some(position.hash.to_string());
    }
    if let Some(position) = &execution_output_b.position {
        new_position_hash_b = Some(position.hash.to_string());
    }

    let mut return_collateral_hash_a = None;
    let mut return_collateral_idx_a: u64 = 0;
    let mut return_collateral_hash_b = None;
    let mut return_collateral_idx_b: u64 = 0;
    if let Some(rc_note) = &execution_output_a.return_collateral_note {
        return_collateral_idx_a = rc_note.index;
        return_collateral_hash_a = Some(rc_note.hash.to_string());
    }
    if let Some(rc_note) = &execution_output_b.return_collateral_note {
        return_collateral_hash_b = Some(rc_note.hash.to_string());
        return_collateral_idx_b = rc_note.index;
    }

    if *order_a_side == OrderSide::Long {
        json_output = swap_output.wrap_output(
            is_first_fill_a,
            is_first_fill_b,
            &execution_output_a.prev_pfr_note,
            &execution_output_b.prev_pfr_note,
            &new_pfr_note_hash_a,
            &new_pfr_note_hash_b,
            &execution_output_a.prev_position,
            &execution_output_b.prev_position,
            &new_position_hash_a,
            &new_position_hash_b,
            execution_output_a.position_index,
            execution_output_b.position_index,
            new_pfr_idx_a,
            new_pfr_idx_b,
            return_collateral_idx_a,
            return_collateral_idx_b,
            &return_collateral_hash_a,
            &return_collateral_hash_b,
            execution_output_a.prev_funding_idx,
            execution_output_b.prev_funding_idx,
            current_funding_idx,
        );
    } else {
        json_output = swap_output.wrap_output(
            is_first_fill_b,
            is_first_fill_a,
            &execution_output_b.prev_pfr_note,
            &execution_output_a.prev_pfr_note,
            &new_pfr_note_hash_b,
            &new_pfr_note_hash_a,
            &execution_output_b.prev_position,
            &execution_output_a.prev_position,
            &new_position_hash_b,
            &new_position_hash_a,
            execution_output_b.position_index,
            execution_output_a.position_index,
            new_pfr_idx_b,
            new_pfr_idx_a,
            return_collateral_idx_b,
            return_collateral_idx_a,
            &return_collateral_hash_b,
            &return_collateral_hash_a,
            execution_output_b.prev_funding_idx,
            execution_output_a.prev_funding_idx,
            current_funding_idx,
        );
    }

    swap_output_json.push(json_output);
}

// * Helpers ===========================================================================================
pub fn verify_position_existence(
    perpetual_state_tree__: &Arc<Mutex<SuperficialTree>>,
    partially_filled_positions: &Arc<Mutex<HashMap<String, (PerpPosition, u64)>>>,
    position: &Option<PerpPosition>,
    order_id: u64,
) -> Result<PerpPosition, PerpSwapExecutionError> {
    let perpetual_state_tree = perpetual_state_tree__.lock();

    let partially_filled_positions_m = partially_filled_positions.lock();
    if let Some((pos_, _)) = partially_filled_positions_m.get(
        &position
            .as_ref()
            .unwrap()
            .position_header
            .position_address
            .to_string(),
    ) {
        // ? Verify the position hash is valid and exists in the state
        if pos_.hash != pos_.hash_position()
            || perpetual_state_tree.get_leaf_by_index(pos_.index as u64) != pos_.hash
        {
            let pos = position.as_ref().unwrap();

            verify_existance(&perpetual_state_tree, &pos, order_id)?;

            return Ok(pos.clone());
        } else {
            return Ok(pos_.clone());
        }
    } else {
        let pos = position.as_ref().unwrap();

        verify_existance(&perpetual_state_tree, &pos, order_id)?;

        return Ok(pos.clone());
    }
}

fn verify_existance(
    state_tree: &SuperficialTree,
    position: &PerpPosition,
    order_id: u64,
) -> Result<(), PerpSwapExecutionError> {
    // ? Verify the position hash is valid and exists in the state
    if position.hash != position.hash_position() {
        return Err(send_perp_swap_error(
            "position hash not valid".to_string(),
            Some(order_id),
            None,
        ));
    }

    // ? Check that the position being updated exists in the state
    if state_tree.get_leaf_by_index(position.index as u64) != position.hash {
        return Err(send_perp_swap_error(
            "position does not exist in the state".to_string(),
            Some(order_id),
            None,
        ));
    }
    return Ok(());
}
