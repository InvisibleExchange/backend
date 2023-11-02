use std::{collections::HashMap, sync::Arc, thread::JoinHandle};

use firestore_db_and_auth::ServiceSession;
use parking_lot::Mutex;

use crate::{
    order_tab::OrderTab,
    transactions::limit_order::LimitOrder,
    utils::{
        notes::Note,
        storage::firestore::{
            start_add_fill_thread, start_add_note_thread, start_add_order_tab_thread,
            start_delete_deposit_thread, start_delete_note_thread,
        },
        storage::local_storage::BackupStorage,
    },
};

use super::{swap_helpers::NoteInfoExecutionOutput, transaction_output::FillInfo};

// SWAPS ----------------------------------------------------

/// Remove the spent notes from the database and add the new ones as well as the refund and pfr notes (if necessary)
///
pub fn update_db_after_spot_swap(
    session: &Arc<Mutex<ServiceSession>>,
    backup_storage: &Arc<Mutex<BackupStorage>>,
    order: &LimitOrder,
    note_info_output: &Option<NoteInfoExecutionOutput>,
    updated_order_tab: &Option<OrderTab>,
) {
    let mut delete_notes: Vec<(u64, String)> = Vec::new();
    let mut add_notes: Vec<&Note> = Vec::new();

    if order.spot_note_info.is_some() {
        let (add_notes_, delete_notes_) =
            _update_non_tab_order(add_notes, delete_notes, order, note_info_output);
        add_notes = add_notes_;
        delete_notes = delete_notes_;
    } else {
        let order_tab = updated_order_tab.as_ref().unwrap().clone();

        let _h = start_add_order_tab_thread(order_tab, session, backup_storage);
    }

    let updater = DbNoteUpdater {
        session,
        backup_storage,
        delete_notes,
        add_notes,
    };

    let _handles = updater.update_db();
}

fn _update_non_tab_order<'a>(
    mut add_notes: Vec<&'a Note>,
    mut delete_notes: Vec<(u64, String)>,
    order: &'a LimitOrder,
    note_info_output: &'a Option<NoteInfoExecutionOutput>,
) -> (Vec<&'a Note>, Vec<(u64, String)>) {
    let spot_note_info = order.spot_note_info.as_ref().unwrap();
    let prev_pfr_note = &note_info_output
        .as_ref()
        .unwrap()
        .prev_partial_fill_refund_note;
    let swap_note = &note_info_output.as_ref().unwrap().swap_note;
    let new_pfr_note = &note_info_output.as_ref().unwrap().new_partial_fill_info;

    let is_first_fill = prev_pfr_note.is_none();

    // ? Delete notes spent from the database
    if is_first_fill {
        if spot_note_info.refund_note.is_some() {
            // ? Store the refund note in place of the first note
            add_notes.push(&spot_note_info.refund_note.as_ref().unwrap())
        }

        // ? Delete all notes in
        for n in spot_note_info.notes_in.iter() {
            delete_notes.push((n.index, n.address.x.to_string()))
        }
    }

    // ? Delete prev partially fill refund notes (*if necessary)
    if prev_pfr_note.is_some() {
        let n = prev_pfr_note.as_ref().unwrap();
        delete_notes.push((n.index, n.address.x.to_string()));
    }

    // ? Store swap notes
    add_notes.push(&swap_note);

    // ? Store partial fill refund notes if order was partially filled
    if new_pfr_note.is_some() {
        add_notes.push(&new_pfr_note.as_ref().unwrap().0.as_ref().unwrap());
    }

    return (add_notes, delete_notes);
}

pub fn store_spot_fill(
    session: &Arc<Mutex<ServiceSession>>,
    backup_storage: &Arc<Mutex<BackupStorage>>,
    amount: u64,
    price: u64,
    user_id_a: u64,
    user_id_b: u64,
    base_token: u32,
    quote_token: u32,
    is_buy: bool,
    timestamp: u64,
) {
    let fill_info = FillInfo {
        amount,
        price,
        user_id_a: user_id_a.to_string(),
        user_id_b: user_id_b.to_string(),
        base_token,
        quote_token,
        timestamp,
        is_buy,
    };

    let _handle = start_add_fill_thread(fill_info, session, backup_storage);
}

