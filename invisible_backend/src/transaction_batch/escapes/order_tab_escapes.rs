use firestore_db_and_auth::ServiceSession;
use num_bigint::BigUint;
use num_traits::{FromPrimitive, Zero};
use parking_lot::Mutex;
use std::{collections::HashMap, sync::Arc};

use crate::{
    order_tab::OrderTab,
    transaction_batch::LeafNodeType,
    utils::{
        crypto_utils::{pedersen, verify, Signature},
        storage::firestore::start_delete_order_tab_thread,
    },
};
use crate::{
    trees::superficial_tree::SuperficialTree, utils::storage::local_storage::BackupStorage,
};

use serde::Deserialize;
use serde::Serialize;

#[derive(Serialize, Deserialize)]
pub struct OrderTabEscape {
    escape_id: u32,
    is_valid: bool,
    order_tab: OrderTab,
    valid_leaf: String,
    signature: Signature,
}

pub fn verify_order_tab_escape(
    state_tree: &Arc<Mutex<SuperficialTree>>,
    updated_state_hashes: &Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
    firebase_session: &Arc<Mutex<ServiceSession>>,
    backup_storage: &Arc<Mutex<BackupStorage>>,
    escape_id: u32,
    order_tab: OrderTab,
    signature: Signature,
) -> OrderTabEscape {
    let mut state_tree_m = state_tree.lock();
    let mut updated_state_hashes_m = updated_state_hashes.lock();

    let leaf_node = state_tree_m.get_leaf_by_index(order_tab.tab_idx as u64);
    let is_valid = leaf_node == order_tab.hash;

    if !verify_tab_signature(&order_tab, &signature, escape_id) {

        return OrderTabEscape {
            escape_id,
            is_valid,
            order_tab,
            valid_leaf: leaf_node.to_string(),
            signature,
        };
    }

    if is_valid {
        println!("VALID TAB ESCAPE: {}", escape_id);

        let z = BigUint::zero();
        state_tree_m.update_leaf_node(&z, order_tab.tab_idx as u64);
        updated_state_hashes_m.insert(order_tab.tab_idx as u64, (LeafNodeType::OrderTab, z));

        // ? Update the database
        let _h = start_delete_order_tab_thread(
            firebase_session,
            backup_storage,
            order_tab.tab_header.pub_key.to_string(),
            order_tab.tab_idx.to_string(),
        );
    }

    OrderTabEscape {
        escape_id,
        is_valid,
        order_tab,
        valid_leaf: leaf_node.to_string(),
        signature,
    }
}

fn verify_tab_signature(order_tab: &OrderTab, signature: &Signature, escape_id: u32) -> bool {
    let message_hash = pedersen(&order_tab.hash, &BigUint::from_u32(escape_id).unwrap());
    let valid = verify(&order_tab.tab_header.pub_key, &message_hash, signature);
    return valid;
}
