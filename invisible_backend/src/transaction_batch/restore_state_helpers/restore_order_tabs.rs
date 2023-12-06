use num_bigint::BigUint;
use num_traits::Zero;
use parking_lot::Mutex;
use serde_json::{Map, Value};
use std::{collections::HashMap, str::FromStr, sync::Arc};

use crate::{transaction_batch::LeafNodeType, trees::superficial_tree::SuperficialTree};

// * OPEN ORDER TAB RESTORE FUNCTIONS ================================================================================

pub fn restore_open_order_tab(
    tree_m: &Arc<Mutex<SuperficialTree>>,
    updated_state_hashes_m: &Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
    transaction: &Map<String, Value>,
) {
    let mut state_tree = tree_m.lock();
    let mut updated_state_hashes = updated_state_hashes_m.lock();

    let base_notes_in = transaction
        .get("base_notes_in")
        .unwrap()
        .as_array()
        .unwrap();
    let base_refund_note = transaction.get("base_refund_note").unwrap();
    let quote_notes_in = transaction
        .get("quote_notes_in")
        .unwrap()
        .as_array()
        .unwrap();
    let quote_refund_note = transaction.get("quote_refund_note").unwrap();

    // ? Base notes
    for note in base_notes_in {
        let idx = note.get("index").unwrap().as_u64().unwrap();
        // let note_out_hash = BigUint::from_str(note.get("hash").unwrap().as_str().unwrap()).unwrap();
        state_tree.update_leaf_node(&BigUint::zero(), idx);
        updated_state_hashes.insert(idx, (LeafNodeType::Note, BigUint::zero()));
    }
    if !base_refund_note.is_null() {
        let idx = base_refund_note.get("index").unwrap().as_u64().unwrap();
        let note_out_hash =
            BigUint::from_str(base_refund_note.get("hash").unwrap().as_str().unwrap()).unwrap();
        state_tree.update_leaf_node(&note_out_hash, idx);
        updated_state_hashes.insert(idx, (LeafNodeType::Note, note_out_hash));
    }

    // ? Quote notes
    for note in quote_notes_in {
        let idx = note.get("index").unwrap().as_u64().unwrap();
        // let note_out_hash = BigUint::from_str(note.get("hash").unwrap().as_str().unwrap()).unwrap();
        state_tree.update_leaf_node(&BigUint::zero(), idx);
        updated_state_hashes.insert(idx, (LeafNodeType::Note, BigUint::zero()));
    }
    if !quote_refund_note.is_null() {
        let idx = quote_refund_note.get("index").unwrap().as_u64().unwrap();
        let note_out_hash =
            BigUint::from_str(quote_refund_note.get("hash").unwrap().as_str().unwrap()).unwrap();
        state_tree.update_leaf_node(&note_out_hash, idx);
        updated_state_hashes.insert(idx, (LeafNodeType::Note, note_out_hash));
    }

    // ? Order tab
    let order_tab = transaction.get("order_tab").unwrap();
    let idx: u64 = order_tab.get("tab_idx").unwrap().as_u64().unwrap();
    let tab_hash = order_tab.get("hash").unwrap().as_str().unwrap();
    let tab_hash = BigUint::from_str(tab_hash).unwrap();

    state_tree.update_leaf_node(&tab_hash, idx);
    updated_state_hashes.insert(idx, (LeafNodeType::OrderTab, tab_hash));

    drop(state_tree);
    drop(updated_state_hashes);
}

// * CLOSE ORDER TAB RESTORE FUNCTIONS ================================================================================

