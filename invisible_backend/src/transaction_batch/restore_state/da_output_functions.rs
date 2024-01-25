use std::collections::HashMap;

use num_bigint::BigUint;
use num_traits::{FromPrimitive, One, Zero};
use serde_json::{Map, Value};

use crate::{
    order_tab::OrderTab,
    perpetual::{perp_position::PerpPosition, OrderSide},
    transaction_batch::{restore_state::helpers::perp_helpers::position_from_json, LeafNodeType},
    utils::{crypto_utils::hash, notes::Note},
};

use super::helpers::{
    perp_helpers::{
        open_pos_after_liquidations, refund_partial_fill, return_collateral_on_close,
        update_liquidated_position, update_position_close, update_position_modify,
        update_position_open,
    },
    spot_helpers::{
        close_tab, get_updated_order_tab, note_from_json, open_new_tab, order_tab_from_json,
        rebuild_swap_note, restore_partial_fill_refund_note,
    },
    state_helpers::{rebuild_return_collateral_note, restore_mm_action},
};

// * SPOT SWAP DA FUNCTIONS ==========================================================================================
pub fn spot_order_da_output(
    updated_state_hashes: &HashMap<u64, (LeafNodeType, BigUint)>,
    note_outputs: &mut Vec<(u64, [BigUint; 3])>,
    tab_outputs: &mut Vec<(u64, [BigUint; 4])>,
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
        let updated_order_tab = get_updated_order_tab(transaction, is_a);
        append_tab_output(updated_state_hashes, tab_outputs, &updated_order_tab);
    } else {
        let swap_note = rebuild_swap_note(&transaction, is_a);
        append_note_output(updated_state_hashes, note_outputs, &swap_note);

        let pfr_note = restore_partial_fill_refund_note(&transaction, is_a);
        if let Some(pfr_note) = &pfr_note {
            append_note_output(updated_state_hashes, note_outputs, pfr_note);
        }

        let is_first_fill = transaction
            .get(if is_a {
                "prev_pfr_note_a"
            } else {
                "prev_pfr_note_b"
            })
            .unwrap()
            .is_null();

        if is_first_fill {
            let order = transaction
                .get("swap_data")
                .unwrap()
                .get(if is_a { "order_a" } else { "order_b" })
                .unwrap();
            let spont_note_info = order.get("spot_note_info").unwrap();
            let refund_note = spont_note_info.get("refund_note").unwrap();

            if !refund_note.is_null() {
                append_note_output(
                    updated_state_hashes,
                    note_outputs,
                    &note_from_json(refund_note),
                )
            }
        }
    }
}

// * Deposits/Witdrawals * //
pub fn deposit_da_output(
    updated_state_hashes: &HashMap<u64, (LeafNodeType, BigUint)>,
    note_outputs: &mut Vec<(u64, [BigUint; 3])>,
    transaction: &Map<String, Value>,
) {
    let deposit = transaction.get("deposit").unwrap();
    let deposit_notes = deposit.get("notes").unwrap().as_array().unwrap();

    for note in deposit_notes {
        let note = note_from_json(note);
        append_note_output(updated_state_hashes, note_outputs, &note);
    }
}

pub fn withdrawal_da_output(
    updated_state_hashes: &HashMap<u64, (LeafNodeType, BigUint)>,
    note_outputs: &mut Vec<(u64, [BigUint; 3])>,
    transaction: &Map<String, Value>,
) {
    let refund_note = transaction
        .get("withdrawal")
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

// * PERP SWAP DA FUNCTIONS ==========================================================================================
pub fn perp_swap_da_output(
    updated_state_hashes: &HashMap<u64, (LeafNodeType, BigUint)>,
    note_outputs: &mut Vec<(u64, [BigUint; 3])>,
    position_outputs: &mut Vec<(u64, [BigUint; 3])>,
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
            let is_first_fill = transaction
                .get(if is_a {
                    "prev_pfr_note_a"
                } else {
                    "prev_pfr_note_b"
                })
                .unwrap()
                .is_null();

            if is_first_fill {
                let refund_note = order
                    .get("open_order_fields")
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

            let new_pfr_note = refund_partial_fill(transaction, is_a);
            if let Some(pfr_note) = new_pfr_note {
                append_note_output(updated_state_hashes, note_outputs, &pfr_note);
            }

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
            append_position_output(updated_state_hashes, position_outputs, &updated_position);
        }
        "Modify" => {
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
            append_position_output(updated_state_hashes, position_outputs, &updated_position);
        }
        "Close" => {
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

            if let Some(position) = updated_position {
                append_position_output(updated_state_hashes, position_outputs, &position);
            }

            let collateral_return_note =
                return_collateral_on_close(transaction, is_a, collateral_returned);
            append_note_output(updated_state_hashes, note_outputs, &collateral_return_note);
        }
        _ => {}
    }
}

