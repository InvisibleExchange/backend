use std::collections::HashMap;

use num_bigint::BigUint;
use serde_json::{Map, Value};

use crate::transaction_batch::{
    restore_state::helpers::{
        perp_helpers::position_from_json, spot_helpers::restore_partial_fill_refund_note,
    },
    LeafNodeType,
};

use super::{
    super::helpers::{
        perp_helpers::{
            open_pos_after_liquidations, refund_partial_fill, return_collateral_on_close,
            update_liquidated_position, update_position_close, update_position_modify,
            update_position_open,
        },
        spot_helpers::{get_updated_order_tab, note_from_json, rebuild_swap_note},
    },
    helpers::{
        _update_accumulated_deposit_hash, _update_accumulated_withdrawal_hash, append_note_output,
        append_position_output, append_tab_output,
    },
};

// * SPOT SWAP DA FUNCTIONS ==========================================================================================

pub fn spot_order_da_output(
    updated_state_hashes: &HashMap<u64, (LeafNodeType, BigUint)>,
    note_outputs: &mut Vec<(u64, [BigUint; 4])>,
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
    note_outputs: &mut Vec<(u64, [BigUint; 4])>,
    accumulated_deposit_hashes: &mut HashMap<u32, BigUint>,
    transaction: &Map<String, Value>,
) {
    let deposit = transaction.get("deposit").unwrap();
    let deposit_notes = deposit.get("notes").unwrap().as_array().unwrap();

    let mut note_hashes: Vec<BigUint> = Vec::new();
    for note in deposit_notes {
        let note = note_from_json(note);
        append_note_output(updated_state_hashes, note_outputs, &note);

        note_hashes.push(note.hash);
    }

    // * Update accumulated deposit hashes * //

    _update_accumulated_deposit_hash(deposit, accumulated_deposit_hashes)
}

pub fn withdrawal_da_output(
    updated_state_hashes: &HashMap<u64, (LeafNodeType, BigUint)>,
    note_outputs: &mut Vec<(u64, [BigUint; 4])>,
    accumulated_withdrawal_hashes: &mut HashMap<u32, BigUint>,
    transaction: &Map<String, Value>,
) {
    let withdrawal = transaction.get("withdrawal").unwrap();

    let refund_note = withdrawal.get("refund_note").unwrap();
    if !refund_note.is_null() {
        let refund_note = note_from_json(refund_note);
        append_note_output(updated_state_hashes, note_outputs, &refund_note);
    }

    // * Update accumulated withdrawal hashes * //
    _update_accumulated_withdrawal_hash(withdrawal, accumulated_withdrawal_hashes);
}

// * PERP SWAP DA FUNCTIONS ==========================================================================================
pub fn perp_swap_da_output(
    updated_state_hashes: &HashMap<u64, (LeafNodeType, BigUint)>,
    note_outputs: &mut Vec<(u64, [BigUint; 4])>,
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
    note_outputs: &mut Vec<(u64, [BigUint; 4])>,
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
