use num_bigint::{BigInt, BigUint};
use num_traits::Zero;
use parking_lot::Mutex;
use serde_json::{Map, Value};
use std::{collections::HashMap, str::FromStr, sync::Arc};

use crate::{
    perpetual::{perp_position::PerpPosition, COLLATERAL_TOKEN},
    transaction_batch::LeafNodeType,
    trees::superficial_tree::SuperficialTree,
    utils::crypto_utils::EcPoint,
    utils::notes::Note,
};

use super::perp_helpers::position_from_json;

// * UPDATE MARGIN RESTORE FUNCTIONS ================================================================================

pub fn restore_margin_update(
    tree_m: &Arc<Mutex<SuperficialTree>>,
    updated_state_hashes_m: &Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
    transaction: &Map<String, Value>,
) {
    let mut tree = tree_m.lock();
    let mut updated_state_hashes = updated_state_hashes_m.lock();

    let pos_index = transaction
        .get("margin_change")
        .unwrap()
        .get("position")
        .unwrap()
        .get("index")
        .unwrap()
        .as_u64()
        .unwrap();
    let new_position_hash = BigUint::from_str(
        transaction
            .get("new_position_hash")
            .unwrap()
            .as_str()
            .unwrap(),
    )
    .unwrap();

    // TODO =======================================================================================================================================
    let margin_change = transaction.get("margin_change").unwrap();

    let prev_position = margin_change.get("position").unwrap();
    let mut position = position_from_json(prev_position);

    let change_amount = margin_change
        .get("margin_change")
        .unwrap()
        .as_i64()
        .unwrap();

    position.modify_margin(change_amount).unwrap();

    //TODO: CHECK IF CORRECT

    // TODO =======================================================================================================================================

    if !transaction
        .get("margin_change")
        .unwrap()
        .get("notes_in")
        .unwrap()
        .is_null()
    {
        // * Adding margin ---- ---- ---- ----

        let notes_in = transaction
            .get("margin_change")
            .unwrap()
            .get("notes_in")
            .unwrap()
            .as_array()
            .unwrap();
        let refund_note = transaction.get("margin_change").unwrap().get("refund_note");

        let refund_idx: u64;
        let refund_note_hash: BigUint;
        if !refund_note.unwrap().is_null() {
            refund_idx = refund_note.unwrap().get("index").unwrap().as_u64().unwrap();
            refund_note_hash =
                BigUint::from_str(refund_note.unwrap().get("hash").unwrap().as_str().unwrap())
                    .unwrap();
        } else {
            refund_idx = notes_in[0].get("index").unwrap().as_u64().unwrap();
            refund_note_hash = BigUint::zero();
        };

        tree.update_leaf_node(&refund_note_hash, refund_idx);
        updated_state_hashes.insert(refund_idx, (LeafNodeType::Note, refund_note_hash));

        for note in notes_in.iter().skip(1) {
            let idx = note.get("index").unwrap().as_u64().unwrap();
            let note_hash = BigUint::from_str(note.get("hash").unwrap().as_str().unwrap()).unwrap();

            tree.update_leaf_node(&note_hash, idx);
            updated_state_hashes.insert(idx, (LeafNodeType::Note, note_hash));
        }

        // ? Update the position state tree
        tree.update_leaf_node(&new_position_hash, pos_index);
        updated_state_hashes.insert(pos_index, (LeafNodeType::Position, new_position_hash));

        drop(tree);
        drop(updated_state_hashes);
    } else {
        // * Removing margin ---- ---- ---- ----

        let return_collateral_note = rebuild_return_collateral_note(transaction);

        tree.update_leaf_node(&return_collateral_note.hash, return_collateral_note.index);
        updated_state_hashes.insert(
            return_collateral_note.index,
            (LeafNodeType::Note, return_collateral_note.hash),
        );

        // ? Update the position state tree
        tree.update_leaf_node(&new_position_hash, pos_index);
        updated_state_hashes.insert(pos_index, (LeafNodeType::Position, new_position_hash));

        drop(tree);
        drop(updated_state_hashes);
    }
}

