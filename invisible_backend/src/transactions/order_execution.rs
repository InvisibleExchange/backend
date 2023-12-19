use std::{collections::HashMap, sync::Arc};

use error_stack::Result;
use num_bigint::BigUint;
use parking_lot::Mutex;

use crate::{
    order_tab::OrderTab,
    transaction_batch::LeafNodeType,
    trees::superficial_tree::SuperficialTree,
    utils::{
        errors::{send_swap_error, SwapThreadExecutionError},
        notes::Note,
    },
};

use crate::utils::crypto_utils::Signature;

use super::{
    limit_order::{LimitOrder, SpotNotesInfo},
    transaction_helpers::{
        helpers::{
            non_tab_orders::{
                check_non_tab_order_validity, execute_non_tab_order_modifications,
                update_state_after_non_tab_order,
            },
            tab_orders::{
                check_tab_order_validity, execute_tab_order_modifications,
                update_state_after_tab_order,
            },
        },
        swap_helpers::{block_until_prev_fill_finished, NoteInfoExecutionOutput},
    },
};

// * UPDATE STATE FUNCTION * ========================================================
pub fn execute_order(
    tree_m: &Arc<Mutex<SuperficialTree>>,
    partial_fill_tracker_m: &Arc<Mutex<HashMap<u64, (Option<Note>, u64)>>>,
    blocked_order_ids_m: &Arc<Mutex<HashMap<u64, bool>>>,
    order: &LimitOrder,
    order_tab: Option<OrderTab>,
    signature: &Signature,
    spent_amount_x: u64,
    spent_amount_y: u64,
    fee_taken_x: u64,
) -> Result<(bool, Option<NoteInfoExecutionOutput>, Option<OrderTab>, u64), SwapThreadExecutionError>
{
    let partial_fill_info = block_until_prev_fill_finished(
        partial_fill_tracker_m,
        blocked_order_ids_m,
        order.order_id,
    )?;

    // ? This proves the transaction is valid and the state can be updated
    check_order_validity(
        tree_m,
        &partial_fill_info,
        order,
        &order_tab,
        spent_amount_x,
        signature,
    )?;

    // ? This generates all the notes for the update
    let (is_partialy_filled, note_info_output, updated_order_tab, new_amount_filled) =
        execute_order_modifications(
            tree_m,
            &partial_fill_info,
            order,
            order_tab,
            spent_amount_x,
            spent_amount_y,
            fee_taken_x,
        );

    return Ok((
        is_partialy_filled,
        note_info_output,
        updated_order_tab,
        new_amount_filled,
    ));
}

fn execute_order_modifications(
    tree_m: &Arc<Mutex<SuperficialTree>>,
    partial_fill_info: &Option<(Option<Note>, u64)>,
    order: &LimitOrder,
    order_tab: Option<OrderTab>,
    spent_amount_x: u64,
    spent_amount_y: u64,
    fee_taken_x: u64,
) -> (bool, Option<NoteInfoExecutionOutput>, Option<OrderTab>, u64) {
    if order.spot_note_info.is_some() {
        let (
            is_partialy_filled,
            swap_note,
            new_partial_fill_info,
            prev_partial_fill_refund_note,
            new_amount_filled,
        ) = execute_non_tab_order_modifications(
            tree_m,
            partial_fill_info,
            order,
            spent_amount_x,
            spent_amount_y,
            fee_taken_x,
        );

        let note_info_output = NoteInfoExecutionOutput {
            new_partial_fill_info,
            prev_partial_fill_refund_note,
            swap_note,
        };

        return (
            is_partialy_filled,
            Some(note_info_output),
            None,
            new_amount_filled,
        );
    } else {
        let order_tab = order_tab.unwrap();

        let prev_filled_amount = if partial_fill_info.is_some() {
            partial_fill_info.as_ref().unwrap().1
        } else {
            0
        };

        let (is_partially_filled, updated_order_tab, new_amount_filled) =
            execute_tab_order_modifications(
                prev_filled_amount,
                order,
                order_tab,
                spent_amount_x,
                spent_amount_y,
                fee_taken_x,
            );

        return (
            is_partially_filled,
            None,
            Some(updated_order_tab),
            new_amount_filled,
        );
    }
}

fn check_order_validity(
    tree_m: &Arc<Mutex<SuperficialTree>>,
    partial_fill_info: &Option<(Option<Note>, u64)>,
    order: &LimitOrder,
    order_tab: &Option<OrderTab>,
    spent_amount: u64,
    signature: &Signature,
) -> Result<(), SwapThreadExecutionError> {
    //

    // ? Verify that the order were signed correctly
    order.verify_order_signature(signature, order_tab)?;

    if order.spot_note_info.is_some() {
        check_non_tab_order_validity(tree_m, partial_fill_info, order, spent_amount)?;
    } else {
        check_tab_order_validity(tree_m, order, order_tab, spent_amount)?;
    }

    return Ok(());
}

