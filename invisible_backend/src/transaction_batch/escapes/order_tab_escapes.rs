use firestore_db_and_auth::ServiceSession;
use num_bigint::BigUint;
use num_traits::{FromPrimitive, Zero};
use parking_lot::Mutex;
use std::str::FromStr;
use std::{collections::HashMap, sync::Arc};

use crate::trees::superficial_tree::SuperficialTree;
use crate::utils::crypto_utils::keccak256;
use crate::utils::storage::backup_storage::BackupStorage;
use crate::{
    order_tab::OrderTab,
    transaction_batch::LeafNodeType,
    utils::{
        crypto_utils::{verify, Signature},
        storage::firestore::start_delete_order_tab_thread,
    },
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

    if !verify_tab_signature(&order_tab, &signature) {
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

fn verify_tab_signature(order_tab: &OrderTab, signature: &Signature) -> bool {
    let message_hash = hash_tab_keccak(order_tab);

    let p = BigUint::from_str(
        "3618502788666131213697322783095070105623107215331596699973092056135872020481",
    )
    .unwrap();
    let hash_on_curve = message_hash % &p;

    let valid = verify(&order_tab.tab_header.pub_key, &hash_on_curve, signature);
    return valid;
}

// * --------------------------------------------------------------------------------------------

fn hash_tab_keccak(order_tab: &OrderTab) -> BigUint {
    // & H({base_token, quote_token, pub_key, base_amount, quote_amount})

    let mut input_arr = Vec::new();

    let base_token = BigUint::from_u32(order_tab.tab_header.base_token).unwrap();
    input_arr.push(base_token);

    let quote_token = BigUint::from_u32(order_tab.tab_header.quote_token).unwrap();
    input_arr.push(quote_token);

    input_arr.push(order_tab.tab_header.pub_key.clone());

    let base_amount = BigUint::from_u64(order_tab.base_amount).unwrap();
    input_arr.push(base_amount);

    let quote_amount = BigUint::from_u64(order_tab.quote_amount).unwrap();
    input_arr.push(quote_amount);

    let tab_hash = keccak256(&input_arr);

    return tab_hash;
}
