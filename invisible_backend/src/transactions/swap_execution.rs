use firestore_db_and_auth::ServiceSession;
use parking_lot::{Mutex, MutexGuard};
use std::collections::HashMap;
use std::sync::Arc;

use num_bigint::BigUint;

use crossbeam::thread;
use error_stack::{Report, Result};

//
use super::limit_order::LimitOrder;
use super::order_execution::{execute_order, reverify_existances, update_state_after_order};
use super::transaction_helpers::db_updates::update_db_after_spot_swap;
use super::transaction_helpers::swap_helpers::{
    finalize_updates, unblock_order, TxExecutionThreadOutput,
};
use super::transaction_helpers::transaction_output::TransactionOutptut;
use crate::order_tab::OrderTab;
use crate::transaction_batch::{LeafNodeType, TxOutputJson};
use crate::trees::superficial_tree::SuperficialTree;
use crate::utils::crypto_utils::Signature;
use crate::utils::errors::{send_swap_error, SwapThreadExecutionError};
use crate::utils::notes::Note;
use crate::utils::storage::backup_storage::BackupStorage;

type ExecutionResult = (TxExecutionThreadOutput, TxExecutionThreadOutput);
pub fn execute_swap_transaction(
    tree_m: &Arc<Mutex<SuperficialTree>>,
    partial_fill_tracker_m: &Arc<Mutex<HashMap<u64, (Option<Note>, u64)>>>,
    blocked_order_ids_m: &Arc<Mutex<HashMap<u64, bool>>>,
    order_a: &LimitOrder,
    order_b: &LimitOrder,
    order_tab_a: Option<OrderTab>,
    order_tab_b: Option<OrderTab>,
    signature_a: &Signature,
    signature_b: &Signature,
    spent_amount_a: u64,
    spent_amount_b: u64,
    fee_taken_a: u64,
    fee_taken_b: u64,
) -> Result<ExecutionResult, SwapThreadExecutionError> {
    let swap_execution_handle = thread::scope(move |s| {
        let tree = tree_m.clone();
        let partial_fill_tracker = partial_fill_tracker_m.clone();
        let blocked_order_ids = blocked_order_ids_m.clone();

        let order_handle_a = s.spawn(move |_| {
            // ? Exececute order a -----------------------------------------------------

            let execution_output: TxExecutionThreadOutput;

            let (is_partially_filled, note_info_output, updated_order_tab, new_amount_filled) =
                execute_order(
                    &tree,
                    &partial_fill_tracker,
                    &blocked_order_ids,
                    order_a,
                    order_tab_a,
                    signature_a,
                    spent_amount_a,
                    spent_amount_b,
                    fee_taken_a,
                )?;

            execution_output = TxExecutionThreadOutput {
                is_partially_filled,
                note_info_output,
                updated_order_tab,
                new_amount_filled,
            };

            return Ok(execution_output);
        });

        let tree = tree_m.clone();
        let partial_fill_tracker = partial_fill_tracker_m.clone();
        let blocked_order_ids = blocked_order_ids_m.clone();

        let order_handle_b = s.spawn(move |_| {
            // ? Exececute order b -----------------------------------------------------

            let execution_output: TxExecutionThreadOutput;

            let (is_partially_filled, note_info_output, updated_order_tab, new_amount_filled) =
                execute_order(
                    &tree,
                    &partial_fill_tracker,
                    &blocked_order_ids,
                    order_b,
                    order_tab_b,
                    signature_b,
                    spent_amount_b,
                    spent_amount_a,
                    fee_taken_b,
                )?;

            execution_output = TxExecutionThreadOutput {
                is_partially_filled,
                note_info_output,
                updated_order_tab,
                new_amount_filled,
            };

            return Ok(execution_output);
        });

        // ? Get the result of thread_a execution or return an error
        let order_a_output = order_handle_a
            .join()
            .or_else(|_| {
                // ? Un unknown error occured executing order a thread
                Err(send_swap_error(
                    "Unknow Error Occured".to_string(),
                    None,
                    None,
                ))
            })?
            .or_else(|err: Report<SwapThreadExecutionError>| {
                // ? An error occured executing order a thread
                Err(err)
            })?;

        // ? Get the result of thread_b execution or return an error
        let order_b_output = order_handle_b
            .join()
            .or_else(|_| {
                // ? Un unknown error occured executing order a thread
                Err(send_swap_error(
                    "Unknow Error Occured".to_string(),
                    None,
                    None,
                ))
            })?
            .or_else(|err: Report<SwapThreadExecutionError>| {
                // ? An error occured executing order a thread
                Err(err)
            })?;

        return Ok((order_a_output, order_b_output));
    });

    let execution_result = swap_execution_handle
        .or_else(|e| {
            println!("error occured executing spot swap2 :  {:?}", e);

            unblock_order(&blocked_order_ids_m, order_a.order_id, order_b.order_id);

            Err(send_swap_error(
                "Unknow Error Occured".to_string(),
                None,
                Some(format!("error occured executing spot swap:  {:?}", e)),
            ))
        })?
        .or_else(|err: Report<SwapThreadExecutionError>| {
            println!("error occured executing spot swap1:  {:?}", err);

            unblock_order(&blocked_order_ids_m, order_a.order_id, order_b.order_id);

            Err(err)
        })?;

    return Ok(execution_result);
}