// * Liquidations * //
pub fn liquidations_da_output(
    updated_state_hashes: &HashMap<u64, (LeafNodeType, BigUint)>,
    note_outputs: &mut Vec<(u64, [BigUint; 3])>,
    position_outputs: &mut Vec<(u64, [BigUint; 3])>,
    funding_rates: &HashMap<u32, Vec<i64>>,
    funding_prices: &HashMap<u32, Vec<u64>>,
    transaction: &Map<String, Value>,
) {
    let liquidation_order = transaction.get("liquidation_order").unwrap();
    let open_order_fields = liquidation_order.get("open_order_fields").unwrap();

    let refund_note = open_order_fields.get("refund_note").unwrap();
    if !refund_note.is_null() {
        append_note_output(
            updated_state_hashes,
            note_outputs,
            &note_from_json(refund_note),
        );
    }

    let liquidated_position = liquidation_order.get("position").unwrap();
    let liquidated_position = position_from_json(liquidated_position);

    let (liquidated_size, liquidator_fee, liquidated_position) = update_liquidated_position(
        transaction,
        liquidated_position,
        funding_rates,
        funding_prices,
    );

    if let Some(position) = liquidated_position {
        append_position_output(updated_state_hashes, position_outputs, &position);
    }

    let new_position = open_pos_after_liquidations(transaction, liquidated_size, liquidator_fee);
    append_position_output(updated_state_hashes, position_outputs, &new_position);
}

