use std::{collections::HashMap, str::FromStr, sync::Arc};

use num_bigint::BigUint;
use num_traits::Zero;
use parking_lot::Mutex;
use serde_json::{Map, Value};

use crate::{transaction_batch::LeafNodeType, trees::superficial_tree::SuperficialTree};

pub fn restore_forced_note_escape(
    tree_m: &Arc<Mutex<SuperficialTree>>,
    updated_state_hashes_m: &Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
    transaction: &Map<String, Value>,
) {
    let note_escape = transaction.get("note_escape").unwrap();

    let is_valid = note_escape.get("invalid_note").unwrap().is_null();
    if !is_valid {
        return;
    }

    let escape_note_indexes = note_escape
        .get("escape_notes")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .map(|note| note.get("index").unwrap().as_u64().unwrap())
        .collect::<Vec<u64>>();

    let mut state_tree = tree_m.lock();
    let mut updated_state_hashes = updated_state_hashes_m.lock();

    for index in escape_note_indexes {
        state_tree.update_leaf_node(&BigUint::zero(), index);
        updated_state_hashes.insert(index, (LeafNodeType::Note, BigUint::zero()));
    }

    drop(state_tree);
    drop(updated_state_hashes);
}

pub fn restore_forced_tab_escape(
    tree_m: &Arc<Mutex<SuperficialTree>>,
    updated_state_hashes_m: &Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
    transaction: &Map<String, Value>,
) {
    let tab_escape = transaction.get("tab_escape").unwrap();

    let is_valid = tab_escape
        .get("is_valid")
        .unwrap()
        .as_bool()
        .unwrap_or_default();
    if !is_valid {
        return;
    }

    // ? Order tab
    let order_tab = tab_escape.get("order_tab").unwrap();
    let idx: u64 = order_tab.get("tab_idx").unwrap().as_u64().unwrap();

    let mut state_tree = tree_m.lock();
    let mut updated_state_hashes = updated_state_hashes_m.lock();

    state_tree.update_leaf_node(&BigUint::zero(), idx);
    updated_state_hashes.insert(idx, (LeafNodeType::OrderTab, BigUint::zero()));

    drop(state_tree);
    drop(updated_state_hashes);
}

pub fn restore_forced_position_escape(
    tree_m: &Arc<Mutex<SuperficialTree>>,
    updated_state_hashes_m: &Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
    transaction: &Map<String, Value>,
) {
    let position_escape = transaction.get("position_escape").unwrap();

    let new_position_index = position_escape
        .get("position_idx")
        .unwrap()
        .as_u64()
        .unwrap();
    let new_position_hash = position_escape.get("new_position_hash").unwrap();
    if new_position_hash.is_null() || new_position_hash.as_str().unwrap() == "" {
        return;
    }

    let mut state_tree = tree_m.lock();
    let mut updated_state_hashes = updated_state_hashes_m.lock();
    let open_order_fields_b = position_escape.get("open_order_fields_b").unwrap();

    if !open_order_fields_b.is_null() {
        let notes_in = open_order_fields_b
            .get("notes_in")
            .unwrap()
            .as_array()
            .unwrap();
        let refund_note = open_order_fields_b.get("refund_note").unwrap();

        for note in notes_in {
            let idx = note.get("index").unwrap().as_u64().unwrap();
            state_tree.update_leaf_node(&BigUint::zero(), idx);
            updated_state_hashes.insert(idx, (LeafNodeType::Note, BigUint::zero()));
        }
        if !refund_note.is_null() {
            let idx = refund_note.get("index").unwrap().as_u64().unwrap();
            state_tree.update_leaf_node(&BigUint::zero(), idx);
            updated_state_hashes.insert(idx, (LeafNodeType::Note, BigUint::zero()));
        }
    }

    let position_a = position_escape.get("position_a").unwrap();
    let idx = position_a.get("index").unwrap().as_u64().unwrap();
    state_tree.update_leaf_node(&BigUint::zero(), idx);
    updated_state_hashes.insert(idx, (LeafNodeType::Position, BigUint::zero()));

    let new_position_hash = BigUint::from_str(new_position_hash.as_str().unwrap()).unwrap();

    state_tree.update_leaf_node(&new_position_hash, new_position_index);
    updated_state_hashes.insert(
        new_position_index,
        (LeafNodeType::Position, new_position_hash),
    );

    drop(state_tree);
    drop(updated_state_hashes);
}
