use parking_lot::{Mutex, MutexGuard};
use std::{collections::HashMap, sync::Arc};

use error_stack::Result;
use num_bigint::BigUint;
use num_traits::Zero;

use crate::{
    transaction_batch::{LeafNodeType, StateUpdate, TxOutputJson},
    trees::superficial_tree::SuperficialTree,
    utils::{
        errors::{send_withdrawal_error, WithdrawalThreadExecutionError},
        notes::Note,
    },
};

// * ================================================================================================================================================
// * Swap state updates -----------------------------------------------------------------------------------------------------------------------------

// ! FIRST FILL ! // ==================

pub fn update_state_after_swap_first_fill(
    tree_m: &Arc<Mutex<SuperficialTree>>,
    updated_state_hashes_m: &Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
    transaction_output_json: &mut MutexGuard<TxOutputJson>,
    notes_in: &Vec<Note>,
    refund_note: &Option<Note>,
    swap_note: &Note,
    partial_fill_refund_note: &Option<&Note>,
) {
    //

    // ? Get lock to mutable values
    let mut tree = tree_m.lock();
    let mut updated_state_hashes = updated_state_hashes_m.lock();

    // ? Update the state tree -———————————————————————————————————
    let refund_idx = notes_in[0].index;
    let refund_hash = if refund_note.is_some() {
        refund_note.as_ref().unwrap().hash.clone()
    } else {
        BigUint::zero()
    };

    tree.update_leaf_node(&refund_hash, refund_idx);
    updated_state_hashes.insert(refund_idx, (LeafNodeType::Note, refund_hash));
    if let Some(note) = refund_note {
        transaction_output_json
            .state_updates
            .push(StateUpdate::Note { note: note.clone() });
    }

    let swap_idx = swap_note.index;

    tree.update_leaf_node(&swap_note.hash, swap_idx);
    updated_state_hashes.insert(swap_idx, (LeafNodeType::Note, swap_note.hash.clone()));
    transaction_output_json
        .state_updates
        .push(StateUpdate::Note {
            note: swap_note.clone(),
        });

    if partial_fill_refund_note.is_some() {
        //
        let note = partial_fill_refund_note.unwrap();
        let idx: u64 = note.index;

        tree.update_leaf_node(&note.hash, idx);
        updated_state_hashes.insert(idx, (LeafNodeType::Note, note.hash.clone()));
        transaction_output_json
            .state_updates
            .push(StateUpdate::Note { note: note.clone() });
        //
    }

    for i in 2..notes_in.len() {
        let idx = notes_in[i].index;

        tree.update_leaf_node(&BigUint::zero(), idx);
        updated_state_hashes.insert(idx, (LeafNodeType::Note, BigUint::zero()));
    }

    drop(updated_state_hashes);
    drop(tree);
}

// ! LATER FILLS ! // =================

pub fn update_state_after_swap_later_fills(
    tree_m: &Arc<Mutex<SuperficialTree>>,
    updated_state_hashes_m: &Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
    transaction_output_json: &mut MutexGuard<TxOutputJson>,
    swap_note: &Note,
    new_partial_fill_refund_note: &Option<&Note>,
) {
    //

    // ? Get mutable pointer locks
    let mut tree = tree_m.lock();
    let mut updated_state_hashes = updated_state_hashes_m.lock();

    // ? Update the state tree
    let swap_idx = swap_note.index;

    tree.update_leaf_node(&swap_note.hash, swap_idx);
    updated_state_hashes.insert(swap_idx, (LeafNodeType::Note, swap_note.hash.clone()));
    transaction_output_json
        .state_updates
        .push(StateUpdate::Note {
            note: swap_note.clone(),
        });

    if new_partial_fill_refund_note.is_some() {
        let pfr_note: &Note = new_partial_fill_refund_note.as_ref().unwrap();
        let pfr_idx = pfr_note.index;

        tree.update_leaf_node(&pfr_note.hash, pfr_idx);
        updated_state_hashes.insert(pfr_idx, (LeafNodeType::Note, pfr_note.hash.clone()));
        transaction_output_json
            .state_updates
            .push(StateUpdate::Note {
                note: pfr_note.clone(),
            });
    }

    drop(updated_state_hashes);
    drop(tree);
}

// * Deposit state updates ----------------------------------------------------------------------------------------------------------------------

/// Adds the new notes to the state
pub fn update_state_after_deposit(
    tree: &mut SuperficialTree,
    updated_state_hashes_m: &Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
    transaction_output_json: &Arc<Mutex<TxOutputJson>>,
    notes: &Vec<Note>,
) {
    //

    // ? Upadte the state by adding the note hashes to the merkle tree
    let mut updated_state_hashes = updated_state_hashes_m.lock();
    let mut transaction_output_json_m = transaction_output_json.lock();

    for note in notes.iter() {
        let idx = note.index;

        tree.update_leaf_node(&note.hash, idx);
        updated_state_hashes.insert(idx, (LeafNodeType::Note, note.hash.clone()));
        transaction_output_json_m
            .state_updates
            .push(StateUpdate::Note { note: note.clone() });
    }
    drop(updated_state_hashes);
    drop(transaction_output_json_m);
}

// * ===============================================================================================================================================
// * Withdrawal state updates ----------------------------------------------------------------------------------------------------------------------

pub fn update_state_after_withdrawal(
    tree: &mut SuperficialTree,
    updated_state_hashes: &mut HashMap<u64, (LeafNodeType, BigUint)>,
    transaction_output_json: &Arc<Mutex<TxOutputJson>>,
    notes_in: &Vec<Note>,
    refund_note: &Option<Note>,
) -> Result<(), WithdrawalThreadExecutionError> {
    //

    // ? Remove the notes_in from the tree and add the refund note
    let refund_idx = notes_in[0].index;

    let mut refund_note_hash = &BigUint::zero();
    if let Some(refund_note) = refund_note {
        refund_note_hash = &refund_note.hash
    };
    let leaf_hash = tree.get_leaf_by_index(refund_idx);
    if leaf_hash != notes_in[0].hash {
        return Err(send_withdrawal_error(
            "note withdrawn does not exist in the state".to_string(),
            None,
        ));
    }

    let mut transaction_output_json_m = transaction_output_json.lock();

    tree.update_leaf_node(refund_note_hash, refund_idx);
    updated_state_hashes.insert(refund_idx, (LeafNodeType::Note, refund_note_hash.clone()));
    if let Some(refund_note) = refund_note {
        transaction_output_json_m
            .state_updates
            .push(StateUpdate::Note {
                note: refund_note.clone(),
            });
    };

    for note in notes_in.iter().skip(1) {
        let idx = note.index;

        // ?verify notes exist in the tree
        let leaf_hash = tree.get_leaf_by_index(idx);
        if leaf_hash != note.hash {
            return Err(send_withdrawal_error(
                "note withdrawn does not exist in the state".to_string(),
                None,
            ));
        }

        tree.update_leaf_node(&BigUint::zero(), idx);
        updated_state_hashes.insert(idx, (LeafNodeType::Note, BigUint::zero()));
    }

    Ok(())
}