pub fn update_state_and_finalize(
    tree_m: &Arc<Mutex<SuperficialTree>>,
    transaction_output_json: &mut MutexGuard<TxOutputJson>,
    partial_fill_tracker_m: &Arc<Mutex<HashMap<u64, (Option<Note>, u64)>>>,
    updated_state_hashes_m: &Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
    blocked_order_ids_m: &Arc<Mutex<HashMap<u64, bool>>>,
    session: &Arc<Mutex<ServiceSession>>,
    backup_storage: &Arc<Mutex<BackupStorage>>,
    execution_result: &ExecutionResult,
    order_a: &LimitOrder,
    order_b: &LimitOrder,
    prev_order_tab_a: &Option<OrderTab>,
    prev_order_tab_b: &Option<OrderTab>,
) -> Result<(), SwapThreadExecutionError> {
    let update_and_finalize_handle = thread::scope(move |_s| {
        let order_a_output = &execution_result.0;
        let order_b_output = &execution_result.1;

        // * AFTER BOTH orders have been verified successfully update the state —————————————————————————————————————

        reverify_existances(
            &tree_m,
            &order_a,
            prev_order_tab_a,
            &order_a_output.note_info_output,
            &order_b,
            prev_order_tab_b,
            &order_b_output.note_info_output,
        )?;

        // ? Order a ---------------------------------------- ----------------------------------------

        update_state_after_order(
            &tree_m,
            &updated_state_hashes_m,
            transaction_output_json,
            &order_a.spot_note_info,
            &order_a_output.note_info_output,
            &order_a_output.updated_order_tab,
        );

        update_db_after_spot_swap(
            &session,
            &backup_storage,
            &order_a,
            &order_a_output.note_info_output,
            &order_a_output.updated_order_tab,
        );

        // ? update the  partial_fill_tracker map and allow other threads to continue filling the same order
        finalize_updates(
            &partial_fill_tracker_m,
            &blocked_order_ids_m,
            order_a.order_id,
            prev_order_tab_a.is_some(),
            &order_a_output,
        );

        // ? Order b ---------------------------------------- ----------------------------------------
        update_state_after_order(
            &tree_m,
            &updated_state_hashes_m,
            transaction_output_json,
            &order_b.spot_note_info,
            &order_b_output.note_info_output,
            &order_b_output.updated_order_tab,
        );

        update_db_after_spot_swap(
            &session,
            &backup_storage,
            &order_b,
            &order_b_output.note_info_output,
            &order_b_output.updated_order_tab,
        );

        // ? update the  partial_fill_tracker map and allow other threads to continue filling the same order
        finalize_updates(
            &partial_fill_tracker_m,
            &blocked_order_ids_m,
            order_b.order_id,
            prev_order_tab_b.is_some(),
            &order_b_output,
        );

        return Ok(());
    });

    update_and_finalize_handle
        .or_else(|e| {
            println!("error occured finalizing spot_swap :  {:?}", e);

            unblock_order(&blocked_order_ids_m, order_a.order_id, order_b.order_id);

            Err(send_swap_error(
                "Unknow Error Occured".to_string(),
                None,
                Some(format!("error occured finalizing spot_swap:  {:?}", e)),
            ))
        })?
        .or_else(|err: Report<SwapThreadExecutionError>| {
            println!("error occured finalizing spot_swap:  {:?}", err);

            unblock_order(&blocked_order_ids_m, order_a.order_id, order_b.order_id);

            Err(err)
        })?;

    return Ok(());
}

pub fn update_json_output(
    transaction_output_json: &mut MutexGuard<TxOutputJson>,
    swap_output: &TransactionOutptut,
    execution_output_a: &TxExecutionThreadOutput,
    execution_output_b: &TxExecutionThreadOutput,
    prev_order_tab_a: &Option<OrderTab>,
    prev_order_tab_b: &Option<OrderTab>,
) {
    // * JSON Output ========================================================================================
    // let swap_output = TransactionOutptut::new(&self);

    let mut spot_note_info_res_a = None;
    let mut spot_note_info_res_b = None;
    let mut updated_tab_hash_a = None;
    let mut updated_tab_hash_b = None;
    if execution_output_a.updated_order_tab.is_some() {
        let updated_tab = execution_output_a.updated_order_tab.as_ref().unwrap();
        updated_tab_hash_a = Some(updated_tab.hash.clone());
    } else {
        // ? non-tab order
        let note_info_output = execution_output_a.note_info_output.as_ref().unwrap();

        let mut new_pfr_idx_a: u64 = 0;
        if let Some(new_pfr_note) = note_info_output.new_partial_fill_info.as_ref() {
            new_pfr_idx_a = new_pfr_note.0.as_ref().unwrap().index;
        }

        spot_note_info_res_a = Some((
            note_info_output.prev_partial_fill_refund_note.clone(),
            note_info_output.swap_note.index,
            new_pfr_idx_a,
        ));
    }
    if execution_output_b.updated_order_tab.is_some() {
        let updated_tab = execution_output_b.updated_order_tab.as_ref().unwrap();
        updated_tab_hash_b = Some(updated_tab.hash.clone());
    } else {
        // ? non-tab order
        let note_info_output = execution_output_b.note_info_output.as_ref().unwrap();

        let mut new_pfr_idx_b: u64 = 0;
        if let Some(new_pfr_note) = note_info_output.new_partial_fill_info.as_ref() {
            new_pfr_idx_b = new_pfr_note.0.as_ref().unwrap().index;
        }

        spot_note_info_res_b = Some((
            note_info_output.prev_partial_fill_refund_note.clone(),
            note_info_output.swap_note.index,
            new_pfr_idx_b,
        ));
    }

    let json_output = swap_output.wrap_output(
        &spot_note_info_res_a,
        &spot_note_info_res_b,
        &prev_order_tab_a,
        &prev_order_tab_b,
        &updated_tab_hash_a,
        &updated_tab_hash_b,
    );

    transaction_output_json.tx_micro_batch.push(json_output);
}