pub fn restore_close_order_tab(
    tree_m: &Arc<Mutex<SuperficialTree>>,
    updated_state_hashes_m: &Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
    transaction: &Map<String, Value>,
) {
    let mut state_tree = tree_m.lock();
    let mut updated_state_hashes = updated_state_hashes_m.lock();

    let base_return_note_index = transaction.get("base_return_note_idx").unwrap();
    let base_return_note_hash = transaction.get("base_return_note_hash").unwrap();
    let base_return_note_hash = BigUint::from_str(base_return_note_hash.as_str().unwrap()).unwrap();

    let quote_return_note_index = transaction.get("quote_return_note_idx").unwrap();
    let quote_refund_note_hash = transaction.get("quote_return_note_hash").unwrap();
    let quote_refund_note_hash =
        BigUint::from_str(quote_refund_note_hash.as_str().unwrap()).unwrap();

    state_tree.update_leaf_node(
        &base_return_note_hash,
        base_return_note_index.as_u64().unwrap(),
    );
    updated_state_hashes.insert(
        base_return_note_index.as_u64().unwrap(),
        (LeafNodeType::Note, base_return_note_hash),
    );

    state_tree.update_leaf_node(
        &quote_refund_note_hash,
        quote_return_note_index.as_u64().unwrap(),
    );
    updated_state_hashes.insert(
        quote_return_note_index.as_u64().unwrap(),
        (LeafNodeType::Note, quote_refund_note_hash),
    );

    // ? Order tab
    let order_tab = transaction.get("order_tab").unwrap();
    let idx: u64 = order_tab.get("tab_idx").unwrap().as_u64().unwrap();
    let updated_tab_hash = order_tab.get("updated_tab_hash").unwrap().as_str().unwrap();
    let updated_tab_hash = BigUint::from_str(updated_tab_hash).unwrap();

    state_tree.update_leaf_node(&updated_tab_hash, idx);
    updated_state_hashes.insert(idx, (LeafNodeType::OrderTab, updated_tab_hash));

    drop(state_tree);
    drop(updated_state_hashes);
}

// * REGISTER MM RESTORE FUNCTIONS ================================================================================
pub fn restore_register_mm(
    tree_m: &Arc<Mutex<SuperficialTree>>,
    updated_state_hashes_m: &Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
    transaction: &Map<String, Value>,
) {
    let mut state_tree = tree_m.lock();
    let mut updated_state_hashes = updated_state_hashes_m.lock();

    let idx = transaction.get("vlp_note_idx").unwrap().as_u64().unwrap();
    let note_out_hash =
        BigUint::from_str(transaction.get("vlp_note_hash").unwrap().as_str().unwrap()).unwrap();
    state_tree.update_leaf_node(&note_out_hash, idx);
    updated_state_hashes.insert(idx, (LeafNodeType::Note, note_out_hash));

    // ? Order tab
    let order_tab = transaction.get("prev_order_tab").unwrap();
    if !order_tab.is_null() {
        let idx: u64 = order_tab.get("tab_idx").unwrap().as_u64().unwrap();
        let tab_hash = transaction
            .get("new_order_tab_hash")
            .unwrap()
            .as_str()
            .unwrap();
        let tab_hash = BigUint::from_str(tab_hash).unwrap();

        state_tree.update_leaf_node(&tab_hash, idx);
        updated_state_hashes.insert(idx, (LeafNodeType::OrderTab, tab_hash));
    }

    // ? Position
    let position = transaction.get("prev_position").unwrap();
    if !position.is_null() {
        let idx: u64 = position.get("index").unwrap().as_u64().unwrap();
        let pos_hash = transaction
            .get("new_position_hash")
            .unwrap()
            .as_str()
            .unwrap();
        let pos_hash = BigUint::from_str(pos_hash).unwrap();

        state_tree.update_leaf_node(&pos_hash, idx);
        updated_state_hashes.insert(idx, (LeafNodeType::Position, pos_hash));
    }

    drop(state_tree);
    drop(updated_state_hashes);
}

// * ADD LIQUIDITY RESTORE FUNCTIONS ================================================================================

