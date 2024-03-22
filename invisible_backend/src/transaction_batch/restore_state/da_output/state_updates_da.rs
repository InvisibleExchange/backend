use std::collections::HashMap;

use num_bigint::BigUint;
use serde_json::{Map, Value};

use crate::transaction_batch::{
    restore_state::helpers::perp_helpers::position_from_json, LeafNodeType,
};

use super::{
    super::helpers::{
        spot_helpers::{close_tab, note_from_json, open_new_tab, order_tab_from_json},
        state_helpers::{rebuild_return_collateral_note, restore_mm_action},
    },
    helpers::{append_note_output, append_position_output, append_tab_output},
};

// * STATE UPDATE DA FUNCTIONS =======================================================================================
pub fn margin_update_da_output(
    updated_state_hashes: &HashMap<u64, (LeafNodeType, BigUint)>,
    note_outputs: &mut Vec<(u64, [BigUint; 4])>,
    position_outputs: &mut Vec<(u64, [BigUint; 3])>,
    transaction: &Map<String, Value>,
) {
    let margin_change = transaction.get("margin_change").unwrap();

    let prev_position = margin_change.get("position").unwrap();
    let mut position = position_from_json(prev_position);

    let change_amount = margin_change
        .get("margin_change")
        .unwrap()
        .as_i64()
        .unwrap();

    position.modify_margin(change_amount).unwrap();
    append_position_output(updated_state_hashes, position_outputs, &position);

    if change_amount > 0 {
        let refund_note = margin_change.get("refund_note").unwrap();
        if !refund_note.is_null() {
            append_note_output(
                updated_state_hashes,
                note_outputs,
                &note_from_json(refund_note),
            );
        }
    } else {
        let return_collateral_note = rebuild_return_collateral_note(transaction);
        append_note_output(updated_state_hashes, note_outputs, &return_collateral_note);
    }
}

pub fn note_split_da_output(
    updated_state_hashes: &HashMap<u64, (LeafNodeType, BigUint)>,
    note_outputs: &mut Vec<(u64, [BigUint; 4])>,
    transaction: &Map<String, Value>,
) {
    let new_note = transaction
        .get("note_split")
        .unwrap()
        .get("new_note")
        .unwrap();
    append_note_output(
        updated_state_hashes,
        note_outputs,
        &note_from_json(new_note),
    );

    let refund_note = transaction
        .get("note_split")
        .unwrap()
        .get("refund_note")
        .unwrap();
    if !refund_note.is_null() {
        append_note_output(
            updated_state_hashes,
            note_outputs,
            &note_from_json(refund_note),
        );
    }
}

// * ORDER TABS DA FUNCTIONS ==========================================================================================
pub fn open_order_tab_da_output(
    updated_state_hashes: &HashMap<u64, (LeafNodeType, BigUint)>,
    note_outputs: &mut Vec<(u64, [BigUint; 4])>,
    tab_outputs: &mut Vec<(u64, [BigUint; 4])>,
    transaction: &Map<String, Value>,
) {
    let base_refund_note = transaction.get("base_refund_note").unwrap();
    let quote_refund_note = transaction.get("quote_refund_note").unwrap();

    if !base_refund_note.is_null() {
        append_note_output(
            updated_state_hashes,
            note_outputs,
            &note_from_json(base_refund_note),
        );
    }
    if !quote_refund_note.is_null() {
        append_note_output(
            updated_state_hashes,
            note_outputs,
            &note_from_json(quote_refund_note),
        );
    }

    let new_order_tab = open_new_tab(transaction);
    append_tab_output(updated_state_hashes, tab_outputs, &new_order_tab);
}

pub fn close_order_tab_da_ouput(
    updated_state_hashes: &HashMap<u64, (LeafNodeType, BigUint)>,
    note_outputs: &mut Vec<(u64, [BigUint; 4])>,
    tab_outputs: &mut Vec<(u64, [BigUint; 4])>,
    transaction: &Map<String, Value>,
) {
    let prev_order_tab = transaction.get("order_tab").unwrap();
    let order_tab = order_tab_from_json(prev_order_tab);

    let (base_return_note, quote_return_note, new_order_tab) = close_tab(transaction, order_tab);

    append_note_output(updated_state_hashes, note_outputs, &base_return_note);
    append_note_output(updated_state_hashes, note_outputs, &quote_return_note);

    if let Some(new_order_tab) = new_order_tab {
        append_tab_output(updated_state_hashes, tab_outputs, &new_order_tab);
    }
}

pub fn onchain_mm_action_da_output(
    updated_state_hashes: &HashMap<u64, (LeafNodeType, BigUint)>,
    position_outputs: &mut Vec<(u64, [BigUint; 3])>,
    transaction: &Map<String, Value>,
) {
    let position = transaction.get("prev_position").unwrap();
    let prev_position = position_from_json(position);

    let updated_position = restore_mm_action(transaction, prev_position);
    append_position_output(updated_state_hashes, position_outputs, &updated_position);
}

// * FORCED ESCAPES DA FUNCTIONS =====================================================================================
pub fn forced_position_escape_da_output(
    updated_state_hashes: &HashMap<u64, (LeafNodeType, BigUint)>,
    note_outputs: &mut Vec<(u64, [BigUint; 4])>,
    position_outputs: &mut Vec<(u64, [BigUint; 3])>,
    transaction: &Map<String, Value>,
) {
    let position_escape = transaction.get("position_escape").unwrap();
    let open_order_fields_b = position_escape.get("open_order_fields_b").unwrap();
    if !open_order_fields_b.is_null() {
        let refund_note = open_order_fields_b.get("refund_note").unwrap();
        if !refund_note.is_null() {
            append_note_output(
                updated_state_hashes,
                note_outputs,
                &note_from_json(refund_note),
            );
        }
    }

    let new_position_b = transaction.get("new_position_b").unwrap();
    let new_position_b = position_from_json(new_position_b);

    append_position_output(updated_state_hashes, position_outputs, &new_position_b);
}
