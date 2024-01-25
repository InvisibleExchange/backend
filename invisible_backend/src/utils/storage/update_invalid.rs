use std::{collections::HashMap, sync::Arc};

use crate::{
    trees::superficial_tree::SuperficialTree,
    utils::storage::{
        firestore_helpers::{store_note_output, store_order_tab_output, store_position_output},
        get_state_at_index, StateValue,
    },
};
use firestore_db_and_auth::ServiceSession;

use num_bigint::BigUint;
use num_traits::Zero;
use parking_lot::Mutex;

use super::backup_storage::BackupStorage;

/// Reads the state that is stored locally and updates in the database.
/// This is used to update the state when the database is corrupted.
/// We monitor the state externally and update it when necessary.
pub fn update_invalid_state(
    state_tree_m: &Arc<Mutex<SuperficialTree>>,
    session: &Arc<Mutex<ServiceSession>>,
    backup_storage: &Arc<Mutex<BackupStorage>>,
    indexes: Vec<u64>,
) {
    let state_tree = state_tree_m.lock();

    let session = session.lock();
    for i in indexes {
        let state_value = get_state_at_index(i as u64);

        if state_value.is_none() {
            assert!(
                state_tree.leaf_nodes[i as usize] == BigUint::zero(),
                "state value at index {} is not zero",
                i
            );

            continue;
        }

        match state_value.unwrap().1 {
            StateValue::Note(note) => {
                assert!(
                    state_tree.leaf_nodes[i as usize].to_string() == note.hash,
                    "state value at index {} is not equal to note hash",
                    i
                );

                store_note_output(&session, note);
            }
            StateValue::OrderTab(order_tab_output) => {
                assert!(
                    state_tree.leaf_nodes[i as usize].to_string() == order_tab_output.hash,
                    "state value at index {} is not equal to order tab hash",
                    i
                );

                store_order_tab_output(&session, order_tab_output);
            }
            StateValue::Position(position_output) => {
                assert!(
                    state_tree.leaf_nodes[i as usize].to_string() == position_output.hash,
                    "state value at index {} is not equal to perp position hash",
                    i
                );

                store_position_output(&session, backup_storage, position_output);
            }
        }
    }
}

pub fn verify_state_storage(
    state_tree_m: &Arc<Mutex<SuperficialTree>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let state_tree = state_tree_m.lock();

    let mut state_map: HashMap<u64, String> = HashMap::new();

    for i in 0..state_tree.leaf_nodes.len() {
        let state_value = get_state_at_index(i as u64);

        if state_value.is_none() {
            assert!(
                state_tree.leaf_nodes[i] == BigUint::zero(),
                "state value at index {} is not zero",
                i
            );

            state_map.insert(i as u64, "0".to_string());

            continue;
        }

        match state_value.unwrap().1 {
            StateValue::Note(note) => {
                assert!(
                    state_tree.leaf_nodes[i].to_string() == note.hash,
                    "state value at index {} is not equal to note hash",
                    i
                );

                state_map.insert(i as u64, note.hash);
            }
            StateValue::OrderTab(order_tab) => {
                assert!(
                    state_tree.leaf_nodes[i].to_string() == order_tab.hash,
                    "state value at index {} is not equal to order tab hash",
                    i
                );

                state_map.insert(i as u64, order_tab.hash);
            }
            StateValue::Position(perp_position) => {
                assert!(
                    state_tree.leaf_nodes[i].to_string() == perp_position.hash,
                    "state value at index {} is not equal to perp position hash",
                    i
                );

                state_map.insert(i as u64, perp_position.hash);
            }
        }
    }

    println!("state_map: {:#?}", state_map);

    Ok(())
}
