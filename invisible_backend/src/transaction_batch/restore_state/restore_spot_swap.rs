use num_bigint::BigUint;
use num_traits::Zero;
use parking_lot::Mutex;
use serde_json::{Map, Value};
use std::{collections::HashMap, str::FromStr, sync::Arc};

use crate::{
    order_tab::OrderTab, transaction_batch::LeafNodeType, trees::superficial_tree::SuperficialTree,
    utils::notes::Note,
};

use super::helpers::{order_tab_from_json, rebuild_swap_note, restore_partial_fill_refund_note};

pub fn restore_spot_order_execution(
    tree_m: &Arc<Mutex<SuperficialTree>>,
    updated_state_hashes_m: &Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
    transaction: &Map<String, Value>,
    is_a: bool,
) {
    let is_tab_order = transaction
        .get(if is_a {
            "is_tab_order_a"
        } else {
            "is_tab_order_b"
        })
        .unwrap()
        .as_bool()
        .unwrap();

    if is_tab_order {
        let order_tab = transaction
            .get(if is_a {
                "prev_order_tab_a"
            } else {
                "prev_order_tab_b"
            })
            .unwrap();
        let tab_idx = order_tab.get("tab_idx").unwrap().as_u64().unwrap();

        let mut state_tree_m = tree_m.lock();
        let mut updated_state_hashes = updated_state_hashes_m.lock();

        let updated_tab_hash = transaction
            .get(if is_a {
                "updated_tab_hash_a"
            } else {
                "updated_tab_hash_b"
            })
            .unwrap()
            .as_str()
            .unwrap();
        let updated_tab_hash = BigUint::from_str(updated_tab_hash).unwrap();

        // TODO: TESTING

        state_tree_m.update_leaf_node(&updated_tab_hash, tab_idx);
        updated_state_hashes.insert(tab_idx, (LeafNodeType::OrderTab, updated_tab_hash));

        //
    } else {
        let swap_note = rebuild_swap_note(&transaction, is_a);
        let pfr_note = restore_partial_fill_refund_note(&transaction, is_a);

        if transaction
            .get(if is_a {
                "prev_pfr_note_a"
            } else {
                "prev_pfr_note_b"
            })
            .unwrap()
            .is_null()
        {
            // ? First fill
            let order = transaction
                .get("swap_data")
                .unwrap()
                .get(if is_a { "order_a" } else { "order_b" })
                .unwrap();
            let spont_note_info = order.get("spot_note_info").unwrap();
            let notes_in = spont_note_info.get("notes_in").unwrap().as_array().unwrap();
            let refund_note = spont_note_info.get("refund_note");

            restore_after_swap_first_fill(
                tree_m,
                updated_state_hashes_m,
                &notes_in,
                refund_note,
                swap_note,
                pfr_note,
            );
        } else {
            // ? Second fill

            restore_after_swap_later_fills(tree_m, updated_state_hashes_m, swap_note, pfr_note);
        }
    }
}

// * ======
// * =========
// * ======

// * SPOT STATE RESTORE FUNCTIONS ================================================================================

fn restore_after_swap_first_fill(
    tree_m: &Arc<Mutex<SuperficialTree>>,
    updated_state_hashes_m: &Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
    notes_in: &Vec<Value>,
    refund_note: Option<&Value>,
    swap_note: Note,
    partial_fill_refund_note: Option<Note>,
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

    let swap_idx = swap_note.index;
    let swap_hash = swap_note.hash;
    tree.update_leaf_node(&swap_hash, swap_idx);
    updated_state_hashes.insert(swap_idx, (LeafNodeType::Note, swap_hash));

    if partial_fill_refund_note.is_some() {
        //

        let idx: u64 = partial_fill_refund_note.as_ref().unwrap().index;
        let hash = partial_fill_refund_note.unwrap().hash;

        tree.update_leaf_node(&hash, idx);
        updated_state_hashes.insert(idx, (LeafNodeType::Note, hash));
        //
    } else if notes_in.len() > 2 {
        //
        let idx = notes_in[2].get("index").unwrap().as_u64().unwrap();

        tree.update_leaf_node(&BigUint::zero(), idx);
        updated_state_hashes.insert(idx, (LeafNodeType::Note, BigUint::zero()));
        //
    }

    for i in 3..notes_in.len() {
        let idx = notes_in[i].get("index").unwrap().as_u64().unwrap();

        tree.update_leaf_node(&BigUint::zero(), idx);
        updated_state_hashes.insert(idx, (LeafNodeType::Note, BigUint::zero()));
    }

    drop(tree);
    drop(updated_state_hashes);
}