pub fn update_state_after_order(
    tree: &Arc<Mutex<SuperficialTree>>,
    updated_state_hashes: &Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
    spot_note_info: &Option<SpotNotesInfo>,
    note_info_output: &Option<NoteInfoExecutionOutput>,
    updated_order_tab: &Option<OrderTab>,
) {
    if spot_note_info.is_some() {
        let notes_in = &spot_note_info.as_ref().unwrap().notes_in;
        let refund_note = &spot_note_info.as_ref().unwrap().refund_note;
        let swap_note = &note_info_output.as_ref().unwrap().swap_note;
        let new_partial_fill_info = &note_info_output.as_ref().unwrap().new_partial_fill_info;
        let prev_partial_refund_note = &note_info_output
            .as_ref()
            .unwrap()
            .prev_partial_fill_refund_note;

        let is_first_fill = prev_partial_refund_note.is_none();

        update_state_after_non_tab_order(
            tree,
            updated_state_hashes,
            is_first_fill,
            notes_in,
            refund_note,
            swap_note,
            new_partial_fill_info,
        )
    } else {
        let updated_order_tab = updated_order_tab.as_ref().unwrap();

        update_state_after_tab_order(tree, updated_state_hashes, updated_order_tab);
    }
}

pub fn reverify_existances(
    state_tree: &Arc<Mutex<SuperficialTree>>,
    order_a: &LimitOrder,
    prev_order_tab_a: &Option<OrderTab>,
    note_info_output_a: &Option<NoteInfoExecutionOutput>,
    order_b: &LimitOrder,
    prev_order_tab_b: &Option<OrderTab>,
    note_info_output_b: &Option<NoteInfoExecutionOutput>,
) -> Result<(), SwapThreadExecutionError> {
   
    let state_tree = state_tree.lock();
  

    if note_info_output_a.is_some() {
        if note_info_output_a.is_some()
            && note_info_output_a
                .as_ref()
                .unwrap()
                .prev_partial_fill_refund_note
                .is_some()
        {
            let pfr_note = note_info_output_a
                .as_ref()
                .unwrap()
                .prev_partial_fill_refund_note
                .as_ref()
                .unwrap();

            let leaf_hash = state_tree.get_leaf_by_index(pfr_note.index);
            if leaf_hash != pfr_note.hash {
                return Err(send_swap_error(
                    "Note spent for swap does not exist in the state".to_string(),
                    Some(order_a.order_id),
                    Some(format!(
                        "note spent for swap does not exist in the state: hash={:?}",
                        pfr_note.hash,
                    )),
                ));
            }
        } else {
            let notes_in = &order_a.spot_note_info.as_ref().unwrap().notes_in;

            for note in notes_in.iter() {
                let leaf_hash = state_tree.get_leaf_by_index(note.index);
                if leaf_hash != note.hash {
                    return Err(send_swap_error(
                        "Note spent for swap does not exist in the state".to_string(),
                        Some(order_a.order_id),
                        Some(format!(
                            "note spent for swap does not exist in the state: hash={:?}",
                            note.hash,
                        )),
                    ));
                }
            }
        }
    } else {
        let order_tab = prev_order_tab_a.as_ref().unwrap();

        // ? Check that the order tab hash exists in the state --------------------------------------------
        if order_tab.hash != state_tree.get_leaf_by_index(order_tab.tab_idx as u64) {
            return Err(send_swap_error(
                "order_tab hash does not exist in the state".to_string(),
                Some(order_a.order_id),
                None,
            ));
        }
    }

    // * =========================================================================================

    if note_info_output_b.is_some() {
        if note_info_output_b.is_some()
            && note_info_output_b
                .as_ref()
                .unwrap()
                .prev_partial_fill_refund_note
                .is_some()
        {
            let pfr_note = note_info_output_b
                .as_ref()
                .unwrap()
                .prev_partial_fill_refund_note
                .as_ref()
                .unwrap();

            let leaf_hash = state_tree.get_leaf_by_index(pfr_note.index);
            if leaf_hash != pfr_note.hash {
                return Err(send_swap_error(
                    "Note spent for swap does not exist in the state".to_string(),
                    Some(order_b.order_id),
                    Some(format!(
                        "note spent for swap does not exist in the state: hash={:?}",
                        pfr_note.hash,
                    )),
                ));
            }
        } else {
            let notes_in = &order_b.spot_note_info.as_ref().unwrap().notes_in;

            for note in notes_in.iter() {
                let leaf_hash = state_tree.get_leaf_by_index(note.index);
                if leaf_hash != note.hash {
                    return Err(send_swap_error(
                        "Note spent for swap does not exist in the state".to_string(),
                        Some(order_b.order_id),
                        Some(format!(
                            "note spent for swap does not exist in the state: hash={:?}",
                            note.hash,
                        )),
                    ));
                }
            }
        }
    } else {
        let order_tab = prev_order_tab_b.as_ref().unwrap();

        // ? Check that the order tab hash exists in the state --------------------------------------------
        if order_tab.hash != state_tree.get_leaf_by_index(order_tab.tab_idx as u64) {
            return Err(send_swap_error(
                "order_tab hash does not exist in the state".to_string(),
                Some(order_b.order_id),
                None,
            ));
        }
    }


    return Ok(());
}
