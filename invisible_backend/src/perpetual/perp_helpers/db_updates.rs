use std::sync::Arc;

use firestore_db_and_auth::ServiceSession;
use parking_lot::Mutex;

use crate::utils::storage::backup_storage::BackupStorage;
use crate::{
    perpetual::{perp_order::PerpOrder, perp_position::PerpPosition, PositionEffectType},
    transactions::transaction_helpers::{
        db_updates::DbNoteUpdater, transaction_output::PerpFillInfo,
    },
    utils::{
        notes::Note,
        storage::firestore::{
            start_add_perp_fill_thread, start_add_position_thread, start_delete_position_thread,
        },
    },
};

pub fn update_db_after_perp_swap(
    session: &Arc<Mutex<ServiceSession>>,
    backup_storage: &Arc<Mutex<BackupStorage>>,
    order: &PerpOrder,
    prev_pfr_note: &Option<Note>,
    new_pfr_note: &Option<Note>,
    return_collateral_note: &Option<Note>,
    position: &Option<PerpPosition>,
) {
    let mut delete_notes: Vec<(u64, String)> = Vec::new();
    let mut add_notes: Vec<&Note> = Vec::new();

    let mut position_handles = Vec::new();

    // ? Remove the notes spent from the database if necessary ==============================================================
    if order.position_effect_type == PositionEffectType::Open {
        let is_first_fill = prev_pfr_note.is_none();

        if is_first_fill {
            // ? Store refund note (*if necessary) -----------------------------------------
            let refund_note = &order.open_order_fields.as_ref().unwrap().refund_note;
            if refund_note.is_some() {
                // ? Store the refund note in place of the first note
                add_notes.push(&refund_note.as_ref().unwrap())
            }

            // ? Delete all notes in
            for n in order.open_order_fields.as_ref().unwrap().notes_in.iter() {
                // let tup = (n.index, n.address.x.to_string());
                delete_notes.push((n.index, n.address.x.to_string()))
            }
        } else {
            let n: &Note = prev_pfr_note.as_ref().unwrap();
            delete_notes.push((n.index, n.address.x.to_string()));
        }

        // ? store partial fill refund notes (if necessary)
        if new_pfr_note.is_some() {
            add_notes.push(&new_pfr_note.as_ref().unwrap());
        }
    }

    // ? Store the return collateral note and remove closed positions (when necessary)
    if order.position_effect_type == PositionEffectType::Close {
        //
        add_notes.push(&return_collateral_note.as_ref().unwrap());

        if position.is_none() {
            let handle = start_delete_position_thread(
                session,
                backup_storage,
                order
                    .position
                    .as_ref()
                    .unwrap()
                    .position_header
                    .position_address
                    .to_string(),
                order.position.as_ref().unwrap().index.to_string(),
            );
            position_handles.push(handle);
        }
    }

    // ? Store the updated position (if necessary)
    if position.is_some() {
        let handle =
            start_add_position_thread(position.as_ref().unwrap().clone(), session, backup_storage);
        position_handles.push(handle);
    }

    let updater = DbNoteUpdater {
        session,
        backup_storage,
        delete_notes,
        add_notes,
    };

    let _handles = updater.update_db();
}

// Store perp fill

pub fn store_perp_fill(
    session: &Arc<Mutex<ServiceSession>>,
    backup_storage: &Arc<Mutex<BackupStorage>>,
    amount: u64,
    price: u64,
    user_id_a: u64,
    user_id_b: u64,
    synthetic_token: u32,
    is_buy: bool,
    timestamp: u64,
) {
    let fill_info = PerpFillInfo {
        amount,
        price,
        user_id_a: user_id_a.to_string(),
        user_id_b: user_id_b.to_string(),
        timestamp,
        synthetic_token,
        is_buy,
    };

    let _handle = start_add_perp_fill_thread(fill_info, session, backup_storage);
}