fn restore_after_swap_later_fills(
    tree_m: &Arc<Mutex<SuperficialTree>>,
    updated_state_hashes_m: &Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
    swap_note: Note,
    partial_fill_refund_note: Option<Note>,
) {
    let mut tree = tree_m.lock();
    let mut updated_state_hashes = updated_state_hashes_m.lock();

    // ? Update the state tree
    let swap_idx = swap_note.index;
    let swap_hash = swap_note.hash;
    tree.update_leaf_node(&swap_hash, swap_idx);
    updated_state_hashes.insert(swap_idx, (LeafNodeType::Note, swap_hash));

    if partial_fill_refund_note.is_some() {
        let idx: u64 = partial_fill_refund_note.as_ref().unwrap().index;
        let hash = partial_fill_refund_note.unwrap().hash;

        tree.update_leaf_node(&hash, idx);
        updated_state_hashes.insert(idx, (LeafNodeType::Note, hash));
    }

    drop(updated_state_hashes);
    drop(tree);
}

// * Order tabs ** //
fn get_updated_order_tab(transaction: &Map<String, Value>, is_a: bool) -> OrderTab {
    // ? Get the info --------------------------

    let tab_json = transaction
        .get(if is_a {
            "prev_order_tab_a"
        } else {
            "prev_order_tab_b"
        })
        .unwrap();

    let order_json: &Value = transaction
        .get("swap_data")
        .unwrap()
        .get(if is_a { "order_a" } else { "order_b" })
        .unwrap();
    let token_received = order_json.get("token_received").unwrap().as_u64().unwrap() as u32;

    let spent_amount_x = transaction
        .get("swap_data")
        .unwrap()
        .get(if is_a {
            "spent_amount_a"
        } else {
            "spent_amount_b"
        })
        .unwrap()
        .as_u64()
        .unwrap();

    let spent_amount_y = transaction
        .get("swap_data")
        .unwrap()
        .get(if is_a {
            "spent_amount_b"
        } else {
            "spent_amount_a"
        })
        .unwrap()
        .as_u64()
        .unwrap();

    let fee_taken_x = transaction
        .get("swap_data")
        .unwrap()
        .get(if is_a { "fee_taken_a" } else { "fee_taken_b" })
        .unwrap()
        .as_u64()
        .unwrap();

    // ? Make the update

    let mut order_tab = order_tab_from_json(tab_json);

    let is_buy = order_tab.tab_header.base_token == token_received;

    if is_buy {
        order_tab.quote_amount -= spent_amount_x;
        order_tab.base_amount += spent_amount_y - fee_taken_x;
    } else {
        order_tab.base_amount -= spent_amount_x;
        order_tab.quote_amount += spent_amount_y - fee_taken_x;
    }

    order_tab.update_hash();

    return order_tab;
}

// * =========================================================================================================================
// *  DEPOSITS/ WITHDRAWALS RESTORE FUNCTIONS ================================================================================

pub fn restore_deposit_update(
    tree_m: &Arc<Mutex<SuperficialTree>>,
    updated_state_hashes_m: &Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
    notes: &Vec<Value>,
) {
    // ? Upadte the state by adding the note hashes to the merkle tree

    let mut tree = tree_m.lock();
    let mut updated_state_hashes = updated_state_hashes_m.lock();

    for note in notes.iter() {
        let idx = note.get("index").unwrap().as_u64().unwrap();
        let hash = BigUint::from_str(note.get("hash").unwrap().as_str().unwrap()).unwrap();

        tree.update_leaf_node(&hash, idx);
        updated_state_hashes.insert(idx, (LeafNodeType::Note, hash));
    }
    drop(tree);
}

pub fn restore_withdrawal_update(
    tree_m: &Arc<Mutex<SuperficialTree>>,
    updated_state_hashes_m: &Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
    notes_in: &Vec<Value>,
    refund_note: Option<&Value>,
) {
    // ? Upadte the state by adding the note hashes to the merkle tree

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

    for note in notes_in.iter().skip(1) {
        let idx = note.get("index").unwrap().as_u64().unwrap();

        tree.update_leaf_node(&BigUint::zero(), idx);
        updated_state_hashes.insert(idx, (LeafNodeType::Note, BigUint::zero()));
    }
    drop(tree);
}
