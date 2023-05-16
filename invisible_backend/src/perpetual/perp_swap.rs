use firestore_db_and_auth::ServiceSession;
use num_bigint::BigUint;
use parking_lot::Mutex;
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::sync::Arc;
use std::thread::ThreadId;

use crossbeam::thread;

use super::order_execution::close_order::execute_close_order;
use super::order_execution::modify_order::{execute_modify_order, verify_position_existence};
use super::order_execution::open_order::{
    check_valid_collateral_token, execute_open_order, get_init_margin,
};
use super::perp_helpers::db_updates::update_db_after_perp_swap;
use super::perp_helpers::perp_rollback::{
    save_close_order_rollback_info, save_open_order_rollback_info, PerpRollbackInfo,
};
use super::perp_helpers::perp_state_updates::{
    return_collateral_on_position_close, update_perpetual_state,
    update_state_after_swap_first_fill, update_state_after_swap_later_fills,
};
//
use super::perp_helpers::perp_swap_helpers::{consistency_checks, finalize_updates};
use super::perp_helpers::perp_swap_outptut::{
    PerpSwapOutput, PerpSwapResponse, TxExecutionThreadOutput,
};
use super::PositionEffectType;
use super::{perp_order::PerpOrder, perp_position::PerpPosition, OrderSide};
use crate::transaction_batch::tx_batch_structs::SwapFundingInfo;
use crate::transactions::transaction_helpers::swap_helpers::unblock_order;
use crate::trees::superficial_tree::SuperficialTree;
use crate::utils::crypto_utils::Signature;
use crate::utils::storage::BackupStorage;
use crate::utils::{
    errors::{send_perp_swap_error, PerpSwapExecutionError},
    notes::Note,
};

use error_stack::{Report, Result};
//

// TODO: DO SOMETHING WITH LEFTOVER MARGIN IN 000 SITUATIONS

#[derive(Clone, Debug)]
pub struct PerpSwap {
    pub transaction_type: String,
    pub order_a: PerpOrder, // Should be a Long order
    pub order_b: PerpOrder, // Should be a Short order
    pub signature_a: Option<Signature>,
    pub signature_b: Option<Signature>,
    pub spent_collateral: u64, // amount spent in collateral token
    pub spent_synthetic: u64,  // amount spent in synthetic token
    pub fee_taken_a: u64,      // Fee taken in collateral token
    pub fee_taken_b: u64,      // Fee taken in collateral token
}

impl PerpSwap {
    pub fn new(
        order_a: PerpOrder,
        order_b: PerpOrder,
        signature_a: Option<Signature>,
        signature_b: Option<Signature>,
        spent_collateral: u64,
        spent_synthetic: u64,
        fee_taken_a: u64,
        fee_taken_b: u64,
    ) -> PerpSwap {
        PerpSwap {
            transaction_type: String::from("perpetual_swap"),
            order_a,
            order_b,
            signature_a,
            signature_b,
            spent_collateral,
            spent_synthetic,
            fee_taken_a,
            fee_taken_b,
        }
    }

