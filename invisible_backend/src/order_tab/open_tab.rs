use std::{collections::HashMap, sync::Arc};

use num_bigint::BigUint;
use num_traits::Zero;
use parking_lot::Mutex;
use serde_json::Value;
use starknet::curve::AffinePoint;

use firestore_db_and_auth::ServiceSession;

use crate::utils::storage::backup_storage::BackupStorage;
use crate::{
    server::grpc::engine_proto::OpenOrderTabReq,
    transaction_batch::LeafNodeType,
    trees::superficial_tree::SuperficialTree,
    utils::{crypto_utils::hash_many, notes::Note},
};

use crate::utils::crypto_utils::{verify, EcPoint, Signature};

use super::{
    db_updates::open_tab_db_updates, json_output::open_tab_json_output,
    state_updates::open_tab_state_updates, OrderTab,
};

// TODO: Check that the notes exist just before you update the state tree not in the beginning

pub fn open_order_tab(
    session: &Arc<Mutex<ServiceSession>>,
    backup_storage: &Arc<Mutex<BackupStorage>>,
    open_order_tab_req: OpenOrderTabReq,
    state_tree: &Arc<Mutex<SuperficialTree>>,
    updated_state_hashes: &Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
    swap_output_json_m: &Arc<Mutex<Vec<serde_json::Map<String, Value>>>>,
) -> std::result::Result<OrderTab, String> {
    let sig_pub_key: BigUint;

    let tab_header = open_order_tab_req
        .order_tab
        .as_ref()
        .unwrap()
        .tab_header
        .as_ref()
        .unwrap();
    let base_token = tab_header.base_token;
    let quote_token = tab_header.quote_token;

    let mut base_amount = 0;
    let mut base_refund_note: Option<Note> = None;
    let mut quote_amount = 0;
    let mut quote_refund_note: Option<Note> = None;

    let mut pub_key_sum: AffinePoint = AffinePoint::identity();

    // ? Check that the token pair is valid

    // ? Check that the notes spent exist
    let mut state_tree_m = state_tree.lock();
    // & BASE TOKEN —————————————————————————
    let mut base_notes_in = Vec::new();
    for note_ in open_order_tab_req.base_notes_in.into_iter() {
        if note_.token != base_token {
            return Err("token missmatch".to_string());
        }

        let note = Note::try_from(note_);
        if let Err(e) = note {
            return Err(e.to_string());
        }
        let note = note.unwrap();

        // ? Check that notes for base token exist
        let leaf_hash = state_tree_m.get_leaf_by_index(note.index);

        if leaf_hash != note.hash {
            return Err("note spent to open tab does not exist".to_string());
        }

        // ? Add to the pub key for sig verification
        let ec_point = AffinePoint::from(&note.address);
        pub_key_sum = &pub_key_sum + &ec_point;

        base_amount += note.amount;

        base_notes_in.push(note);
    }
    // ? Check if there is a refund note for base token
    if open_order_tab_req.base_refund_note.is_some() {
        let note_ = open_order_tab_req.base_refund_note.as_ref().unwrap();
        if note_.token != base_token {
            return Err("token missmatch".to_string());
        }

        if note_.index != base_notes_in[0].index {
            return Err("refund note index missmatch".to_string());
        }

        base_amount -= note_.amount;

        base_refund_note = Note::try_from(note_.clone()).ok();
    }

    // & QUOTE TOKEN —————————————————————————
    // ? Check that notes for quote token exist
    let mut quote_notes_in = Vec::new();
    for note_ in open_order_tab_req.quote_notes_in.into_iter() {
        if note_.token != quote_token {
            return Err("token missmatch".to_string());
        }

        let note = Note::try_from(note_);
        if let Err(e) = note {
            return Err(e.to_string());
        }
        let note = note.unwrap();

        let leaf_hash = state_tree_m.get_leaf_by_index(note.index);

        if leaf_hash != note.hash {
            return Err("note spent to open tab does not exist".to_string());
        }

        // ? Add to the pub key for sig verification
        let ec_point = AffinePoint::from(&note.address);
        pub_key_sum = &pub_key_sum + &ec_point;

        quote_amount += note.amount;

        quote_notes_in.push(note);
    }
    // ? Check if there is a refund note for base token
    if open_order_tab_req.quote_refund_note.is_some() {
        let note_ = open_order_tab_req.quote_refund_note.as_ref().unwrap();
        if note_.token != quote_token {
            return Err("token missmatch".to_string());
        }

        if note_.index != quote_notes_in[0].index {
            return Err("refund note index missmatch".to_string());
        }

        quote_amount -= note_.amount;
        quote_refund_note = Note::try_from(note_.clone()).ok();
    }

    // ? Get the public key from the sum of the notes
    sig_pub_key = EcPoint::from(&pub_key_sum).x.to_biguint().unwrap();

    // ? Create an OrderTab object and verify against base and quote amounts
    let order_tab = OrderTab::try_from(open_order_tab_req.order_tab.unwrap());
    if let Err(e) = order_tab {
        return Err(e.to_string());
    }
    let mut order_tab = order_tab.unwrap();

    let prev_order_tab;
    if open_order_tab_req.add_only {
        // ? Verify that the order tab exists

        let leaf_hash = state_tree_m.get_leaf_by_index(order_tab.tab_idx as u64);
        if leaf_hash != order_tab.hash {
            return Err("order tab does not exist".to_string());
        }

        // ? Adding to an existing order tab
        prev_order_tab = Some(order_tab.clone());

        order_tab.base_amount += base_amount;
        order_tab.quote_amount += quote_amount;

        order_tab.update_hash();
    } else {
        // ? Opening new order tab
        prev_order_tab = None;

        order_tab.base_amount = base_amount;
        order_tab.quote_amount = quote_amount;

        // ? Set the tab index
        let z_index = state_tree_m.first_zero_idx();
        order_tab.tab_idx = z_index;

        order_tab.update_hash();
    }

    drop(state_tree_m);

    // ? Verify the signature ---------------------------------------------------------------------
    let signature = Signature::try_from(open_order_tab_req.signature.unwrap_or_default())
        .map_err(|err| err.to_string())?;
    let valid = verfiy_open_order_sig(
        &prev_order_tab,
        &order_tab,
        &base_refund_note,
        &quote_refund_note,
        &sig_pub_key,
        &signature,
    );

    if !valid {
        return Err("Invalid Signature".to_string());
    }

    // ? GENERATE THE JSON_OUTPUT -----------------------------------------------------------------
    open_tab_json_output(
        &swap_output_json_m,
        &base_notes_in,
        &base_refund_note,
        &quote_notes_in,
        &quote_refund_note,
        open_order_tab_req.add_only,
        &prev_order_tab,
        &order_tab,
        &signature,
    );

    // ? UPDATE THE DATABASE ----------------------------------------------------------------------
    open_tab_db_updates(
        session,
        backup_storage,
        order_tab.clone(),
        &base_notes_in,
        &quote_notes_in,
        base_refund_note.clone(),
        quote_refund_note.clone(),
    );

    // ? UPDATE THE STATE TREE --------------------------------------------------------------------
    open_tab_state_updates(
        state_tree,
        updated_state_hashes,
        order_tab.clone(),
        base_notes_in,
        quote_notes_in,
        base_refund_note,
        quote_refund_note,
    );

    Ok(order_tab)
}

