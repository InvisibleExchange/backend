use firestore_db_and_auth::ServiceSession;
use num_bigint::BigUint;
use num_traits::Zero;
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::Arc,
    thread::{self, JoinHandle},
};

use crate::{
    order_tab::{close_tab::close_order_tab, open_tab::open_order_tab, OrderTab},
    perpetual::{
        perp_helpers::perp_swap_helpers::get_max_leverage, perp_position::PerpPosition,
        COLLATERAL_TOKEN,
    },
    server::grpc::{OrderTabActionMessage, OrderTabActionResponse},
    smart_contract_mms::{
        add_liquidity::add_liquidity_to_mm, register_mm::onchain_register_mm,
        remove_liquidity::remove_liquidity_from_order_tab,
    },
    transaction_batch::LeafNodeType,
    transactions::transaction_helpers::db_updates::{update_db_after_note_split, DbNoteUpdater},
    utils::storage::firestore::{start_add_note_thread, start_add_position_thread},
};
use crate::{
    trees::superficial_tree::SuperficialTree, utils::storage::local_storage::BackupStorage,
};

use crate::utils::notes::Note;

use crate::server::{
    grpc::ChangeMarginMessage,
    server_helpers::engine_helpers::{verify_margin_change_signature, verify_position_existence},
};

use crate::transaction_batch::tx_batch_helpers::{
    add_margin_state_updates, reduce_margin_state_updates,
};

// struct NoteEscape {
//     uint32 escapeId;
//     uint32 timestamp;
//     Note[] notes;
// }
// struct PositionEscape {
//     uint32 escapeId;
//     uint32 timestamp;
//     Position[] positions;
// }
// struct OrderTabEscape {
//     uint32 escapeId;
//     uint32 timestamp;
//     OrderTab[] orderTabs;
// }

// pub fn _verify_escape(
//     state_tree: &Arc<Mutex<SuperficialTree>>,
//     updated_state_hashes: &Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
//     firebase_session: &Arc<Mutex<ServiceSession>>,
//     backup_storage: &Arc<Mutex<BackupStorage>>,
//     swap_output_json: &Arc<Mutex<Vec<serde_json::Map<String, Value>>>>,
//     note_escapes: (&Vec<Note>, u32),
//     position_escapes: (&Vec<PerpPosition>, u32),
//     order_tab_escapes: (&Vec<OrderTab>, u32),
// ) -> std::result::Result<Vec<u64>, String> {
//     // ? get the note/position/orderTab escapes from the state tree

//     let mut state_tree_m = state_tree.lock();

//     // ? Loop over each escape and verfiy or reject it
//     for note in note_escapes.0 {

//         if state_tree_m.get_leaf_by_index(note.index) == note.hash {

//             let mut updated_state_hashes_m = updated_state_hashes.lock();
//             updated_state_hashes_m.insert(note.index, (LeafNodeType::Note, note.hash.clone()));

//         } else {

//         }

//            // ? If verified, update the state tree and return the escape ids

//     // ? If rejected, return the escape ids

//     // ? Get the merkle proofs for the escapes

//     }

// }
