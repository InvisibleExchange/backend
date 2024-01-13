use std::{collections::HashMap, sync::Arc};

use num_bigint::BigUint;
use num_traits::{FromPrimitive, Zero};
use parking_lot::Mutex;
use serde_json::{Map, Value};

use crate::{
    trees::superficial_tree::SuperficialTree,
    utils::{crypto_utils::hash_many, notes::Note},
};

use self::{
    da_output_functions::{
        close_order_tab_da_ouput, deposit_da_output, forced_position_escape_da_output,
        liquidations_da_output, margin_update_da_output, note_split_da_output,
        onchain_mm_action_da_output, open_order_tab_da_output, perp_swap_da_output,
        spot_order_da_output, withdrawal_da_output,
    },
    helpers::state_helpers::{restore_margin_update, restore_note_split},
    restore_functions::restore_forced_escapes::{
        restore_forced_note_escape, restore_forced_position_escape, restore_forced_tab_escape,
    },
    restore_functions::restore_order_tabs::{
        restore_close_order_tab, restore_onchain_mm_action, restore_open_order_tab,
    },
    restore_functions::restore_perp_swaps::{
        restore_liquidation_order_execution, restore_perp_order_execution,
    },
    restore_functions::restore_spot_swap::{
        restore_deposit_update, restore_spot_order_execution, restore_withdrawal_update,
    },
};

use super::LeafNodeType;

mod da_output_functions;
mod helpers;
mod restore_functions;

// * RESTORE STATE FROM THE TRANSACTION BATCH * //

pub fn _restore_state_inner(
    state_tree: &Arc<Mutex<SuperficialTree>>,
    updated_state_hashes: &Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
    perpetual_partial_fill_tracker: &Arc<Mutex<HashMap<u64, (Option<Note>, u64, u64)>>>,
    transactions: Vec<Map<String, Value>>,
) {
    for transaction in transactions {
        let transaction_type = transaction
            .get("transaction_type")
            .unwrap()
            .as_str()
            .unwrap();

        println!("transaction_type: {:?}", transaction_type);

        match transaction_type {
            "deposit" => {
                let deposit = transaction.get("deposit").unwrap();
                let deposit_notes = deposit.get("notes").unwrap().as_array().unwrap();

                restore_deposit_update(&state_tree, &updated_state_hashes, deposit_notes);
            }
            "withdrawal" => {
                let withdrawal = transaction.get("withdrawal").unwrap();
                let withdrawal_notes_in = withdrawal.get("notes_in").unwrap().as_array().unwrap();
                let refund_note = transaction.get("withdrawal").unwrap().get("refund_note");

                restore_withdrawal_update(
                    &state_tree,
                    &updated_state_hashes,
                    withdrawal_notes_in,
                    refund_note,
                );
            }
            "swap" => {
                // * Order a ------------------------

                restore_spot_order_execution(
                    &state_tree,
                    &updated_state_hashes,
                    &transaction,
                    true,
                );

                // * Order b ------------------------

                restore_spot_order_execution(
                    &state_tree,
                    &updated_state_hashes,
                    &transaction,
                    false,
                );
            }
            "perpetual_swap" => {
                // * Order a ------------------------
                restore_perp_order_execution(
                    &state_tree,
                    &updated_state_hashes,
                    &perpetual_partial_fill_tracker,
                    &transaction,
                    true,
                );

                // * Order b ------------------------
                restore_perp_order_execution(
                    &state_tree,
                    &updated_state_hashes,
                    &perpetual_partial_fill_tracker,
                    &transaction,
                    false,
                );
            }
            "liquidation_order" => restore_liquidation_order_execution(
                &state_tree,
                &updated_state_hashes,
                &transaction,
            ),
            "margin_change" => {
                restore_margin_update(&state_tree, &updated_state_hashes, &transaction)
            }
            "note_split" => restore_note_split(&state_tree, &updated_state_hashes, &transaction),
            "open_order_tab" => {
                restore_open_order_tab(&state_tree, &updated_state_hashes, &transaction);
            }
            "close_order_tab" => {
                restore_close_order_tab(&state_tree, &updated_state_hashes, &transaction)
            }
            "onchain_mm_action" => {
                restore_onchain_mm_action(&state_tree, &updated_state_hashes, &transaction)
            }
            "forced_escape" => match transaction.get("escape_type").unwrap().as_str().unwrap() {
                "note_escape" => {
                    restore_forced_note_escape(&state_tree, &updated_state_hashes, &transaction)
                }
                "order_tab_escape" => {
                    restore_forced_tab_escape(&state_tree, &updated_state_hashes, &transaction)
                }
                "position_escape" => {
                    restore_forced_position_escape(&state_tree, &updated_state_hashes, &transaction)
                }
                _ => {
                    panic!("Invalid escape type");
                }
            },
            _ => {
                panic!("Invalid transaction type");
            }
        }
    }
}

// * DATA AVAILABILITY OUTPUT (Notes/Positions/OrderTabs) * //

