use std::{collections::HashMap, sync::Arc};

use num_bigint::BigUint;
use num_traits::Zero;
use parking_lot::Mutex;

use crate::{
    transaction_batch::{LeafNodeType, StateUpdate, TxOutputJson},
    trees::superficial_tree::SuperficialTree,
    utils::notes::Note,
};

use super::OrderTab;

// * Open Tab State Updates  -----------------------------------------------------------------------
pub fn open_tab_state_updates(
    state_tree: &Arc<Mutex<SuperficialTree>>,
    updated_state_hashes: &Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
    transaction_output_json: &Arc<Mutex<TxOutputJson>>,
    order_tab: OrderTab,
    base_notes_in: Vec<Note>,
    quote_notes_in: Vec<Note>,
    base_refund_note: Option<Note>,
    quote_refund_note: Option<Note>,
) {
    // ? Remove the notes from the state tree and add the refund notes ------------------
    let mut state_tree_m = state_tree.lock();
    let mut updated_state_hashes_m = updated_state_hashes.lock();
    let mut transaction_output_json_m = transaction_output_json.lock();
    for note in base_notes_in.into_iter() {
        state_tree_m.update_leaf_node(&BigUint::zero(), note.index);
        updated_state_hashes_m.insert(note.index, (LeafNodeType::Note, BigUint::zero()));
    }
    for note in quote_notes_in.into_iter() {
        state_tree_m.update_leaf_node(&BigUint::zero(), note.index);
        updated_state_hashes_m.insert(note.index, (LeafNodeType::Note, BigUint::zero()));
    }
    if let Some(note) = base_refund_note {
        state_tree_m.update_leaf_node(&note.hash, note.index);
        updated_state_hashes_m.insert(note.index, (LeafNodeType::Note, note.hash.clone()));
        transaction_output_json_m
            .state_updates
            .push(StateUpdate::Note { note });
    }
    if let Some(note) = quote_refund_note {
        state_tree_m.update_leaf_node(&note.hash, note.index);
        updated_state_hashes_m.insert(note.index, (LeafNodeType::Note, note.hash.clone()));
        transaction_output_json_m
            .state_updates
            .push(StateUpdate::Note { note });
    }

    // ? add it to the order tabs state
    state_tree_m.update_leaf_node(&order_tab.hash, order_tab.tab_idx as u64);
    updated_state_hashes_m.insert(
        order_tab.tab_idx as u64,
        (LeafNodeType::OrderTab, order_tab.hash.clone()),
    );
    transaction_output_json_m
        .state_updates
        .push(StateUpdate::OrderTab { order_tab });

    drop(state_tree_m);
    drop(updated_state_hashes_m);
    drop(transaction_output_json_m)
}

// * Close Tab State Updates -----------------------------------------------------------------------
pub fn close_tab_state_updates(
    state_tree: &Arc<Mutex<SuperficialTree>>,
    updated_state_hashes: &Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
    transaction_output_json: &Arc<Mutex<TxOutputJson>>,
    order_tab: &OrderTab,
    updated_order_tab: &Option<OrderTab>,
    base_return_note: Note,
    quote_return_note: Note,
) {
    // ? add the return notes to the state
    let mut state_tree_m = state_tree.lock();
    let mut updated_state_hashes_m = updated_state_hashes.lock();
    let mut transaction_output_json_m = transaction_output_json.lock();

    state_tree_m.update_leaf_node(&base_return_note.hash, base_return_note.index);
    updated_state_hashes_m.insert(
        base_return_note.index,
        (LeafNodeType::Note, base_return_note.hash.clone()),
    );
    transaction_output_json_m
        .state_updates
        .push(StateUpdate::Note {
            note: base_return_note,
        });

    state_tree_m.update_leaf_node(&quote_return_note.hash, quote_return_note.index);
    updated_state_hashes_m.insert(
        quote_return_note.index,
        (LeafNodeType::Note, quote_return_note.hash.clone()),
    );
    transaction_output_json_m
        .state_updates
        .push(StateUpdate::Note {
            note: quote_return_note,
        });

    // ? remove the tab from the state
    let updated_tab_hash = if updated_order_tab.is_some() {
        updated_order_tab.as_ref().unwrap().hash.clone()
    } else {
        BigUint::zero()
    };

    state_tree_m.update_leaf_node(&updated_tab_hash, order_tab.tab_idx as u64);
    updated_state_hashes_m.insert(
        order_tab.tab_idx as u64,
        (LeafNodeType::OrderTab, updated_tab_hash),
    );
    if let Some(order_tab) = updated_order_tab {
        transaction_output_json_m
            .state_updates
            .push(StateUpdate::OrderTab {
                order_tab: order_tab.clone(),
            });
    }

    drop(state_tree_m);
    drop(updated_state_hashes_m);
    drop(transaction_output_json_m)
}
