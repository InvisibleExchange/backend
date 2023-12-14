//

use std::{collections::HashMap, sync::Arc};

use error_stack::Result;
use num_bigint::BigUint;
use parking_lot::Mutex;

use crate::{
    perpetual::DUST_AMOUNT_PER_ASSET,
    transaction_batch::LeafNodeType,
    transactions::{
        limit_order::LimitOrder,
        transaction_helpers::state_updates::{
            update_state_after_swap_first_fill, update_state_after_swap_later_fills,
        },
    },
    trees::superficial_tree::SuperficialTree,
    utils::{
        errors::{send_swap_error, SwapThreadExecutionError},
        notes::Note,
    },
};

use super::non_tab_helpers::{
    check_note_sums, check_prev_fill_consistencies, construct_new_swap_note, refund_partial_fill,
};

//* THIS ARE THE HELPER FUNCTIONS FOR REGULAR LIMIT ORDERS THAT USE spot_notes_info AND NOT AN order_tab */
// *

// * CHECK ORDER VALIDITY FUNCTION * --------------------------------------------
pub fn check_non_tab_order_validity(
    tree_m: &Arc<Mutex<SuperficialTree>>,
    partial_fill_info: &Option<(Option<Note>, u64)>,
    order: &LimitOrder,
    spent_amount: u64,
) -> Result<(), SwapThreadExecutionError> {
    let note_info = order.spot_note_info.as_ref().unwrap();

    let is_first_fill =
        partial_fill_info.is_none() || partial_fill_info.as_ref().unwrap().0.is_none();

    // ? Check the sum of notes in matches refund and output amounts
    if is_first_fill {
        // ? if this is the first fill
        check_note_sums(&order)?;

        if let Some(rf_note) = &note_info.refund_note {
            if note_info.notes_in[0].index != rf_note.index {
                return Err(send_swap_error(
                    "refund note index is not the same as the first note index".to_string(),
                    Some(order.order_id),
                    None,
                ));
            }
        }
    } else {
        // ? if order was partially filled befor
        check_prev_fill_consistencies(partial_fill_info, &order, spent_amount)?;
    }

    // ? Verify the notes exist in the state
    let tree = tree_m.lock();
    if is_first_fill {
        for note in note_info.notes_in.iter() {
            let leaf_hash = tree.get_leaf_by_index(note.index);

            if leaf_hash != note.hash {
                return Err(send_swap_error(
                    "note spent for swap does not exist in the state".to_string(),
                    Some(order.order_id),
                    Some(format!(
                        "note spent for swap does not exist in the state: hash={:?}",
                        note.hash,
                    )),
                ));
            }
        }
    } else {
        let pfr_note = &partial_fill_info.as_ref().unwrap().0.as_ref().unwrap();
        let leaf_hash = tree.get_leaf_by_index(pfr_note.index);
        if leaf_hash != pfr_note.hash {
            return Err(send_swap_error(
                "prev partial refund note used in swap does not exist in the state".to_string(),
                Some(order.order_id),
                Some(format!(
                    "prev partial refund note used in swap does not exist in the state: hash={:?}",
                    pfr_note.hash,
                )),
            ));
        }
    }
    drop(tree);

    Ok(())
}

// * CHECK ORDER VALIDITY FUNCTION * --------------------------------------------
pub fn execute_non_tab_order_modifications(
    tree_m: &Arc<Mutex<SuperficialTree>>,
    partial_fill_info: &Option<(Option<Note>, u64)>,
    order: &LimitOrder,
    spent_amount_x: u64,
    spent_amount_y: u64,
    fee_taken_x: u64,
) -> (bool, Note, Option<(Option<Note>, u64)>, Option<Note>, u64) {
    let is_first_fill =
        partial_fill_info.is_none() || partial_fill_info.as_ref().unwrap().0.is_none();
    let note_info = order.spot_note_info.as_ref().unwrap();

    // ? Generate new swap notes ============================
    let swap_note: Note = construct_new_swap_note(
        partial_fill_info,
        tree_m,
        is_first_fill,
        &note_info,
        order.token_received,
        spent_amount_y,
        fee_taken_x,
    );

    // ? Update previous and new partial fills ==========================
    let prev_amount_filled = if is_first_fill {
        0
    } else {
        partial_fill_info.as_ref().unwrap().1
    };
    let new_amount_filled = prev_amount_filled + spent_amount_y;

    let spend_amount_left = if is_first_fill {
        order.amount_spent
    } else {
        partial_fill_info
            .as_ref()
            .unwrap()
            .0
            .as_ref()
            .unwrap()
            .amount
    };

    let prev_partial_fill_refund_note: Option<Note>;
    let new_partial_refund_note: Option<Note>;

    let is_partially_filled =
        spend_amount_left - spent_amount_x >= DUST_AMOUNT_PER_ASSET[&order.token_spent.to_string()];
    if is_partially_filled {
        //? Order A was partially filled, we must refund the rest

        let mut tree = tree_m.lock();
        let partial_refund_idx = tree.first_zero_idx();
        drop(tree);

        let new_partial_refund_note_ = refund_partial_fill(
            spend_amount_left,
            note_info,
            order.token_spent,
            spent_amount_x,
            partial_refund_idx,
        );
        prev_partial_fill_refund_note = if partial_fill_info.is_some() {
            Some(
                partial_fill_info
                    .as_ref()
                    .unwrap()
                    .0
                    .as_ref()
                    .unwrap()
                    .clone(),
            )
        } else {
            None
        };
        new_partial_refund_note = Some(new_partial_refund_note_);
    } else {
        prev_partial_fill_refund_note = if partial_fill_info.is_some() {
            Some(
                partial_fill_info
                    .as_ref()
                    .unwrap()
                    .0
                    .as_ref()
                    .unwrap()
                    .clone(),
            )
        } else {
            None
        };
        new_partial_refund_note = None;
    }

    let new_pratial_fill_info = if is_partially_filled {
        Some((new_partial_refund_note, new_amount_filled))
    } else {
        None
    };

    return (
        is_partially_filled,
        swap_note,
        new_pratial_fill_info,
        prev_partial_fill_refund_note,
        new_amount_filled,
    );
}

// * UPDATE STATE AFTER SWAP * --------------------------------------------------

pub fn update_state_after_non_tab_order(
    tree: &Arc<Mutex<SuperficialTree>>,
    updated_note_hashes: &Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
    is_first_fill: bool,
    notes_in: &Vec<Note>,
    refund_note: &Option<Note>,
    swap_note: &Note,
    new_partial_fill_info: &Option<(Option<Note>, u64)>,
) {
    let mut new_partial_refund_note: Option<Note> = None;
    if let Some(new_pfr_note) = new_partial_fill_info.as_ref() {
        new_partial_refund_note = Some(new_pfr_note.0.as_ref().unwrap().clone());
    }

    // ? Update the state for order a
    if is_first_fill {
        update_state_after_swap_first_fill(
            tree,
            updated_note_hashes,
            notes_in,
            refund_note,
            &swap_note,
            &new_partial_refund_note.as_ref(),
        );
    } else {
        update_state_after_swap_later_fills(
            tree,
            updated_note_hashes,
            swap_note,
            &new_partial_refund_note.as_ref(),
        );
    }
}
