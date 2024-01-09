use num_bigint::BigUint;
use num_traits::Zero;
use parking_lot::Mutex;
use serde_json::{Map, Value};
use std::{collections::HashMap, str::FromStr, sync::Arc};

use crate::{
    transaction_batch::LeafNodeType, trees::superficial_tree::SuperficialTree, utils::notes::Note,
};

pub fn restore_perp_order_execution(
    tree_m: &Arc<Mutex<SuperficialTree>>,
    updated_state_hashes_m: &Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
    // perpetual_state_tree_m: &Arc<Mutex<SuperficialTree>>,
    // perpetual_updated_position_hashes_m: &Arc<Mutex<HashMap<u64, BigUint>>>,
    perpetual_partial_fill_tracker_m: &Arc<Mutex<HashMap<u64, (Option<Note>, u64, u64)>>>,
    transaction: &Map<String, Value>,
    is_a: bool,
) {
    let order = transaction
        .get(if is_a { "order_a" } else { "order_b" })
        .unwrap();

    match order.get("position_effect_type").unwrap().as_str().unwrap() {
        "Open" => {
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

                let notes_in = order
                    .get("open_order_fields")
                    .unwrap()
                    .get("notes_in")
                    .unwrap()
                    .as_array()
                    .unwrap();
                let refund_note = order.get("open_order_fields").unwrap().get("refund_note");

                restore_after_perp_swap_first_fill(
                    tree_m,
                    updated_state_hashes_m,
                    perpetual_partial_fill_tracker_m,
                    order.get("order_id").unwrap().as_u64().unwrap(),
                    notes_in,
                    refund_note,
                    &transaction
                        .get("indexes")
                        .unwrap()
                        .get(if is_a { "order_a" } else { "order_b" })
                        .unwrap()
                        .get("new_pfr_idx"),
                    &transaction.get(if is_a {
                        "new_pfr_note_hash_a"
                    } else {
                        "new_pfr_note_hash_b"
                    }),
                )
            } else {
                restore_after_perp_swap_later_fills(
                    tree_m,
                    updated_state_hashes_m,
                    perpetual_partial_fill_tracker_m,
                    order.get("order_id").unwrap().as_u64().unwrap(),
                    transaction
                        .get(if is_a {
                            "prev_pfr_note_a"
                        } else {
                            "prev_pfr_note_b"
                        })
                        .unwrap()
                        .get("index")
                        .unwrap()
                        .as_u64()
                        .unwrap(),
                    &transaction
                        .get("indexes")
                        .unwrap()
                        .get(if is_a { "order_a" } else { "order_b" })
                        .unwrap()
                        .get("new_pfr_idx"),
                    &transaction.get(if is_a {
                        "new_pfr_note_hash_a"
                    } else {
                        "new_pfr_note_hash_b"
                    }),
                )
            }
        }
        "Close" => {
            // ? Close position
            restore_return_collateral_note(
                tree_m,
                updated_state_hashes_m,
                &transaction
                    .get("indexes")
                    .unwrap()
                    .get(if is_a { "order_a" } else { "order_b" })
                    .unwrap()
                    .get("return_collateral_idx")
                    .unwrap(),
                &transaction
                    .get(if is_a {
                        "return_collateral_hash_a"
                    } else {
                        "return_collateral_hash_b"
                    })
                    .unwrap(),
            );
        }
        "Liquidate" => {

            // TODO: IMPLEMENT THIS FOR LIQUIDATIONS
        }
        _ => {}
    }

    restore_perpetual_state(
        tree_m,
        updated_state_hashes_m,
        &transaction
            .get("indexes")
            .unwrap()
            .get(if is_a { "order_a" } else { "order_b" })
            .unwrap()
            .get("position_idx"),
        transaction.get(if is_a {
            "new_position_hash_a"
        } else {
            "new_position_hash_b"
        }),
    );
}

// * ======
// * =========
// * ======

pub fn restore_liquidation_order_execution(
    tree_m: &Arc<Mutex<SuperficialTree>>,
    updated_state_hashes_m: &Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
    transaction: &Map<String, Value>,
) {
    let liquidation_order = transaction.get("liquidation_order").unwrap();

    let mut tree = tree_m.lock();
    let mut updated_state_hashes = updated_state_hashes_m.lock();

    let open_order_fields = liquidation_order.get("open_order_fields").unwrap();

    let notes_in = open_order_fields
        .get("notes_in")
        .unwrap()
        .as_array()
        .unwrap();
    let refund_note = open_order_fields.get("refund_note");

    let refund_idx = notes_in[0].get("index").unwrap().as_u64().unwrap();
    let refund_note_hash = if refund_note.unwrap().is_null() {
        BigUint::zero()
    } else {
        BigUint::from_str(refund_note.unwrap().get("hash").unwrap().as_str().unwrap()).unwrap()
    };

    tree.update_leaf_node(&refund_note_hash, refund_idx);
    updated_state_hashes.insert(refund_idx, (LeafNodeType::Note, refund_note_hash));

    // ========

    for i in 1..notes_in.len() {
        let idx = notes_in[i].get("index").unwrap().as_u64().unwrap();

        tree.update_leaf_node(&BigUint::zero(), idx);
        updated_state_hashes.insert(idx, (LeafNodeType::Note, BigUint::zero()));
    }

    // & Update Perpetual State Tree

    let new_position_idx = transaction
        .get("indexes")
        .unwrap()
        .get("new_position_index")
        .unwrap()
        .as_u64()
        .unwrap();
    let new_liquidated_position_idx = transaction
        .get("prev_liquidated_position")
        .unwrap()
        .get("index")
        .unwrap()
        .as_u64()
        .unwrap();

    let new_position_hash = transaction
        .get("new_position_hash")
        .unwrap()
        .as_str()
        .unwrap();
    let new_liquidated_position_hash = transaction
        .get("new_liquidated_position_hash")
        .unwrap()
        .as_str()
        .unwrap();

    tree.update_leaf_node(
        &BigUint::from_str(new_position_hash).unwrap(),
        new_position_idx,
    );
    updated_state_hashes.insert(
        new_position_idx,
        (
            LeafNodeType::Position,
            BigUint::from_str(new_position_hash).unwrap(),
        ),
    );

    let hash = BigUint::from_str(new_liquidated_position_hash).unwrap();
    if hash != BigUint::zero() {
        tree.update_leaf_node(&hash, new_liquidated_position_idx);
        updated_state_hashes.insert(new_liquidated_position_idx, (LeafNodeType::Position, hash));
    }
}

// * =============================================================================================================
// * PERP STATE RESTORE FUNCTIONS ================================================================================

fn restore_after_perp_swap_first_fill(
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

fn restore_after_perp_swap_later_fills(
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

fn restore_return_collateral_note(
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