// DEPOSITS -----------------------------------------------------

/// Add all the newly generated deposit notes (in most cases only one) to the database
pub fn update_db_after_deposit(
    session: &Arc<Mutex<ServiceSession>>,
    backup_storage: &Arc<Mutex<BackupStorage>>,
    new_notes: Vec<Note>,
    zero_indexes: &Vec<u64>,
    deposit_id: u64,
) {
    let mut _handles = Vec::new();

    for (mut note, z_idx) in new_notes.into_iter().zip(zero_indexes.iter()) {
        note.index = *z_idx;
        let handle = start_add_note_thread(note.clone(), session, backup_storage);
        _handles.push(handle);
    }

    let h = start_delete_deposit_thread(deposit_id, session);
    _handles.push(h);
}

// WITHDRAWAL ----------------------------------------------------

/// Remove the withdrawn notes from the database and add the refund note (if necessary)
pub fn update_db_after_withdrawal(
    session: &Arc<Mutex<ServiceSession>>,
    backup_storage: &Arc<Mutex<BackupStorage>>,
    notes_in: &Vec<Note>,
    refund_note: Option<Note>,
) {
    let mut delete_notes: Vec<(u64, String)> = Vec::new();
    let mut add_notes: Vec<&Note> = Vec::new();

    if refund_note.is_some() {
        // ? Store the refund note in place of the first note
        add_notes.push(&refund_note.as_ref().unwrap())
    }

    for n in notes_in.into_iter() {
        delete_notes.push((n.index, n.address.x.to_string()))
    }

    let updater = DbNoteUpdater {
        session,
        backup_storage,
        delete_notes,
        add_notes,
    };

    let _handles = updater.update_db();
}

// NOTE SPLITS -----------------------------------------------------
/// Remove the old notes from the database and add the new ones
pub fn update_db_after_note_split(
    session: &Arc<Mutex<ServiceSession>>,
    backup_storage: &Arc<Mutex<BackupStorage>>,
    notes_in: &Vec<Note>,
    new_note: Note,
    refund_note: Option<Note>,
) {
    let mut delete_notes: Vec<(u64, String)> = Vec::new();
    let mut add_notes: Vec<Note> = Vec::new();

    for note in notes_in {
        delete_notes.push((note.index, note.address.x.to_string()))
    }

    add_notes.push(new_note.clone());
    if let Some(n) = refund_note {
        add_notes.push(n);
    }

    let add_notes = add_notes.iter().collect::<Vec<&Note>>();

    let updater = DbNoteUpdater {
        session,
        backup_storage,
        delete_notes,
        add_notes,
    };

    let _handles = updater.update_db();
}

pub struct DbNoteUpdater<'a> {
    pub session: &'a Arc<Mutex<ServiceSession>>,
    pub backup_storage: &'a Arc<Mutex<BackupStorage>>,
    pub delete_notes: Vec<(u64, String)>,
    pub add_notes: Vec<&'a Note>,
}

impl DbNoteUpdater<'_> {
    pub fn update_db(&self) -> Vec<JoinHandle<()>> {
        let mut _handles: Vec<_> = Vec::new();
        let mut added_notes = HashMap::new();

        for note in self.add_notes.iter() {
            let handle = start_add_note_thread((*note).clone(), self.session, self.backup_storage);
            _handles.push(handle);
            added_notes.insert((note.index, note.address.x.to_string()), false);
        }

        for deletion in self.delete_notes.iter() {
            if added_notes.contains_key(&deletion) {
                continue;
            }

            let handle = start_delete_note_thread(
                self.session,
                self.backup_storage,
                deletion.1.to_string(),
                deletion.0.to_string(),
            );
            _handles.push(handle);
        }

        return _handles;
    }
}
