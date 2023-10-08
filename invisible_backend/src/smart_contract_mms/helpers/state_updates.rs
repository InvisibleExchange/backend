use std::{collections::HashMap, sync::Arc};

use num_bigint::BigUint;
use num_traits::Zero;
use parking_lot::Mutex;

use crate::{
    order_tab::OrderTab, perpetual::perp_position::PerpPosition, transaction_batch::LeafNodeType,
    trees::superficial_tree::SuperficialTree, utils::notes::Note,
};

// * Onchain Open Tab State Updates  -----------------------------------------------------------------------
pub fn onchain_register_mm_state_updates(
    state_tree: &Arc<Mutex<SuperficialTree>>,
    updated_state_hashes: &Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
    order_tab: &Option<OrderTab>,
    position: &Option<PerpPosition>,
    vlp_note: &Note,
) {
    let mut state_tree_m = state_tree.lock();
    let mut updated_state_hashes_m = updated_state_hashes.lock();

    // ? Add the vlp note to the state
    state_tree_m.update_leaf_node(&vlp_note.hash, vlp_note.index);
    updated_state_hashes_m.insert(vlp_note.index, (LeafNodeType::Note, vlp_note.hash.clone()));

    // ? add it to the order tabs state
    if let Some(tab) = order_tab {
        state_tree_m.update_leaf_node(&tab.hash, tab.tab_idx as u64);
        updated_state_hashes_m.insert(
            tab.tab_idx as u64,
            (LeafNodeType::MMSpotRegistration, tab.hash.clone()),
        );
    }

    // ? add it to the positons state
    if let Some(pos) = position {
        state_tree_m.update_leaf_node(&pos.hash, pos.index as u64);
        updated_state_hashes_m.insert(
            pos.index as u64,
            (LeafNodeType::MMPerpRegistration, pos.hash.clone()),
        );
    }

    println!("vlp_note: {:?}", vlp_note);
    println!("order_tab: {:?}", order_tab);

    drop(state_tree_m);
    drop(updated_state_hashes_m);
}

// * ================================================================================================
// * ADD LIQUIDITY * //

pub fn onchain_tab_add_liquidity_state_updates(
    state_tree: &Arc<Mutex<SuperficialTree>>,
    updated_state_hashes: &Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
    order_tab: &OrderTab,
    base_notes_in: &Vec<Note>,
    quote_notes_in: &Vec<Note>,
    base_refund_note: &Option<Note>,
    quote_refund_note: &Option<Note>,
    vlp_note: &Note,
) {
    // ? Remove the notes from the state tree and add the refund notes ------------------
    let mut state_tree_m = state_tree.lock();
    let mut updated_state_hashes_m = updated_state_hashes.lock();
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
    }
    if let Some(note) = quote_refund_note {
        state_tree_m.update_leaf_node(&note.hash, note.index);
        updated_state_hashes_m.insert(note.index, (LeafNodeType::Note, note.hash.clone()));
    }

    state_tree_m.update_leaf_node(&vlp_note.hash, vlp_note.index);
    updated_state_hashes_m.insert(vlp_note.index, (LeafNodeType::Note, vlp_note.hash.clone()));

    // ? add it to the order tabs state
    state_tree_m.update_leaf_node(&order_tab.hash, order_tab.tab_idx as u64);
    updated_state_hashes_m.insert(
        order_tab.tab_idx as u64,
        (LeafNodeType::OrderTab, order_tab.hash.clone()),
    );

    drop(state_tree_m);
    drop(updated_state_hashes_m);
}

