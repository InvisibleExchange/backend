// * ==============================================================================
// * STATE UPDATE FUNCTIONS * //

use std::{collections::HashMap, sync::Arc};

use num_bigint::BigUint;
use num_traits::Zero;
use parking_lot::Mutex;

use crate::transaction_batch::LeafNodeType;
use crate::utils::crypto_utils::EcPoint;
use crate::{trees::superficial_tree::SuperficialTree, utils::notes::Note};

use super::super::{perp_position::PerpPosition, PositionEffectType};

// ! FIRST FILL ! // ===================== (OPEN ORDERS) =====================
pub fn update_state_after_swap_first_fill(
    //
    state_tree_m: &Arc<Mutex<SuperficialTree>>,
    updated_state_hashes_m: &Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
    notes_in: &Vec<Note>,
    refund_note: &Option<Note>,
    partial_fill_refund_note: Option<&Note>,
) {
    let mut tree = state_tree_m.lock();
    let mut updated_state_hashes = updated_state_hashes_m.lock();

    // ? Update the state tree
    let refund_idx = notes_in[0].index;
    let refund_hash = if refund_note.is_some() {
        refund_note.as_ref().unwrap().hash.clone()
    } else {
        BigUint::zero()
    };

    tree.update_leaf_node(&refund_hash, refund_idx);
    updated_state_hashes.insert(refund_idx, (LeafNodeType::Note, refund_hash));

    if partial_fill_refund_note.is_some() {
        //

        let note = partial_fill_refund_note.unwrap();
        let idx: u64 = note.index;

        tree.update_leaf_node(&note.hash, idx);
        updated_state_hashes.insert(idx, (LeafNodeType::Note, note.hash.clone()));
        //
    }

    for i in 1..notes_in.len() {
        let idx = notes_in[i].index;

        tree.update_leaf_node(&BigUint::zero(), idx);
        updated_state_hashes.insert(idx, (LeafNodeType::Note, BigUint::zero()));
    }
    drop(tree);
    drop(updated_state_hashes);
}

// ! LATER FILL ! // ===================== (OPEN ORDERS) =====================
pub fn update_state_after_swap_later_fills(
    state_tree_m: &Arc<Mutex<SuperficialTree>>,
    updated_state_hashes_m: &Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
    prev_partial_fill_refund_note: Note,
    new_partial_fill_refund_note: Option<&Note>,
) {
    let mut tree = state_tree_m.lock();
    let mut updated_state_hashes = updated_state_hashes_m.lock();

    if new_partial_fill_refund_note.is_some() {
        let pfr_note: &Note = new_partial_fill_refund_note.as_ref().unwrap();
        let pfr_idx = pfr_note.index;

        tree.update_leaf_node(&pfr_note.hash, pfr_idx);
        updated_state_hashes.insert(pfr_idx, (LeafNodeType::Note, pfr_note.hash.clone()));
    } else {
        let pfr_idx = prev_partial_fill_refund_note.index;

        tree.update_leaf_node(&BigUint::zero(), pfr_idx);
        updated_state_hashes.insert(pfr_idx, (LeafNodeType::Note, BigUint::zero()));
    }

    drop(tree);
    drop(updated_state_hashes);
}

// ! UPDATING PERPETUAL STATE ! // ============================================
pub fn update_perpetual_state(
    state_tree_m: &Arc<Mutex<SuperficialTree>>,
    updated_state_hashes_m: &Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
    position_effect_type: &PositionEffectType,
    position_idx: u64,
    position: Option<&PerpPosition>,
) {
    //

    // TODO: Should check that the position exists in the tree

    let mut state_tree = state_tree_m.lock();
    let mut updated_state_hashes = updated_state_hashes_m.lock();
    if *position_effect_type == PositionEffectType::Open {
        let position = position.unwrap();

        state_tree.update_leaf_node(&position.hash, position.index as u64);
        updated_state_hashes.insert(
            position.index as u64,
            (LeafNodeType::Position, position.hash.clone()),
        );
    } else {
        let position_hash: BigUint;
        if position.is_some() {
            position_hash = position.unwrap().hash.clone();
        } else {
            position_hash = BigUint::zero()
        };

        state_tree.update_leaf_node(&position_hash, position_idx as u64);
        updated_state_hashes.insert(position_idx as u64, (LeafNodeType::Position, position_hash));
    }
    drop(state_tree);
    drop(updated_state_hashes);
}

// ! RETURN COLLATERAL ON POSITION CLOSE ! // =======
pub fn return_collateral_on_position_close(
    state_tree_m: &Arc<Mutex<SuperficialTree>>,
    updated_state_hashes_m: &Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
    idx: u64,
    collateral_return_amount: u64,
    collateral_token: u32,
    collateral_returned_address: &EcPoint,
    collateral_returned_blinding: &BigUint,
) -> Note {
    let return_collateral_note = Note::new(
        idx,
        collateral_returned_address.clone(),
        collateral_token,
        collateral_return_amount,
        collateral_returned_blinding.clone(),
    );

    let mut tree = state_tree_m.lock();
    let mut updated_state_hashes = updated_state_hashes_m.lock();

    tree.update_leaf_node(&return_collateral_note.hash, idx);
    updated_state_hashes.insert(
        idx,
        (LeafNodeType::Note, return_collateral_note.hash.clone()),
    );
    drop(tree);
    drop(updated_state_hashes);

    return return_collateral_note;
}