pub fn restore_add_liquidity(
    tree_m: &Arc<Mutex<SuperficialTree>>,
    updated_state_hashes_m: &Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
    transaction: &Map<String, Value>,
) {
    let is_order_tab = transaction.get("is_order_tab").unwrap().as_bool().unwrap();

    let mut state_tree = tree_m.lock();
    let mut updated_state_hashes = updated_state_hashes_m.lock();
    if is_order_tab {
        let base_notes_in = transaction
            .get("base_notes_in")
            .unwrap()
            .as_array()
            .unwrap();
        let base_refund_note = transaction.get("base_refund_note").unwrap();
        let quote_notes_in = transaction
            .get("quote_notes_in")
            .unwrap()
            .as_array()
            .unwrap();
        let quote_refund_note = transaction.get("quote_refund_note").unwrap();

        // ? Base notes
        for note in base_notes_in {
            let idx = note.get("index").unwrap().as_u64().unwrap();
            // let note_out_hash = BigUint::from_str(note.get("hash").unwrap().as_str().unwrap()).unwrap();
            state_tree.update_leaf_node(&BigUint::zero(), idx);
            updated_state_hashes.insert(idx, (LeafNodeType::Note, BigUint::zero()));
        }
        if !base_refund_note.is_null() {
            let idx = base_refund_note.get("index").unwrap().as_u64().unwrap();
            let note_out_hash =
                BigUint::from_str(base_refund_note.get("hash").unwrap().as_str().unwrap()).unwrap();
            state_tree.update_leaf_node(&note_out_hash, idx);
            updated_state_hashes.insert(idx, (LeafNodeType::Note, note_out_hash));
        }

        // ? Quote notes
        for note in quote_notes_in {
            let idx = note.get("index").unwrap().as_u64().unwrap();
            // let note_out_hash = BigUint::from_str(note.get("hash").unwrap().as_str().unwrap()).unwrap();
            state_tree.update_leaf_node(&BigUint::zero(), idx);
            updated_state_hashes.insert(idx, (LeafNodeType::Note, BigUint::zero()));
        }
        if !quote_refund_note.is_null() {
            let idx = quote_refund_note.get("index").unwrap().as_u64().unwrap();
            let note_out_hash =
                BigUint::from_str(quote_refund_note.get("hash").unwrap().as_str().unwrap())
                    .unwrap();
            state_tree.update_leaf_node(&note_out_hash, idx);
            updated_state_hashes.insert(idx, (LeafNodeType::Note, note_out_hash));
        }

        // ? Order tab
        let order_tab = transaction.get("prev_order_tab").unwrap();
        let idx: u64 = order_tab.get("tab_idx").unwrap().as_u64().unwrap();
        let tab_hash = transaction
            .get("new_order_tab_hash")
            .unwrap()
            .as_str()
            .unwrap();
        let tab_hash = BigUint::from_str(tab_hash).unwrap();

        state_tree.update_leaf_node(&tab_hash, idx);
        updated_state_hashes.insert(idx, (LeafNodeType::OrderTab, tab_hash));
    } else {
        let collateral_notes_in = transaction
            .get("collateral_notes_in")
            .unwrap()
            .as_array()
            .unwrap();
        let collateral_refund_note = transaction.get("collateral_refund_note").unwrap();

        // ? Base notes
        for note in collateral_notes_in {
            let idx = note.get("index").unwrap().as_u64().unwrap();
            // let note_out_hash = BigUint::from_str(note.get("hash").unwrap().as_str().unwrap()).unwrap();
            state_tree.update_leaf_node(&BigUint::zero(), idx);
            updated_state_hashes.insert(idx, (LeafNodeType::Note, BigUint::zero()));
        }
        if !collateral_refund_note.is_null() {
            let idx = collateral_refund_note
                .get("index")
                .unwrap()
                .as_u64()
                .unwrap();
            let note_out_hash = BigUint::from_str(
                collateral_refund_note
                    .get("hash")
                    .unwrap()
                    .as_str()
                    .unwrap(),
            )
            .unwrap();
            state_tree.update_leaf_node(&note_out_hash, idx);
            updated_state_hashes.insert(idx, (LeafNodeType::Note, note_out_hash));
        }

        // ? Position
        let position = transaction.get("prev_position").unwrap();
        let idx: u64 = position.get("index").unwrap().as_u64().unwrap();
        let pos_hash = transaction
            .get("new_position_hash")
            .unwrap()
            .as_str()
            .unwrap();
        let pos_hash = BigUint::from_str(pos_hash).unwrap();

        state_tree.update_leaf_node(&pos_hash, idx);
        updated_state_hashes.insert(idx, (LeafNodeType::Position, pos_hash));
    }

    let vlp_note_idx = transaction.get("vlp_note_idx").unwrap().as_u64().unwrap();
    let vlp_note_hash = transaction.get("vlp_note_hash").unwrap().as_str().unwrap();
    let vlp_note_hash = BigUint::from_str(vlp_note_hash).unwrap();

    state_tree.update_leaf_node(&vlp_note_hash, vlp_note_idx);
    updated_state_hashes.insert(vlp_note_idx, (LeafNodeType::Note, vlp_note_hash));

    drop(state_tree);
    drop(updated_state_hashes);
}

