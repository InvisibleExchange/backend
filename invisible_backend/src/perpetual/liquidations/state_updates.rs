use std::{collections::HashMap, sync::Arc};

use error_stack::Result;
use num_bigint::BigUint;
use num_traits::Zero;
use parking_lot::Mutex;

use crate::{
    perpetual::perp_position::PerpPosition,
    transaction_batch::LeafNodeType,
    trees::superficial_tree::SuperficialTree,
    utils::{
        errors::{send_perp_swap_error, PerpSwapExecutionError},
        notes::Note,
    },
};

// ! UPDATING SPOT STATE ! // ============================================
pub fn update_state_after_liquidation(
    //
    state_tree_m: &Arc<Mutex<SuperficialTree>>,
    updated_state_hashes_m: &Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
    notes_in: &Vec<Note>,
    refund_note: &Option<Note>,
) -> Result<(), PerpSwapExecutionError> {
    let mut tree = state_tree_m.lock();
    let mut updated_state_hashes = updated_state_hashes_m.lock();

    //  ? verify notes exist in the tree
    for note in notes_in.iter() {
        let leaf_hash = tree.get_leaf_by_index(note.index);

        if leaf_hash != note.hash {
            return Err(send_perp_swap_error(
                "note spent for swap does not exist in the state".to_string(),
                None,
                None,
            ));
        }
    }

    // ? Update the state tree
    let refund_idx = notes_in[0].index;
    let refund_hash = if refund_note.is_some() {
        refund_note.as_ref().unwrap().hash.clone()
    } else {
        BigUint::zero()
    };

    tree.update_leaf_node(&refund_hash, refund_idx);
    updated_state_hashes.insert(refund_idx, (LeafNodeType::Note, refund_hash));

    for i in 1..notes_in.len() {
        let idx = notes_in[i].index;

        tree.update_leaf_node(&BigUint::zero(), idx);
        updated_state_hashes.insert(idx, (LeafNodeType::Note, BigUint::zero()));
    }
    drop(tree);
    drop(updated_state_hashes);

    Ok(())
}

// ! UPDATING PERPETUAL STATE ! // ============================================
pub fn update_perpetual_state_after_liquidation(
    state_tree_m: &Arc<Mutex<SuperficialTree>>,
    updated_state_hashes_m: &Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
    liquidated_position_index: u64,
    liquidated_position: &Option<PerpPosition>,
    new_position: &PerpPosition,
) -> Result<(), PerpSwapExecutionError> {
    //

    let mut state_tree = state_tree_m.lock();
    let mut updated_state_hashes = updated_state_hashes_m.lock();

    if liquidated_position.is_some() {
        let position = &liquidated_position.as_ref().unwrap();

        state_tree.update_leaf_node(&position.hash, position.index as u64);
        updated_state_hashes.insert(
            position.index as u64,
            (LeafNodeType::Position, position.hash.clone()),
        );
    } else {
        state_tree.update_leaf_node(&BigUint::zero(), liquidated_position_index as u64);
        updated_state_hashes.insert(
            liquidated_position_index as u64,
            (LeafNodeType::Position, BigUint::zero()),
        );
    }

    state_tree.update_leaf_node(&new_position.hash, new_position.index as u64);
    updated_state_hashes.insert(
        new_position.index as u64,
        (LeafNodeType::Position, new_position.hash.clone()),
    );

    drop(state_tree);
    drop(updated_state_hashes);

    Ok(())
}
