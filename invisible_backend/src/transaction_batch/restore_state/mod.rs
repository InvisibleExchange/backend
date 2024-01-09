use std::{collections::HashMap, sync::Arc};

use num_bigint::BigUint;
use parking_lot::Mutex;
use serde_json::{Map, Value};

use crate::{trees::superficial_tree::SuperficialTree, utils::notes::Note};

use self::{
    helpers::{restore_margin_update, restore_note_split},
    restore_forced_escapes::{
        restore_forced_note_escape, restore_forced_position_escape, restore_forced_tab_escape,
    },
    restore_order_tabs::{
        restore_close_order_tab, restore_onchain_mm_action, restore_open_order_tab,
    },
    restore_perp_swaps::{restore_liquidation_order_execution, restore_perp_order_execution},
    restore_spot_swap::{
        restore_deposit_update, restore_spot_order_execution, restore_withdrawal_update,
    },
};

use super::LeafNodeType;

pub mod helpers;
mod restore_forced_escapes;
mod restore_order_tabs;
mod restore_perp_swaps;
mod restore_spot_swap;

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

        match transaction_type {
            "deposit" => {
                let deposit_notes = transaction
                    .get("deposit")
                    .unwrap()
                    .get("notes")
                    .unwrap()
                    .as_array()
                    .unwrap();

                restore_deposit_update(&state_tree, &updated_state_hashes, deposit_notes);
            }
            "withdrawal" => {
                let withdrawal_notes_in = transaction
                    .get("withdrawal")
                    .unwrap()
                    .get("notes_in")
                    .unwrap()
                    .as_array()
                    .unwrap();
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
