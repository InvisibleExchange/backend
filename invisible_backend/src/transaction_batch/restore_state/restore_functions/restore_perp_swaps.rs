use num_bigint::BigUint;
use num_traits::Zero;
use parking_lot::Mutex;
use serde_json::{Map, Value};
use std::{collections::HashMap, str::FromStr, sync::Arc};

use crate::{
    transaction_batch::LeafNodeType, trees::superficial_tree::SuperficialTree, utils::notes::Note,
};

use super::helpers::{
    perp_helpers::{
        open_pos_after_liquidations, position_from_json, refund_partial_fill,
        return_collateral_on_close, update_liquidated_position, update_position_close,
        update_position_modify, update_position_open,
    },
    perp_state_updates::{
        restore_after_perp_swap_first_fill, restore_after_perp_swap_later_fills,
        restore_perpetual_state, restore_return_collateral_note,
    },
};

pub fn restore_perp_order_execution(
    tree_m: &Arc<Mutex<SuperficialTree>>,
    updated_state_hashes_m: &Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
    perpetual_partial_fill_tracker_m: &Arc<Mutex<HashMap<u64, (Option<Note>, u64, u64)>>>,
    funding_rates: &HashMap<u32, Vec<i64>>,
    funding_prices: &HashMap<u32, Vec<u64>>,
    transaction: &Map<String, Value>,
    is_a: bool,
) {
    let order = transaction
        .get(if is_a { "order_a" } else { "order_b" })
        .unwrap();

    match order.get("position_effect_type").unwrap().as_str().unwrap() {
        "Open" => {
            // TODO =======================================================================================================================================
            let new_pfr_note = refund_partial_fill(transaction, is_a);
            if let Some(new_pfr_note) = new_pfr_note {
                let new_pfr_hash = &transaction
                    .get(if is_a {
                        "new_pfr_note_hash_a"
                    } else {
                        "new_pfr_note_hash_b"
                    })
                    .unwrap()
                    .as_str()
                    .unwrap();
                if new_pfr_hash != &new_pfr_note.hash.to_string() {
                    println!(
                        "PFR NOTE HASH MISMETCH{} {}",
                        new_pfr_hash,
                        new_pfr_note.hash.to_string()
                    );
                }
            }
            // TODO =======================================================================================================================================

            // ======

            // TODO =======================================================================================================================================
            let prev_position = transaction
                .get(if is_a {
                    "prev_position_a"
                } else {
                    "prev_position_b"
                })
                .unwrap();
            let prev_position = if prev_position.is_null() {
                None
            } else {
                Some(position_from_json(prev_position))
            };

            let updated_position = update_position_open(transaction, prev_position, is_a);

            let new_pos_hash = transaction.get(if is_a {
                "new_position_hash_a"
            } else {
                "new_position_hash_b"
            });

            if updated_position.hash.to_string() != new_pos_hash.unwrap().as_str().unwrap() {
                println!(
                    "Perp Order position hash: {} {}",
                    updated_position.hash.to_string(),
                    new_pos_hash.unwrap().as_str().unwrap()
                );
            }
            // TODO =======================================================================================================================================

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

        "Modify" => {
            // TODO =======================================================================================================================================
            let prev_position = transaction
                .get(if is_a {
                    "prev_position_a"
                } else {
                    "prev_position_b"
                })
                .unwrap();
            let prev_position = position_from_json(prev_position);

            let updated_position = update_position_modify(
                transaction,
                prev_position,
                is_a,
                funding_rates,
                funding_prices,
            );

            let new_pos_hash = transaction.get(if is_a {
                "new_position_hash_a"
            } else {
                "new_position_hash_b"
            });

            if updated_position.hash.to_string() != new_pos_hash.unwrap().as_str().unwrap() {
                println!(
                    "MODIFY: {} {}",
                    updated_position.hash.to_string(),
                    new_pos_hash.unwrap().as_str().unwrap()
                );
            }

            // TODO =======================================================================================================================================
        }
        "Close" => {
            // TODO =======================================================================================================================================
            let prev_position = transaction
                .get(if is_a {
                    "prev_position_a"
                } else {
                    "prev_position_b"
                })
                .unwrap();
            let prev_position = position_from_json(prev_position);

            let (collateral_returned, updated_position) = update_position_close(
                transaction,
                prev_position,
                is_a,
                funding_rates,
                funding_prices,
            );

            let collateral_return_note =
                return_collateral_on_close(transaction, is_a, collateral_returned);

            // * ============== * //

            let ret_coll_hash = &transaction
                .get(if is_a {
                    "return_collateral_hash_a"
                } else {
                    "return_collateral_hash_b"
                })
                .unwrap();

            if ret_coll_hash.as_str().unwrap() != collateral_return_note.hash.to_string() {
                println!(
                    "RETURN COLLATERAL NOTE HASH MISMATCH: {} {}",
                    ret_coll_hash.as_str().unwrap(),
                    collateral_return_note.hash.to_string()
                );
            }

            let new_pos_hash = transaction.get(if is_a {
                "new_position_hash_a"
            } else {
                "new_position_hash_b"
            });

            let updated_hash = if let Some(pos) = updated_position {
                pos.hash.to_string()
            } else {
                String::from("0")
            };

            let new_pos_hash_test = if let Some(hash) = new_pos_hash.unwrap().as_str() {
                hash
            } else {
                "0"
            };

            if updated_hash != new_pos_hash_test {
                println!("CLOSE: {} {}", updated_hash, new_pos_hash_test);
            }

            // TODO =======================================================================================================================================

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
    funding_rates: &HashMap<u32, Vec<i64>>,
    funding_prices: &HashMap<u32, Vec<u64>>,
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

    // TODO =======================================================================================================================================

    let liquidated_position = transaction
        .get("liquidation_order")
        .unwrap()
        .get("position")
        .unwrap();
    let liquidated_position = position_from_json(liquidated_position);

    let (liquidated_size, liquidator_fee, liquidated_position) = update_liquidated_position(
        transaction,
        liquidated_position,
        funding_rates,
        funding_prices,
    );

    let new_position = open_pos_after_liquidations(transaction, liquidated_size, liquidator_fee);

    let liq_pos_hash = if let Some(pos) = liquidated_position {
        pos.hash.to_string()
    } else {
        String::from("0")
    };

    if liq_pos_hash != new_liquidated_position_hash {
        println!(
            "LIQUIDATION: {} {}",
            liq_pos_hash, new_liquidated_position_hash
        );
    }

    if new_position.hash.to_string() != new_position_hash {
        println!(
            "LIQUIDATION: {} {}",
            new_position.hash.to_string(),
            new_position_hash
        );
    }

    // TODO =======================================================================================================================================

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

// * ===========================================================================================
// * ===========================================================================================