// * REMOVE LIQUIDITY RESTORE FUNCTIONS ================================================================================

pub fn restore_remove_liquidity(
    tree_m: &Arc<Mutex<SuperficialTree>>,
    updated_state_hashes_m: &Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
    transaction: &Map<String, Value>,
) {
    let is_order_tab = transaction.get("is_order_tab").unwrap().as_bool().unwrap();

    let vlp_notes_in = transaction.get("vlp_notes_in").unwrap().as_array().unwrap();

    let mut state_tree = tree_m.lock();
    let mut updated_state_hashes = updated_state_hashes_m.lock();

    // ? vlp notes
    for note in vlp_notes_in {
        let idx = note.get("index").unwrap().as_u64().unwrap();
        state_tree.update_leaf_node(&BigUint::zero(), idx);
        updated_state_hashes.insert(idx, (LeafNodeType::Note, BigUint::zero()));
    }

    if is_order_tab {
        // ? Base return note
        let idx: u64 = transaction
            .get("base_return_note_index")
            .unwrap()
            .as_u64()
            .unwrap();
        let hash = transaction
            .get("base_return_note_hash")
            .unwrap()
            .as_str()
            .unwrap();
        let hash = BigUint::from_str(hash).unwrap();

        state_tree.update_leaf_node(&hash, idx);
        updated_state_hashes.insert(idx, (LeafNodeType::Note, hash));

        // ? Quote return note
        let idx: u64 = transaction
            .get("quote_return_note_index")
            .unwrap()
            .as_u64()
            .unwrap();
        let hash = transaction
            .get("quote_return_note_hash")
            .unwrap()
            .as_str()
            .unwrap();
        let hash = BigUint::from_str(hash).unwrap();

        state_tree.update_leaf_node(&hash, idx);
        updated_state_hashes.insert(idx, (LeafNodeType::Note, hash));

        // ? Order tab
        let order_tab = transaction.get("prev_order_tab").unwrap();
        let idx: u64 = order_tab.get("tab_idx").unwrap().as_u64().unwrap();
        let tab_hash = transaction.get("new_order_tab_hash").unwrap();
        let tab_hash = if tab_hash.is_null() {
            BigUint::zero()
        } else {
            BigUint::from_str(tab_hash.as_str().unwrap()).unwrap()
        };

        state_tree.update_leaf_node(&tab_hash, idx);
        updated_state_hashes.insert(idx, (LeafNodeType::OrderTab, tab_hash));
    } else {
        // ? Collateral return note
        let idx: u64 = transaction
            .get("collateral_return_note_index")
            .unwrap()
            .as_u64()
            .unwrap();
        let hash = transaction
            .get("collateral_return_note_hash")
            .unwrap()
            .as_str()
            .unwrap();
        let hash = BigUint::from_str(hash).unwrap();

        state_tree.update_leaf_node(&hash, idx);
        updated_state_hashes.insert(idx, (LeafNodeType::Note, hash));

        // ? Position
        let position = transaction.get("prev_position").unwrap();
        let idx: u64 = position.get("index").unwrap().as_u64().unwrap();
        let pos_hash = transaction.get("new_position_hash").unwrap();
        let pos_hash = if pos_hash.is_null() {
            BigUint::zero()
        } else {
            BigUint::from_str(pos_hash.as_str().unwrap()).unwrap()
        };

        state_tree.update_leaf_node(&pos_hash, idx);
        updated_state_hashes.insert(idx, (LeafNodeType::Position, pos_hash));
    }

    drop(state_tree);
    drop(updated_state_hashes);
}