    // & order a should be a Long order, & order b should be a Short order
    // & order a (Long) is swapping collateral for synthetic tokens
    // & order b (Short) is swapping synthetic tokens for collateral
    pub fn execute(
        &self,
        state_tree: Arc<Mutex<SuperficialTree>>,
        updated_note_hashes: Arc<Mutex<HashMap<u64, BigUint>>>,
        swap_output_json: Arc<Mutex<Vec<serde_json::Map<String, Value>>>>,
        blocked_perp_order_ids: Arc<Mutex<HashMap<u64, bool>>>,
        //
        perpetual_state_tree: Arc<Mutex<SuperficialTree>>,
        perpetual_partial_fill_tracker: Arc<Mutex<HashMap<u64, (Option<Note>, u64, u64)>>>, // (pfr_note, amount_filled, spent_margin)
        partialy_filled_positions: Arc<Mutex<HashMap<String, (PerpPosition, u64)>>>, // (position, synthetic filled)
        perpetual_updated_position_hashes: Arc<Mutex<HashMap<u64, BigUint>>>,
        //
        index_price: u64,
        min_funding_idxs: Arc<Mutex<HashMap<u64, u32>>>,
        swap_funding_info: SwapFundingInfo,
        //
        perp_rollback_safeguard: Arc<Mutex<HashMap<ThreadId, PerpRollbackInfo>>>,
        session: Arc<Mutex<ServiceSession>>,
        backup_storage: Arc<Mutex<BackupStorage>>,
    ) -> Result<PerpSwapResponse, PerpSwapExecutionError> {
        //

        consistency_checks(
            &self.order_a,
            &self.order_b,
            self.spent_collateral,
            self.spent_synthetic,
            self.fee_taken_a,
            self.fee_taken_b,
        )?;

        // ? Execute orders in parallel ===========================================================

        let thread_id: ThreadId = std::thread::current().id();
        let current_funding_idx = swap_funding_info.current_funding_idx;

        let perpetual_state_tree_c = perpetual_state_tree.clone();
        let blocked_perp_order_ids_c = blocked_perp_order_ids.clone();

        let (execution_output_a, execution_output_b) = thread::scope(move |s| {
            // ? ORDER A ------------------------------------------------------------------------------------------------
            let state_tree__ = state_tree.clone();
            let perpetual_state_tree__ = perpetual_state_tree_c.clone();
            let blocked_perp_order_ids__ = blocked_perp_order_ids.clone();
            let perpetual_partial_fill_tracker__ = perpetual_partial_fill_tracker.clone();
            let partialy_filled_positions__ = partialy_filled_positions.clone();
            let swap_funding_info__ = swap_funding_info.clone();

            let order_handle_a = s.spawn(move |_| {
                let execution_output: TxExecutionThreadOutput;

                match self.order_a.position_effect_type {
                    PositionEffectType::Open => {
                        // ? Check the collateral token is valid
                        check_valid_collateral_token(&self.order_a)?;

                        let pub_key_sum = self
                            .order_a
                            .verify_order_signature(&self.signature_a.as_ref().unwrap(), None)?
                            .unwrap();

                        // Get the zero indexes from the tree
                        let mut per_state_tree = perpetual_state_tree__.lock();
                        let perp_zero_idx = per_state_tree.first_zero_idx();
                        drop(per_state_tree);

                        let init_margin = get_init_margin(&self.order_a, self.spent_synthetic);

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
                            &perpetual_partial_fill_tracker__,
                            &partialy_filled_positions__,
                            &blocked_perp_order_ids__,
                            &self.order_a,
                            pub_key_sum,
                            self.fee_taken_a,
                            perp_zero_idx,
                            swap_funding_info__.current_funding_idx,
                            self.spent_synthetic,
                            self.spent_collateral,
                            init_margin,
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
                        verify_position_existence(
                            &perpetual_state_tree__,
                            &partialy_filled_positions__,
                            &self.order_a.position,
                            self.order_a.order_id,
                        )?;

                        let (
                            prev_position,
                            position,
                            new_pfr_info,
                            new_spent_sythetic,
                            prev_funding_idx,
                            is_fully_filled,
                        ) = execute_modify_order(
                            &swap_funding_info__,
                            index_price,
                            self.fee_taken_a,
                            &partialy_filled_positions__,
                            &perpetual_partial_fill_tracker__,
                            &blocked_perp_order_ids__,
                            &self.order_a,
                            &self.signature_a.as_ref().unwrap(),
                            self.spent_collateral,
                            self.spent_synthetic,
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
                        verify_position_existence(
                            &perpetual_state_tree__,
                            &partialy_filled_positions__,
                            &self.order_a.position,
                            self.order_a.order_id,
                        )?;

                        let (
                            position_index,
                            prev_position,
                            position,
                            new_pfr_info,
                            collateral_returned,
                            new_spent_sythetic,
                            prev_funding_idx,
                            is_fully_filled,
                        ) = execute_close_order(
                            &swap_funding_info__,
                            &partialy_filled_positions__,
                            &perpetual_partial_fill_tracker__,
                            &blocked_perp_order_ids__,
                            &self.order_a,
                            &self.signature_a.as_ref().unwrap(),
                            self.fee_taken_a,
                            self.spent_collateral,
                            self.spent_synthetic,
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
            let perpetual_state_tree__ = perpetual_state_tree_c.clone();
            let perpetual_partial_fill_tracker__ = perpetual_partial_fill_tracker.clone();
            let partialy_filled_positions__ = partialy_filled_positions.clone();
            let swap_funding_info__ = swap_funding_info.clone();

            let order_handle_b = s.spawn(move |_| {
                let execution_output: TxExecutionThreadOutput;

                match self.order_b.position_effect_type {
                    PositionEffectType::Open => {
                        // ? Check the collateral token is valid
                        check_valid_collateral_token(&self.order_b)?;

                        let pub_key_sum = self
                            .order_b
                            .verify_order_signature(&self.signature_b.as_ref().unwrap(), None)?
                            .unwrap();

                        // Get the zero indexes from the tree
                        let mut per_state_tree = perpetual_state_tree__.lock();
                        let perp_zero_idx = per_state_tree.first_zero_idx();
                        drop(per_state_tree);

                        let init_margin = get_init_margin(&self.order_b, self.spent_synthetic);

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
                            &perpetual_partial_fill_tracker__,
                            &partialy_filled_positions__,
                            &blocked_perp_order_ids__,
                            &self.order_b,
                            pub_key_sum,
                            self.fee_taken_b,
                            perp_zero_idx,
                            swap_funding_info__.current_funding_idx,
                            self.spent_synthetic,
                            self.spent_collateral,
                            init_margin,
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
                        verify_position_existence(
                            &perpetual_state_tree__,
                            &partialy_filled_positions__,
                            &self.order_b.position,
                            self.order_b.order_id,
                        )?;

                        let (
                            prev_position,
                            position,
                            new_pfr_info,
                            new_spent_sythetic,
                            prev_funding_idx,
                            is_fully_filled,
                        ) = execute_modify_order(
                            &swap_funding_info__,
                            index_price,
                            self.fee_taken_b,
                            &partialy_filled_positions__,
                            &perpetual_partial_fill_tracker__,
                            &blocked_perp_order_ids__,
                            &self.order_b,
                            &self.signature_b.as_ref().unwrap(),
                            self.spent_collateral,
                            self.spent_synthetic,
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
                        verify_position_existence(
                            &perpetual_state_tree__,
                            &partialy_filled_positions__,
                            &self.order_b.position,
                            self.order_b.order_id,
                        )?;

                        let (
                            position_index,
                            prev_position,
                            position,
                            new_pfr_info,
                            collateral_returned,
                            new_spent_sythetic,
                            prev_funding_idx,
                            is_fully_filled,
                        ) = execute_close_order(
                            &swap_funding_info__,
                            &partialy_filled_positions__,
                            &perpetual_partial_fill_tracker__,
                            &blocked_perp_order_ids__,
                            &self.order_b,
                            &self.signature_b.as_ref().unwrap(),
                            self.fee_taken_b,
                            self.spent_collateral,
                            self.spent_synthetic,
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
            let mut execution_output_a = order_handle_a
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
                    blocked_perp_order_ids.remove(&self.order_a.order_id);
                    drop(blocked_perp_order_ids);

                    // ? An error occured executing order a threads
                    Err(err)
                })?;

            let mut execution_output_b = order_handle_b
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

            // * UPDATE STATE AFTER SWAP ——————————————————————————————————————————----------------------------------------------

            // ? After verification and execution of both orders, we can now update the state trees

            // ! State updates after order a
            let state_tree__ = state_tree.clone();
            let updated_note_hashes__ = updated_note_hashes.clone();
            let perpetual_state_tree__ = perpetual_state_tree.clone();
            let perpetual_updated_position_hashes__ = perpetual_updated_position_hashes.clone();
            let perpetual_partial_fill_tracker__ = perpetual_partial_fill_tracker.clone();
            let partialy_filled_positions__ = partialy_filled_positions.clone();
            let blocked_perp_order_ids__ = blocked_perp_order_ids.clone();
            let perp_rollback_safeguard__ = perp_rollback_safeguard.clone();

            let update_handle_a = s.spawn(move |_| {
                let execution_output_a_clone = execution_output_a.clone();

                if self.order_a.position_effect_type == PositionEffectType::Open {
                    let new_pfr_note = &execution_output_a_clone.new_pfr_info.0;

                    //  ===============================
                    save_open_order_rollback_info(
                        true,
                        &perp_rollback_safeguard__,
                        thread_id,
                        self.order_a.order_id,
                        new_pfr_note,
                        &execution_output_a_clone.prev_pfr_note,
                        execution_output_a.position_index,
                        &execution_output_a.prev_position,
                    );
                    //  ===============================

                    if execution_output_a_clone.prev_pfr_note.is_none() {
                        update_state_after_swap_first_fill(
                            &state_tree__,
                            &updated_note_hashes__,
                            &self.order_a.open_order_fields.as_ref().unwrap().notes_in,
                            &self.order_a.open_order_fields.as_ref().unwrap().refund_note,
                            new_pfr_note.as_ref(),
                            self.order_a.order_id,
                        )?;
                    } else {
                        update_state_after_swap_later_fills(
                            &state_tree__,
                            &updated_note_hashes__,
                            execution_output_a_clone.prev_pfr_note.unwrap(),
                            new_pfr_note.as_ref(),
                        )?;
                    }
                } else if self.order_a.position_effect_type == PositionEffectType::Close {
                    let mut tree = state_tree__.lock();
                    let idx = tree.first_zero_idx();
                    drop(tree);

                    //  =====================================
                    save_close_order_rollback_info(
                        true,
                        &perp_rollback_safeguard__,
                        thread_id,
                        idx,
                        &execution_output_a.prev_position,
                    );
                    //  ====================================

                    let return_collateral_note: Note = return_collateral_on_position_close(
                        &state_tree__,
                        &updated_note_hashes__,
                        idx,
                        execution_output_a.collateral_returned,
                        execution_output_a
                            .prev_position
                            .as_ref()
                            .unwrap()
                            .collateral_token,
                        &self
                            .order_a
                            .close_order_fields
                            .as_ref()
                            .unwrap()
                            .dest_received_address,
                        &self
                            .order_a
                            .close_order_fields
                            .as_ref()
                            .unwrap()
                            .dest_received_blinding,
                    )?;

                    execution_output_a.return_collateral_note = Some(return_collateral_note);
                }

                // ! Update perpetual state for order A
                update_perpetual_state(
                    &perpetual_state_tree__,
                    &perpetual_updated_position_hashes__,
                    &self.order_a.position_effect_type,
                    execution_output_a.position_index,
                    execution_output_a.position.as_ref(),
                )?;

                finalize_updates(
                    &self.order_a,
                    &perpetual_partial_fill_tracker__,
                    &partialy_filled_positions__,
                    &blocked_perp_order_ids__,
                    &execution_output_a.new_pfr_info,
                    &execution_output_a.position,
                    execution_output_a.synthetic_amount_filled,
                    execution_output_a.is_fully_filled,
                );

                Ok(execution_output_a)
            });

            // ! State updates after order b
            let state_tree__ = state_tree.clone();
            let updated_note_hashes__ = updated_note_hashes.clone();
            let perpetual_state_tree__ = perpetual_state_tree.clone();
            let perpetual_updated_position_hashes__ = perpetual_updated_position_hashes.clone();
            let perpetual_partial_fill_tracker__ = perpetual_partial_fill_tracker.clone();
            let partialy_filled_positions__ = partialy_filled_positions.clone();
            let blocked_perp_order_ids__ = blocked_perp_order_ids.clone();
            let perp_rollback_safeguard__ = perp_rollback_safeguard.clone();

            let update_handle_b = s.spawn(move |_| {
                let execution_output_b_clone = execution_output_b.clone();

                if self.order_b.position_effect_type == PositionEffectType::Open {
                    let new_pfr_note = &execution_output_b_clone.new_pfr_info.0;

                    //  ===============================
                    save_open_order_rollback_info(
                        false,
                        &perp_rollback_safeguard__,
                        thread_id,
                        self.order_b.order_id,
                        new_pfr_note,
                        &execution_output_b_clone.prev_pfr_note,
                        execution_output_b.position_index,
                        &execution_output_b.prev_position,
                    );
                    //  ===============================

                    if execution_output_b_clone.prev_pfr_note.is_none() {
                        update_state_after_swap_first_fill(
                            &state_tree__,
                            &updated_note_hashes__,
                            &self.order_b.open_order_fields.as_ref().unwrap().notes_in,
                            &self.order_b.open_order_fields.as_ref().unwrap().refund_note,
                            new_pfr_note.as_ref(),
                            self.order_b.order_id,
                        )?;
                    } else {
                        update_state_after_swap_later_fills(
                            &state_tree__,
                            &updated_note_hashes__,
                            execution_output_b_clone.prev_pfr_note.unwrap(),
                            new_pfr_note.as_ref(),
                        )?;
                    }
                } else if self.order_b.position_effect_type == PositionEffectType::Close {
                    let mut tree = state_tree__.lock();
                    let idx = tree.first_zero_idx();
                    drop(tree);

                    //  =====================================
                    save_close_order_rollback_info(
                        false,
                        &perp_rollback_safeguard__,
                        thread_id,
                        idx,
                        &execution_output_b.prev_position,
                    );
                    //  ====================================

                    let return_collateral_note_: Note = return_collateral_on_position_close(
                        &state_tree__,
                        &updated_note_hashes__,
                        idx,
                        execution_output_b.collateral_returned,
                        execution_output_b
                            .prev_position
                            .as_ref()
                            .unwrap()
                            .collateral_token,
                        &self
                            .order_b
                            .close_order_fields
                            .as_ref()
                            .unwrap()
                            .dest_received_address,
                        &self
                            .order_b
                            .close_order_fields
                            .as_ref()
                            .unwrap()
                            .dest_received_blinding,
                    )?;

                    execution_output_b.return_collateral_note = Some(return_collateral_note_);
                }

                // ! Update perpetual state for order B
                update_perpetual_state(
                    &perpetual_state_tree__,
                    &perpetual_updated_position_hashes__,
                    &self.order_b.position_effect_type,
                    execution_output_b.position_index,
                    execution_output_b.position.as_ref(),
                )?;

                finalize_updates(
                    &self.order_b,
                    &perpetual_partial_fill_tracker__,
                    &partialy_filled_positions__,
                    &blocked_perp_order_ids__,
                    &execution_output_b.new_pfr_info,
                    &execution_output_b.position,
                    execution_output_b.synthetic_amount_filled,
                    execution_output_b.is_fully_filled,
                );

                Ok(execution_output_b)
            });

            // ? Run the update state thread_a or return an error
            let execution_output_a = update_handle_a
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

            // ? Run the update state thread_b or return an error
            let execution_output_b = update_handle_b
                .join()
                .or_else(|_| {
                    // ? Un unknown error occured executing order b thread
                    Err(send_perp_swap_error(
                        "Unknow Error Occured".to_string(),
                        None,
                        None,
                    ))
                })?
                .or_else(|err: Report<PerpSwapExecutionError>| {
                    // ? An error occured executing order b thread
                    Err(err)
                })?;

            return Ok((execution_output_a, execution_output_b));
        })
        .or_else(|e| {
            unblock_order(
                &blocked_perp_order_ids_c,
                self.order_a.order_id,
                self.order_b.order_id,
            );

            Err(send_perp_swap_error(
                "Unknow Error Occured".to_string(),
                None,
                Some(format!("error occured executing perp swap:  {:?}", e)),
            ))
        })?
        .or_else(|err: Report<PerpSwapExecutionError>| {
            unblock_order(
                &blocked_perp_order_ids_c,
                self.order_a.order_id,
                self.order_b.order_id,
            );

            Err(err)
        })?;

        //  set new min funding index if necessary (for cairo input ) -------------------------
        let mut min_funding_idxs_m = min_funding_idxs.lock();
        let prev_min_funding_idx = min_funding_idxs_m
            .get(&self.order_a.synthetic_token)
            .unwrap();
        if std::cmp::min(
            execution_output_a.prev_funding_idx,
            execution_output_b.prev_funding_idx,
        ) < *prev_min_funding_idx
        {
            min_funding_idxs_m.insert(
                self.order_a.synthetic_token,
                std::cmp::min(
                    execution_output_a.prev_funding_idx,
                    execution_output_b.prev_funding_idx,
                ),
            );
        }
        drop(min_funding_idxs_m);

        // * Write the swap output to json to be used as input to the cairo program ——————————————

        let return_collateral_idx_a: u64 = if execution_output_a.return_collateral_note.is_some() {
            execution_output_a
                .return_collateral_note
                .as_ref()
                .unwrap()
                .index
        } else {
            0
        };
        let return_collateral_idx_b: u64 = if execution_output_b.return_collateral_note.is_some() {
            execution_output_b
                .return_collateral_note
                .as_ref()
                .unwrap()
                .index
        } else {
            0
        };
        let new_pfr_idx_a: u64 = if execution_output_a.new_pfr_info.0.is_some() {
            execution_output_a.new_pfr_info.0.as_ref().unwrap().index
        } else {
            0
        };

        let new_pfr_idx_b: u64 = if execution_output_b.new_pfr_info.0.is_some() {
            execution_output_b.new_pfr_info.0.as_ref().unwrap().index
        } else {
            0
        };

        // ? Write to json output (make sure order_a is long and order_b is short - for cairo)
        let json_output: Map<String, Value>;

        let is_first_fill_a = execution_output_a.prev_pfr_note.is_none();
        let is_first_fill_b = execution_output_b.prev_pfr_note.is_none();

        let new_pfr_note_hash_a = if execution_output_a.new_pfr_info.0.is_some() {
            Some(
                execution_output_a
                    .new_pfr_info
                    .0
                    .as_ref()
                    .unwrap()
                    .hash
                    .to_string(),
            )
        } else {
            None
        };

        let new_pfr_note_hash_b = if execution_output_b.new_pfr_info.0.is_some() {
            Some(
                execution_output_b
                    .new_pfr_info
                    .0
                    .as_ref()
                    .unwrap()
                    .hash
                    .to_string(),
            )
        } else {
            None
        };

        let new_position_hash_a = if execution_output_a.position.is_some() {
            Some(
                execution_output_a
                    .position
                    .as_ref()
                    .unwrap()
                    .hash
                    .to_string(),
            )
        } else {
            None
        };
        let new_position_hash_b = if execution_output_b.position.is_some() {
            Some(
                execution_output_b
                    .position
                    .as_ref()
                    .unwrap()
                    .hash
                    .to_string(),
            )
        } else {
            None
        };

        let return_collateral_hash_a = if execution_output_a.return_collateral_note.is_some() {
            Some(
                execution_output_a
                    .return_collateral_note
                    .as_ref()
                    .unwrap()
                    .hash
                    .to_string(),
            )
        } else {
            None
        };
        let return_collateral_hash_b = if execution_output_b.return_collateral_note.is_some() {
            Some(
                execution_output_b
                    .return_collateral_note
                    .as_ref()
                    .unwrap()
                    .hash
                    .to_string(),
            )
        } else {
            None
        };

        if self.order_a.order_side == OrderSide::Long {
            let swap_output = PerpSwapOutput::new(&self, &self.order_a, &self.order_b);

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
            let swap_output = PerpSwapOutput::new(&self, &self.order_b, &self.order_a);

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

        let mut swap_output_json_m = swap_output_json.lock();
        swap_output_json_m.push(json_output);
        drop(swap_output_json_m);

        // ? Update the database
        update_db_after_perp_swap(
            &session,
            backup_storage,
            &self.order_a,
            &self.order_b,
            &execution_output_a.prev_pfr_note,
            &execution_output_b.prev_pfr_note,
            &execution_output_a.new_pfr_info.0,
            &execution_output_b.new_pfr_info.0,
            &execution_output_a.return_collateral_note,
            &execution_output_b.return_collateral_note,
            &execution_output_a.position,
            &execution_output_b.position,
        );

        return Ok(PerpSwapResponse {
            position_a: execution_output_a.position,
            position_b: execution_output_b.position,
            new_pfr_info_a: execution_output_a.new_pfr_info,
            new_pfr_info_b: execution_output_b.new_pfr_info,
            return_collateral_note_a: execution_output_a.return_collateral_note,
            return_collateral_note_b: execution_output_b.return_collateral_note,
        });
    }

    //
}

use serde::ser::{Serialize, SerializeStruct, Serializer};

impl Serialize for PerpSwap {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut perp_swap = serializer.serialize_struct("PerpSwap", 9)?;

        if self.order_a.order_side == OrderSide::Long {
            perp_swap.serialize_field("signature_a", &self.signature_a)?;
            perp_swap.serialize_field("signature_b", &self.signature_b)?;
            perp_swap.serialize_field("spent_collateral", &self.spent_collateral)?;
            perp_swap.serialize_field("spent_synthetic", &self.spent_synthetic)?;
            perp_swap.serialize_field("fee_taken_a", &self.fee_taken_a)?;
            perp_swap.serialize_field("fee_taken_b", &self.fee_taken_b)?;
        } else {
            perp_swap.serialize_field("signature_b", &self.signature_a)?;
            perp_swap.serialize_field("signature_a", &self.signature_b)?;
            perp_swap.serialize_field("spent_collateral", &self.spent_collateral)?;
            perp_swap.serialize_field("spent_synthetic", &self.spent_synthetic)?;
            perp_swap.serialize_field("fee_taken_b", &self.fee_taken_a)?;
            perp_swap.serialize_field("fee_taken_a", &self.fee_taken_b)?;
        }

        return perp_swap.end();
    }
}
