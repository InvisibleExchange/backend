use num_bigint::BigUint;
use num_traits::Zero;
use parking_lot::Mutex;
use serde_json::Value;
use std::{collections::HashMap, str::FromStr, sync::Arc};

use crate::{
    transaction_batch::LeafNodeType, trees::superficial_tree::SuperficialTree, utils::notes::Note,
};

// * =============================================================================================================
// * PERP STATE RESTORE FUNCTIONS ================================================================================

pub fn restore_after_perp_swap_first_fill(
    tree_m: &Arc<Mutex<SuperficialTree>>,
    updated_state_hashes_m: &Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
    perpetual_partial_fill_tracker_m: &Arc<Mutex<HashMap<u64, (Option<Note>, u64, u64)>>>,
    order_id: u64,
    notes_in: &Vec<Value>,
    refund_note: Option<&Value>,
    new_pfr_idx: &Option<&Value>,
    new_pfr_hash: &Option<&Value>,
) {
    let mut tree = tree_m.lock();
    let mut updated_state_hashes = updated_state_hashes_m.lock();

    let refund_idx = notes_in[0].get("index").unwrap().as_u64().unwrap();
    let refund_note_hash = if refund_note.unwrap().is_null() {
        BigUint::zero()
    } else {
        BigUint::from_str(refund_note.unwrap().get("hash").unwrap().as_str().unwrap()).unwrap()
    };

    tree.update_leaf_node(&refund_note_hash, refund_idx);
    updated_state_hashes.insert(refund_idx, (LeafNodeType::Note, refund_note_hash));

    if !new_pfr_hash.unwrap().is_null() {
        //

        let idx: u64 = new_pfr_idx.unwrap().as_u64().unwrap();
        let hash = BigUint::from_str(new_pfr_hash.unwrap().as_str().unwrap()).unwrap();

        tree.update_leaf_node(&hash, idx);
        updated_state_hashes.insert(idx, (LeafNodeType::Note, hash));

        //
    } else {
        if notes_in.len() > 1 {
            let idx = notes_in[1].get("index").unwrap().as_u64().unwrap();

            tree.update_leaf_node(&BigUint::zero(), idx);
            updated_state_hashes.insert(idx, (LeafNodeType::Note, BigUint::zero()));
        }

        let mut pft = perpetual_partial_fill_tracker_m.lock();
        pft.remove(&order_id);
        drop(pft);
    }

    for i in 2..notes_in.len() {
        let idx = notes_in[i].get("index").unwrap().as_u64().unwrap();

        tree.update_leaf_node(&BigUint::zero(), idx);
        updated_state_hashes.insert(idx, (LeafNodeType::Note, BigUint::zero()));
    }

    drop(tree);
    drop(updated_state_hashes);
}

pub fn restore_after_perp_swap_later_fills(
    tree_m: &Arc<Mutex<SuperficialTree>>,
    updated_state_hashes_m: &Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
    perpetual_partial_fill_tracker_m: &Arc<Mutex<HashMap<u64, (Option<Note>, u64, u64)>>>,
    order_id: u64,
    prev_pfr_idx: u64,
    new_pfr_idx: &Option<&Value>,
    new_pfr_hash: &Option<&Value>,
) {
    let mut tree = tree_m.lock();
    let mut updated_state_hashes = updated_state_hashes_m.lock();

    if !new_pfr_hash.unwrap().is_null() {
        let idx: u64 = new_pfr_idx.unwrap().as_u64().unwrap();
        let hash = BigUint::from_str(new_pfr_hash.unwrap().as_str().unwrap()).unwrap();

        tree.update_leaf_node(&hash, idx);
        updated_state_hashes.insert(idx, (LeafNodeType::Note, hash));
    } else {
        tree.update_leaf_node(&BigUint::zero(), prev_pfr_idx);
        updated_state_hashes.insert(prev_pfr_idx, (LeafNodeType::Note, BigUint::zero()));

        let mut pft = perpetual_partial_fill_tracker_m.lock();
        pft.remove(&order_id);
        drop(pft);
    }

    drop(updated_state_hashes);
    drop(tree);
}

pub fn restore_return_collateral_note(
    tree_m: &Arc<Mutex<SuperficialTree>>,
    updated_state_hashes_m: &Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
    ret_collatera_note_idx: &Value,
    ret_collatera_note_hash: &Value,
) {
    let mut tree = tree_m.lock();
    let mut updated_state_hashes = updated_state_hashes_m.lock();

    let idx = ret_collatera_note_idx.as_u64().unwrap();
    let hash = BigUint::from_str(ret_collatera_note_hash.as_str().unwrap()).unwrap();

    tree.update_leaf_node(&hash, idx);
    updated_state_hashes.insert(idx, (LeafNodeType::Note, hash));

    drop(updated_state_hashes);
    drop(tree);
}

// ! UPDATING PERPETUAL STATE ! // ============================================
pub fn restore_perpetual_state(
    tree_m: &Arc<Mutex<SuperficialTree>>,
    updated_state_hashes_m: &Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
    position_index: &Option<&Value>,
    position_hash: Option<&Value>,
) {
    //

    let mut state_tree = tree_m.lock();
    let mut updated_state_hashes = updated_state_hashes_m.lock();
    if !position_hash.unwrap().is_null() {
        let idx = position_index.unwrap().as_u64().unwrap();
        let hash = BigUint::from_str(position_hash.unwrap().as_str().unwrap()).unwrap();

        state_tree.update_leaf_node(&hash, idx);
        updated_state_hashes.insert(idx, (LeafNodeType::Position, hash));
    } else {
        let idx = position_index.unwrap().as_u64().unwrap();

        state_tree.update_leaf_node(&BigUint::zero(), idx);
        updated_state_hashes.insert(idx, (LeafNodeType::Position, BigUint::zero()));
    }
    drop(state_tree);
    drop(updated_state_hashes);
}