fn rebuild_return_collateral_note(transaction: &Map<String, Value>) -> Note {
    let index = transaction.get("zero_idx").unwrap().as_u64().unwrap();
    let addr = EcPoint {
        x: BigInt::from_str(
            transaction
                .get("margin_change")
                .unwrap()
                .get("close_order_fields")
                .unwrap()
                .get("dest_received_address")
                .unwrap()
                .get("x")
                .unwrap()
                .as_str()
                .unwrap(),
        )
        .unwrap(),
        y: BigInt::from_str(
            transaction
                .get("margin_change")
                .unwrap()
                .get("close_order_fields")
                .unwrap()
                .get("dest_received_address")
                .unwrap()
                .get("y")
                .unwrap()
                .as_str()
                .unwrap(),
        )
        .unwrap(),
    };
    let amount = transaction
        .get("margin_change")
        .unwrap()
        .get("margin_change")
        .unwrap()
        .as_i64()
        .unwrap()
        .abs() as u64;
    let blinding = BigUint::from_str(
        transaction
            .get("margin_change")
            .unwrap()
            .get("close_order_fields")
            .unwrap()
            .get("dest_received_blinding")
            .unwrap()
            .as_str()
            .unwrap(),
    )
    .unwrap();

    Note::new(index, addr, COLLATERAL_TOKEN, amount, blinding)
}

// * SPLIT NOTES RESTORE FUNCTIONS ================================================================================

pub fn restore_note_split(
    tree_m: &Arc<Mutex<SuperficialTree>>,
    updated_state_hashes_m: &Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
    transaction: &Map<String, Value>,
) {
    let mut state_tree = tree_m.lock();
    let mut updated_state_hashes = updated_state_hashes_m.lock();

    let notes_in = transaction
        .get("note_split")
        .unwrap()
        .get("notes_in")
        .unwrap()
        .as_array()
        .unwrap();
    let new_note = transaction
        .get("note_split")
        .unwrap()
        .get("new_note")
        .unwrap();
    let refund_note = transaction
        .get("note_split")
        .unwrap()
        .get("refund_note")
        .unwrap();

    // ? Remove notes in from state
    for note in notes_in.iter() {
        let idx = note.get("index").unwrap().as_u64().unwrap();

        state_tree.update_leaf_node(&BigUint::zero(), idx);
        updated_state_hashes.insert(idx, (LeafNodeType::Note, BigUint::zero()));
    }

    // ? Add return in to state
    let new_note_index = new_note.get("index").unwrap().as_u64().unwrap();
    let new_note_hash = BigUint::from_str(new_note.get("hash").unwrap().as_str().unwrap()).unwrap();
    state_tree.update_leaf_node(&new_note_hash, new_note_index);
    updated_state_hashes.insert(new_note_index, (LeafNodeType::Note, new_note_hash));

    if !refund_note.is_null() {
        let refund_note_index = refund_note.get("index").unwrap().as_u64().unwrap();
        let refund_note_hash =
            BigUint::from_str(refund_note.get("hash").unwrap().as_str().unwrap()).unwrap();

        state_tree.update_leaf_node(&refund_note_hash, refund_note_index);
        updated_state_hashes.insert(refund_note_index, (LeafNodeType::Note, refund_note_hash));
    }

    drop(updated_state_hashes);
    drop(state_tree);
}

// * ONCHAIN MM ACTION ============================================================0

pub fn restore_mm_action(
    transaction: &Map<String, Value>,
    mut position: PerpPosition,
) -> PerpPosition {
    let action_type = transaction.get("action_type").unwrap().as_str().unwrap();

    match action_type {
        "register_mm" => {
            // ? Registering a new position
            let vlp_token = transaction.get("vlp_token").unwrap().as_u64().unwrap() as u32;
            let max_vlp_supply = transaction.get("max_vlp_supply").unwrap().as_u64().unwrap();

            let vlp_amount = position.margin;

            // ? register mm position
            position.position_header.vlp_token = vlp_token;
            position.position_header.max_vlp_supply = max_vlp_supply;

            position.vlp_supply = vlp_amount;

            position.position_header.update_hash();
            position.hash = position.hash_position();

            return position;
        }
        "add_liquidity" => {
            // ? Adding to an existing position

            let initial_value = transaction.get("initial_value").unwrap().as_u64().unwrap();
            let vlp_amount = transaction.get("vlp_amount").unwrap().as_u64().unwrap();

            position.margin += initial_value;
            position.vlp_supply += vlp_amount;
            position.update_position_info();

            return position;
        }
        "remove_liquidity" => {
            // ? Remove from an existing order tab
            let return_collateral_amount = transaction
                .get("return_collateral_amount")
                .unwrap()
                .as_u64()
                .unwrap();
            let vlp_amount = transaction.get("vlp_amount").unwrap().as_u64().unwrap();

            position.margin -= return_collateral_amount;
            position.vlp_supply -= vlp_amount;
            position.update_position_info();

            return position;
        }
        "close_mm_position" => {
            // ? Adding to an existing order tab

            let return_collateral_amount = transaction
                .get("return_collateral_amount")
                .unwrap()
                .as_u64()
                .unwrap();

            position.margin -= return_collateral_amount;
            position.vlp_supply = 0;
            position.update_position_info();

            return position;
        }
        _ => {
            panic!("Invalid onchain mm action type")
        }
    }
}