//

// * HELPERS =======================================================================================

/// Verify the signature for the order tab hash
pub fn verfiy_open_order_sig(
    prev_order_tab: &Option<OrderTab>,
    new_order_tab: &OrderTab,
    base_refund_note: &Option<Note>,
    quote_refund_note: &Option<Note>,
    pub_key: &BigUint,
    signature: &Signature,
) -> bool {
    // & header_hash = H({prev_tab_hash, new_tab_hash, base_refund_note_hash, quote_refund_note_hash})

    let mut hash_inputs: Vec<&BigUint> = Vec::new();

    let z = BigUint::zero();
    let prev_tab_hash = if prev_order_tab.is_some() {
        &prev_order_tab.as_ref().unwrap().hash
    } else {
        &z
    };
    hash_inputs.push(prev_tab_hash);

    let new_tab_hash = &new_order_tab.hash;
    hash_inputs.push(new_tab_hash);

    let base_refund_note_hash = if base_refund_note.is_some() {
        &base_refund_note.as_ref().unwrap().hash
    } else {
        &z
    };
    hash_inputs.push(&base_refund_note_hash);

    let quote_refund_note_hash = if quote_refund_note.is_some() {
        &quote_refund_note.as_ref().unwrap().hash
    } else {
        &z
    };
    hash_inputs.push(&quote_refund_note_hash);

    let hash = hash_many(&hash_inputs);

    let valid = verify(pub_key, &hash, signature);

    return valid;
}