// * STATE UPDATE DA FUNCTIONS =======================================================================================
pub fn margin_update_da_output(
    updated_state_hashes: &HashMap<u64, (LeafNodeType, BigUint)>,
    note_outputs: &mut Vec<(u64, [BigUint; 3])>,
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
    note_outputs: &mut Vec<(u64, [BigUint; 3])>,
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
    note_outputs: &mut Vec<(u64, [BigUint; 3])>,
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
    note_outputs: &mut Vec<(u64, [BigUint; 3])>,
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
    note_outputs: &mut Vec<(u64, [BigUint; 3])>,
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

// & ==================================================================================================================
// & HELPERS ==========================================================================================================

/// Check if the note is part of updated_state_hashes and if
/// it is, then parse and append it to note_outputs.
fn append_note_output(
    updated_state_hashes: &HashMap<u64, (LeafNodeType, BigUint)>,
    note_outputs: &mut Vec<(u64, [BigUint; 3])>,
    note: &Note,
) {
    let (leaf_type, leaf_hash) = updated_state_hashes.get(&note.index).unwrap();

    if leaf_type != &LeafNodeType::Note || leaf_hash != &note.hash {
        return;
    }

    let (index, output) = _get_note_output(note);
    note_outputs.push((index, output));
}

fn _get_note_output(note: &Note) -> (u64, [BigUint; 3]) {
    let hidden_amount = BigUint::from_u64(note.amount).unwrap()
        ^ &note.blinding % BigUint::from_u64(2).unwrap().pow(64);

    // & batched_note_info format: | token (32 bits) | hidden amount (64 bits) | idx (64 bits) |
    let batched_note_info = BigUint::from_u32(note.token).unwrap() << 128
        | hidden_amount << 64
        | BigUint::from_u64(note.index).unwrap();

    let commitment = hash(&BigUint::from_u64(note.amount).unwrap(), &note.blinding);

    return (
        note.index,
        [
            batched_note_info,
            commitment,
            note.address.x.to_biguint().unwrap(),
        ],
    );
}

// * ———————————————————————————————————————————————————————————————————————————————————— * //

/// Check if the position is part of updated_state_hashes and if
/// it is, then parse and append it to position_outputs.
fn append_position_output(
    updated_state_hashes: &HashMap<u64, (LeafNodeType, BigUint)>,
    position_outputs: &mut Vec<(u64, [BigUint; 3])>,
    position: &PerpPosition,
) {
    let (leaf_type, leaf_hash) = updated_state_hashes.get(&(position.index as u64)).unwrap();
    if leaf_type != &LeafNodeType::Position || leaf_hash != &position.hash {
        return;
    }

    let (index, output) = _get_position_output(position);
    position_outputs.push((index, output));
}

fn _get_position_output(position: &PerpPosition) -> (u64, [BigUint; 3]) {
    // & | index (64 bits) | synthetic_token (32 bits) | position_size (64 bits) | vlp_token (32 bits) |
    let batched_position_info_slot1 = BigUint::from_u32(position.index).unwrap() << 128
        | BigUint::from_u32(position.position_header.synthetic_token).unwrap() << 96
        | BigUint::from_u64(position.position_size).unwrap() << 32
        | BigUint::from_u32(position.position_header.vlp_token).unwrap();

    // & format: | entry_price (64 bits) | margin (64 bits) | vlp_supply (64 bits) | last_funding_idx (32 bits) | order_side (1 bits) | allow_partial_liquidations (1 bits) |
    let batched_position_info_slot2 = BigUint::from_u64(position.entry_price).unwrap() << 162
        | BigUint::from_u64(position.margin).unwrap() << 98
        | BigUint::from_u64(position.vlp_supply).unwrap() << 34
        | BigUint::from_u32(position.last_funding_idx).unwrap() << 2
        | if position.order_side == OrderSide::Long {
            BigUint::one()
        } else {
            BigUint::zero()
        } << 1
        | if position.position_header.allow_partial_liquidations {
            BigUint::one()
        } else {
            BigUint::zero()
        };

    let public_key = position.position_header.position_address.clone();

    return (
        position.index as u64,
        [
            batched_position_info_slot1,
            batched_position_info_slot2,
            public_key,
        ],
    );
}

// * ———————————————————————————————————————————————————————————————————————————————————— * //

/// Check if the order_tab is part of updated_state_hashes and if
/// it is, then parse and append it to tab_outputs.
fn append_tab_output(
    updated_state_hashes: &HashMap<u64, (LeafNodeType, BigUint)>,
    tab_outputs: &mut Vec<(u64, [BigUint; 4])>,
    order_tab: &OrderTab,
) {
    let (leaf_type, leaf_hash) = updated_state_hashes
        .get(&(order_tab.tab_idx as u64))
        .unwrap();

    if leaf_type != &LeafNodeType::OrderTab || leaf_hash != &order_tab.hash {
        return;
    }

    let (index, output) = _get_tab_output(order_tab);
    tab_outputs.push((index, output));
}

fn _get_tab_output(order_tab: &OrderTab) -> (u64, [BigUint; 4]) {
    let base_hidden_amount = BigUint::from_u64(order_tab.base_amount).unwrap()
        ^ &order_tab.tab_header.base_blinding % BigUint::from_u64(2).unwrap().pow(64);
    let quote_hidden_amount = BigUint::from_u64(order_tab.quote_amount).unwrap()
        ^ &order_tab.tab_header.quote_blinding % BigUint::from_u64(2).unwrap().pow(64);

    // & batched_tab_info_slot format: | index (59 bits) | base_token (32 bits) | quote_token (32 bits) | base_hidden_amount (64 bits) | quote_hidden_amount (64 bits)
    let batched_tab_info = BigUint::from_u32(order_tab.tab_idx).unwrap() << 192
        | BigUint::from_u32(order_tab.tab_header.base_token).unwrap() << 160
        | BigUint::from_u32(order_tab.tab_header.quote_token).unwrap() << 128
        | base_hidden_amount << 64
        | quote_hidden_amount;

    let base_commitment = hash(
        &BigUint::from_u64(order_tab.base_amount).unwrap(),
        &order_tab.tab_header.base_blinding,
    );
    let quote_commitment = hash(
        &BigUint::from_u64(order_tab.quote_amount).unwrap(),
        &order_tab.tab_header.quote_blinding,
    );

    let public_key = order_tab.tab_header.pub_key.clone();

    return (
        order_tab.tab_idx as u64,
        [
            batched_tab_info,
            base_commitment,
            quote_commitment,
            public_key,
        ],
    );
}