pub fn onchain_position_add_liquidity_state_updates(
    state_tree: &Arc<Mutex<SuperficialTree>>,
    updated_state_hashes: &Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
    position: &PerpPosition,
    collateral_notes_in: &Vec<Note>,
    collateral_refund_note: &Option<Note>,
    vlp_note: &Note,
) {
    // ? Remove the notes from the state tree and add the refund notes ------------------
    let mut state_tree_m = state_tree.lock();
    let mut updated_state_hashes_m = updated_state_hashes.lock();
    for note in collateral_notes_in.into_iter() {
        state_tree_m.update_leaf_node(&BigUint::zero(), note.index);
        updated_state_hashes_m.insert(note.index, (LeafNodeType::Note, BigUint::zero()));
    }
    if let Some(note) = collateral_refund_note {
        state_tree_m.update_leaf_node(&note.hash, note.index);
        updated_state_hashes_m.insert(note.index, (LeafNodeType::Note, note.hash.clone()));
    }

    state_tree_m.update_leaf_node(&vlp_note.hash, vlp_note.index);
    updated_state_hashes_m.insert(vlp_note.index, (LeafNodeType::Note, vlp_note.hash.clone()));

    // ? add it to the order tabs state
    state_tree_m.update_leaf_node(&position.hash, position.index as u64);
    updated_state_hashes_m.insert(
        position.index as u64,
        (LeafNodeType::Position, position.hash.clone()),
    );

    drop(state_tree_m);
    drop(updated_state_hashes_m);
}

// * ================================================================================================
// * REMOVE LIQUIDITY * //

pub fn onchain_tab_remove_liquidity_state_updates(
    state_tree: &Arc<Mutex<SuperficialTree>>,
    updated_state_hashes: &Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
    tab_idx: u64,
    new_order_tab: &Option<OrderTab>,
    vlp_notes_in: &Vec<Note>,
    base_return_note: &Note,
    quote_return_note: &Note,
) {
    let mut state_tree_m = state_tree.lock();
    let mut updated_state_hashes_m = updated_state_hashes.lock();

    // ? Remove the vlp_notes_in from the state tree and add the return notes ------------------
    for note in vlp_notes_in {
        state_tree_m.update_leaf_node(&BigUint::zero(), note.index as u64);
        updated_state_hashes_m.insert(note.index as u64, (LeafNodeType::Note, BigUint::zero()));
    }
    // ? Add the base_return_note
    state_tree_m.update_leaf_node(&base_return_note.hash, base_return_note.index as u64);
    updated_state_hashes_m.insert(
        base_return_note.index as u64,
        (LeafNodeType::Note, base_return_note.hash.clone()),
    );
    // ? Add the quote_return_note
    state_tree_m.update_leaf_node(&quote_return_note.hash, quote_return_note.index as u64);
    updated_state_hashes_m.insert(
        quote_return_note.index as u64,
        (LeafNodeType::Note, quote_return_note.hash.clone()),
    );

    // ? add it to the order tabs state
    let new_order_tab_hash = if new_order_tab.is_some() {
        new_order_tab.as_ref().unwrap().hash.clone()
    } else {
        BigUint::zero()
    };
    state_tree_m.update_leaf_node(&new_order_tab_hash, tab_idx as u64);
    updated_state_hashes_m.insert(tab_idx as u64, (LeafNodeType::OrderTab, new_order_tab_hash));

    drop(state_tree_m);
    drop(updated_state_hashes_m);
}

pub fn onchain_position_remove_liquidity_state_updates(
    state_tree: &Arc<Mutex<SuperficialTree>>,
    updated_state_hashes: &Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
    pos_idx: u64,
    new_position: &Option<PerpPosition>,
    vlp_notes_in: &Vec<Note>,
    collateral_return_note: &Note,
) {
    let mut state_tree_m = state_tree.lock();
    let mut updated_state_hashes_m = updated_state_hashes.lock();

    // ? Remove the vlp_notes_in from the state tree and add the return notes ------------------
    for note in vlp_notes_in {
        state_tree_m.update_leaf_node(&BigUint::zero(), note.index as u64);
        updated_state_hashes_m.insert(note.index as u64, (LeafNodeType::Note, BigUint::zero()));
    }
    // ? Add the collateral_return_note
    state_tree_m.update_leaf_node(
        &collateral_return_note.hash,
        collateral_return_note.index as u64,
    );
    updated_state_hashes_m.insert(
        collateral_return_note.index as u64,
        (LeafNodeType::Note, collateral_return_note.hash.clone()),
    );

    // ? add it to the order tabs state
    let new_position_hash = if new_position.is_some() {
        new_position.as_ref().unwrap().hash.clone()
    } else {
        BigUint::zero()
    };
    state_tree_m.update_leaf_node(&new_position_hash, pos_idx as u64);
    updated_state_hashes_m.insert(pos_idx as u64, (LeafNodeType::Position, new_position_hash));

    drop(state_tree_m);
    drop(updated_state_hashes_m);
}

//