pub fn _get_da_updates_inner(
    updated_state_hashes: &HashMap<u64, (LeafNodeType, BigUint)>,
    funding_rates: &HashMap<u32, Vec<i64>>,
    funding_prices: &HashMap<u32, Vec<u64>>,
    transactions: &Vec<Map<String, Value>>,
) -> (BigUint, Vec<String>) {
    let mut note_outputs: Vec<(u64, [BigUint; 3])> = Vec::new();
    let mut position_outputs: Vec<(u64, [BigUint; 3])> = Vec::new();
    let mut tab_outputs: Vec<(u64, [BigUint; 4])> = Vec::new();
    let mut zero_indexes: Vec<u64> = Vec::new();

    for transaction in transactions {
        let transaction_type = transaction
            .get("transaction_type")
            .unwrap()
            .as_str()
            .unwrap();

        match transaction_type {
            "deposit" => {
                deposit_da_output(updated_state_hashes, &mut note_outputs, &transaction);
            }
            "withdrawal" => {
                withdrawal_da_output(updated_state_hashes, &mut note_outputs, &transaction)
            }
            "swap" => {
                // * Order a ------------------------
                spot_order_da_output(
                    updated_state_hashes,
                    &mut note_outputs,
                    &mut tab_outputs,
                    &transaction,
                    true,
                );

                // * Order b ------------------------
                spot_order_da_output(
                    updated_state_hashes,
                    &mut note_outputs,
                    &mut tab_outputs,
                    &transaction,
                    false,
                );
            }
            "perpetual_swap" => {
                // * Order a ------------------------
                perp_swap_da_output(
                    updated_state_hashes,
                    &mut note_outputs,
                    &mut position_outputs,
                    funding_rates,
                    funding_prices,
                    &transaction,
                    true,
                );

                // * Order b ------------------------
                perp_swap_da_output(
                    updated_state_hashes,
                    &mut note_outputs,
                    &mut position_outputs,
                    funding_rates,
                    funding_prices,
                    &transaction,
                    false,
                );
            }
            "liquidation_order" => liquidations_da_output(
                updated_state_hashes,
                &mut note_outputs,
                &mut position_outputs,
                funding_rates,
                funding_prices,
                &transaction,
            ),
            "margin_change" => margin_update_da_output(
                updated_state_hashes,
                &mut note_outputs,
                &mut position_outputs,
                &transaction,
            ),
            "note_split" => {
                note_split_da_output(updated_state_hashes, &mut note_outputs, &transaction)
            }
            "open_order_tab" => {
                open_order_tab_da_output(
                    updated_state_hashes,
                    &mut note_outputs,
                    &mut tab_outputs,
                    &transaction,
                );
            }
            "close_order_tab" => {
                println!("close_order_tab");

                close_order_tab_da_ouput(
                    updated_state_hashes,
                    &mut note_outputs,
                    &mut tab_outputs,
                    &transaction,
                )
            }
            "onchain_mm_action" => onchain_mm_action_da_output(
                updated_state_hashes,
                &mut position_outputs,
                &transaction,
            ),
            "forced_escape" => match transaction.get("escape_type").unwrap().as_str().unwrap() {
                "note_escape" => {}
                "order_tab_escape" => {}
                "position_escape" => {
                    forced_position_escape_da_output(
                        updated_state_hashes,
                        &mut position_outputs,
                        &transaction,
                    );
                }
                _ => {
                    panic!("Invalid escape type");
                }
            },
            _ => {
                panic!("Invalid transaction type");
            }
        }
    }

    for (i, (_t, leaf_hash)) in updated_state_hashes.iter() {
        if leaf_hash == &BigUint::from(0u64) {
            zero_indexes.push(*i);
        }
    }

    note_outputs.sort_unstable();
    position_outputs.sort_unstable();
    tab_outputs.sort_unstable();
    zero_indexes.sort_unstable();

    // Remove duplicates
    note_outputs.dedup();
    position_outputs.dedup();
    tab_outputs.dedup();
    zero_indexes.dedup();

    println!("tab_outputs: {:?}", tab_outputs);

    // Join all the outputs into a single vector
    let mut data_output: Vec<BigUint> = Vec::new();

    for (_, _output) in note_outputs.drain(..) {
        for el in _output {
            data_output.push(el);
        }
    }
    for (_, _output) in position_outputs.drain(..) {
        for el in _output {
            data_output.push(el);
        }
    }
    for (_, _output) in tab_outputs.drain(..) {
        for el in _output {
            data_output.push(el);
        }
    }
    for _chunk in zero_indexes.chunks(3) {
        let mut idx_batched = BigUint::zero();

        for idx in _chunk {
            idx_batched = idx_batched << 64 | BigUint::from_u64(*idx).unwrap();
        }
        data_output.push(idx_batched);
    }

    // ? Hash and upload the data output
    let references: Vec<&BigUint> = data_output.iter().collect();
    let data_commitment = hash_many(&references);

    let data_output: Vec<String> = data_output.into_iter().map(|el| el.to_string()).collect();

    return (data_commitment, data_output);
}
