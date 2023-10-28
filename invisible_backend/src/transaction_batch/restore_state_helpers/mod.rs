use std::{collections::HashMap, sync::Arc};

use num_bigint::BigUint;
use parking_lot::Mutex;
use serde_json::{Map, Value};

use crate::{
    trees::superficial_tree::SuperficialTree,
    utils::{notes::Note, storage::local_storage::MainStorage},
};

use self::{
    helpers::{restore_margin_update, restore_note_split},
    restore_order_tabs::{
        restore_add_liquidity, restore_close_order_tab, restore_open_order_tab,
        restore_register_mm, restore_remove_liquidity,
    },
    restore_perp_swaps::{restore_liquidation_order_execution, restore_perp_order_execution},
    restore_spot_swap::{
        restore_deposit_update, restore_spot_order_execution, restore_withdrawal_update,
    },
};

use super::LeafNodeType;

pub mod helpers;
pub mod restore_order_tabs;
pub mod restore_perp_swaps;
pub mod restore_spot_swap;

pub fn _restore_state_inner(
    state_tree: &Arc<Mutex<SuperficialTree>>,
    updated_state_hashes: &Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
    perpetual_partial_fill_tracker: &Arc<Mutex<HashMap<u64, (Option<Note>, u64, u64)>>>,
    main_storage: &Arc<Mutex<MainStorage>>,
    transactions: Vec<Map<String, Value>>,
) {
    let mut n_deposits = 0;
    let mut n_withdrawals = 0;
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

                n_deposits += 1;
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

                n_withdrawals += 1;
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
            "onchain_register_mm" => {
                restore_register_mm(&state_tree, &updated_state_hashes, &transaction)
            }
            "add_liquidity" => {
                restore_add_liquidity(&state_tree, &updated_state_hashes, &transaction)
            }
            "remove_liquidity" => {
                restore_remove_liquidity(&state_tree, &updated_state_hashes, &transaction)
            }
            _ => {
                panic!("Invalid transaction type");
            }
        }
    }

    let mut storage = main_storage.lock();
    storage.n_deposits = n_deposits;
    storage.n_withdrawals = n_withdrawals;
    drop(storage);
}
