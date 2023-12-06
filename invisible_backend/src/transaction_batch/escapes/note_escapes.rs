use firestore_db_and_auth::ServiceSession;
use num_bigint::BigUint;
use num_traits::{FromPrimitive, Zero};
use parking_lot::Mutex;
use starknet::curve::AffinePoint;
use std::{collections::HashMap, sync::Arc};

use crate::{
    transaction_batch::LeafNodeType,
    utils::{
        crypto_utils::{pedersen_on_vec, verify, EcPoint, Signature},
        storage::firestore::start_delete_note_thread,
    },
};
use crate::{
    trees::superficial_tree::SuperficialTree, utils::storage::local_storage::BackupStorage,
};

use crate::utils::notes::Note;

use serde::Deserialize;
use serde::Serialize;

#[derive(Serialize, Deserialize)]
pub struct NoteEscape {
    escape_id: u32,
    escape_notes: Vec<Note>,
    invalid_note: Option<(u64, String)>, // (idx, leaf) of one invalid note (if any)
    signature: Signature,
}

pub fn verify_note_escape(
    state_tree: &Arc<Mutex<SuperficialTree>>,
    updated_state_hashes: &Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
    firebase_session: &Arc<Mutex<ServiceSession>>,
    backup_storage: &Arc<Mutex<BackupStorage>>,
    escape_id: u32,
    escape_notes: Vec<Note>,
    signature: Signature,
) -> NoteEscape {
    let order_hash = hash_note_escape_message(escape_id, &escape_notes);
    // let is_signature_valid = true;
    let is_signature_valid = verify_note_signatures(&escape_notes, &signature, &order_hash);

    let invalid_note = find_invalid_note(state_tree, &escape_notes);

    // ? Verify the signatures
    if invalid_note.is_some() || !is_signature_valid {

        return NoteEscape {
            escape_id,
            escape_notes,
            invalid_note,
            signature,
        };
    };

    // * If escape is fully valid than update the state tree and database -------------------
    let mut state_tree_m = state_tree.lock();
    let mut updated_state_hashes_m = updated_state_hashes.lock();

    // ? Update the state and database
    for note in escape_notes.iter() {
        let z = BigUint::zero();
        state_tree_m.update_leaf_node(&z, note.index);
        updated_state_hashes_m.insert(note.index, (LeafNodeType::Note, z));

        // ? Update the database
        let _h = start_delete_note_thread(
            firebase_session,
            backup_storage,
            note.address.x.to_string(),
            note.index.to_string(),
        );
    }
    drop(state_tree_m);
    drop(updated_state_hashes_m);

    println!("VALID NOTE ESCAPE: {}", escape_id);

    NoteEscape {
        escape_id,
        escape_notes,
        invalid_note,
        signature,
    }
}

/// If any of the notes is invalid, return the index and leaf of the first invalid note.
pub fn find_invalid_note(
    state_tree: &Arc<Mutex<SuperficialTree>>,
    escape_notes: &Vec<Note>,
) -> Option<(u64, String)> {
    let state_tree_m = state_tree.lock();

    let mut invalid_leaf: Option<(u64, String)> = None;

    for note in escape_notes.into_iter() {
        let leaf_node = state_tree_m.get_leaf_by_index(note.index);
        if leaf_node != note.hash {
            println!("invalid note: {} {}", note.index, leaf_node.to_string());

            invalid_leaf = Some((note.index, leaf_node.to_string()));

            break;
        }
    }

    return invalid_leaf;
}

fn verify_note_signatures(
    notes_in: &Vec<Note>,
    signature: &Signature,
    order_hash: &BigUint,
) -> bool {
    let mut pub_key_sum: AffinePoint = AffinePoint::identity();

    for i in 0..notes_in.len() {
        let ec_point = AffinePoint::from(&notes_in[i].address);
        pub_key_sum = &pub_key_sum + &ec_point;
    }

    let pub_key: EcPoint = EcPoint::from(&pub_key_sum);

    let valid = verify(&pub_key.x.to_biguint().unwrap(), order_hash, signature);
    return valid;
}

fn hash_note_escape_message(escape_id: u32, escape_notes: &Vec<Note>) -> BigUint {
    let mut hash_inputs: Vec<&BigUint> = Vec::new();

    let escape_id = BigUint::from_u32(escape_id).unwrap();
    hash_inputs.push(&escape_id);

    escape_notes
        .iter()
        .for_each(|note| hash_inputs.push(&note.hash));

    let order_hash = pedersen_on_vec(&hash_inputs);

    return order_hash;
}
