use std::sync::Arc;

use firestore_db_and_auth::ServiceSession;
use parking_lot::Mutex;

use crate::{
    perpetual::perp_position::PerpPosition,
    transactions::transaction_helpers::db_updates::DbNoteUpdater,
    utils::{
        notes::Note,
        storage::firestore::{start_add_position_thread, start_delete_position_thread},
        storage::local_storage::BackupStorage,
    },
};

use super::liquidation_order::LiquidationOrder;

pub fn update_db_after_liquidation_swap(
    session: &Arc<Mutex<ServiceSession>>,
    backup_storage: &Arc<Mutex<BackupStorage>>,
    liquidation_order: &LiquidationOrder,
    liquidated_position: &Option<PerpPosition>,
    new_position: &PerpPosition,
) {
    let mut delete_notes: Vec<(u64, String)> = Vec::new();
    let mut add_notes: Vec<&Note> = Vec::new();

    let mut position_handles = Vec::new();

    // ? Store refund note (*if necessary) -----------------------------------------
    let refund_note = &liquidation_order.open_order_fields.refund_note;
    if refund_note.is_some() {
        // ? Store the refund note in place of the first note
        add_notes.push(&refund_note.as_ref().unwrap())
    }

    // ? Remove the notes spent from the database -----------------------------------------
    let notes_in = &liquidation_order.open_order_fields.notes_in;
    for n in notes_in.iter() {
        delete_notes.push((n.index, n.address.x.to_string()))
    }

    // ? Update/Remove Liquidated position from database -----------------------------------------
    if liquidated_position.is_some() {
        let handle = start_add_position_thread(
            liquidated_position.as_ref().unwrap().clone(),
            session,
            backup_storage,
        );
        position_handles.push(handle);
    } else {
        let handle = start_delete_position_thread(
            session,
            backup_storage,
            liquidation_order
                .position
                .position_header
                .position_address
                .to_string(),
            liquidation_order.position.index.to_string(),
        );
        position_handles.push(handle);
    }

    // ? Store new position in database -----------------------------------------
    let handle = start_add_position_thread(new_position.clone(), session, &backup_storage);
    position_handles.push(handle);

    let updater = DbNoteUpdater {
        session,
        backup_storage,
        delete_notes,
        add_notes,
    };

    let _handles = updater.update_db();
}
