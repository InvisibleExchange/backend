use num_bigint::{BigInt, BigUint};
use num_traits::Zero;
use parking_lot::Mutex;
use serde_json::{Map, Value};
use std::{collections::HashMap, str::FromStr, sync::Arc};

use crate::{
    order_tab::{OrderTab, TabHeader},
    perpetual::{COLLATERAL_TOKEN, DUST_AMOUNT_PER_ASSET},
    transaction_batch::LeafNodeType,
    trees::superficial_tree::SuperficialTree,
    utils::crypto_utils::EcPoint,
    utils::notes::Note,
};

// * HELPER FUNCTIONS ============================================================================================

pub fn rebuild_swap_note(transaction: &Map<String, Value>, is_a: bool) -> Note {
    let order_indexes_json = transaction
        .get("indexes")
        .unwrap()
        .get(if is_a { "order_a" } else { "order_b" })
        .unwrap();

    let swap_idx = order_indexes_json
        .get("swap_note_idx")
        .unwrap()
        .as_u64()
        .unwrap();

    let order_json: &Value = transaction
        .get("swap_data")
        .unwrap()
        .get(if is_a { "order_a" } else { "order_b" })
        .unwrap();
    let spot_note_info = order_json.get("spot_note_info").unwrap();
    let dest_received_address = spot_note_info.get("dest_received_address").unwrap();
    let address = EcPoint {
        x: BigInt::from_str(dest_received_address.get("x").unwrap().as_str().unwrap()).unwrap(),
        y: BigInt::from_str(dest_received_address.get("y").unwrap().as_str().unwrap()).unwrap(),
    };

    let dest_received_blinding = BigUint::from_str(
        spot_note_info
            .get("dest_received_blinding")
            .unwrap()
            .as_str()
            .unwrap(),
    )
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

    let token_received = order_json.get("token_received").unwrap().as_u64().unwrap();

    return Note::new(
        swap_idx,
        address,
        token_received as u32,
        spent_amount_y - fee_taken_x,
        dest_received_blinding,
    );
}

pub fn restore_partial_fill_refund_note(
    transaction: &Map<String, Value>,
    is_a: bool,
) -> Option<Note> {
    let order = transaction
        .get("swap_data")
        .unwrap()
        .get(if is_a { "order_a" } else { "order_b" })
        .unwrap();

    let prev_pfr_note = transaction.get(if is_a {
        "prev_pfr_note_a"
    } else {
        "prev_pfr_note_b"
    });

    let new_partial_refund_amount = if !prev_pfr_note.unwrap().is_null() {
        prev_pfr_note
            .unwrap()
            .get("amount")
            .unwrap()
            .as_u64()
            .unwrap()
            - transaction
                .get("swap_data")
                .unwrap()
                .get(if is_a {
                    "spent_amount_a"
                } else {
                    "spent_amount_b"
                })
                .unwrap()
                .as_u64()
                .unwrap()
    } else {
        order.get("amount_spent").unwrap().as_u64().unwrap()
            - transaction
                .get("swap_data")
                .unwrap()
                .get(if is_a {
                    "spent_amount_a"
                } else {
                    "spent_amount_b"
                })
                .unwrap()
                .as_u64()
                .unwrap()
    };

    if new_partial_refund_amount
        <= DUST_AMOUNT_PER_ASSET[&order
            .get("token_spent")
            .unwrap()
            .as_u64()
            .unwrap()
            .to_string()]
    {
        return None;
    }

    let idx = transaction
        .get("indexes")
        .unwrap()
        .get(if is_a { "order_a" } else { "order_b" })
        .unwrap()
        .get("partial_fill_idx")
        .unwrap()
        .as_u64()
        .unwrap();

    let spot_note_info = &order.get("spot_note_info").unwrap();
    let note0 = &spot_note_info.get("notes_in").unwrap().as_array().unwrap()[0];

    return Some(Note::new(
        idx,
        EcPoint::new(
            &BigUint::from_str(
                note0
                    .get("address")
                    .unwrap()
                    .get("x")
                    .unwrap()
                    .as_str()
                    .unwrap(),
            )
            .unwrap(),
            &BigUint::from_str(
                note0
                    .get("address")
                    .unwrap()
                    .get("y")
                    .unwrap()
                    .as_str()
                    .unwrap(),
            )
            .unwrap(),
        ),
        order.get("token_spent").unwrap().as_u64().unwrap() as u32,
        new_partial_refund_amount,
        BigUint::from_str(note0.get("blinding").unwrap().as_str().unwrap()).unwrap(),
    ));
}

// * ORDER TABS * //

pub fn order_tab_from_json(tab_json: &Value) -> OrderTab {
    let tab_header = TabHeader::new(
        tab_json
            .get("tab_header")
            .unwrap()
            .get("base_token")
            .unwrap()
            .as_u64()
            .unwrap() as u32,
        tab_json
            .get("tab_header")
            .unwrap()
            .get("quote_token")
            .unwrap()
            .as_u64()
            .unwrap() as u32,
        BigUint::from_str(
            tab_json
                .get("tab_header")
                .unwrap()
                .get("base_blinding")
                .unwrap()
                .as_str()
                .unwrap(),
        )
        .unwrap(),
        BigUint::from_str(
            tab_json
                .get("tab_header")
                .unwrap()
                .get("quotee_blinding")
                .unwrap()
                .as_str()
                .unwrap(),
        )
        .unwrap(),
        BigUint::from_str(
            tab_json
                .get("tab_header")
                .unwrap()
                .get("pub_key")
                .unwrap()
                .as_str()
                .unwrap(),
        )
        .unwrap(),
    );

    return OrderTab::new(
        tab_header,
        tab_json.get("base_amount").unwrap().as_u64().unwrap(),
        tab_json.get("quote_amount").unwrap().as_u64().unwrap(),
    );
}

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
