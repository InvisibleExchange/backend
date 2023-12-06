use std::sync::Arc;

use num_bigint::BigUint;
use parking_lot::Mutex;

use firestore_db_and_auth::ServiceSession;

use crate::{
    order_tab::OrderTab,
    perpetual::perp_position::PerpPosition,
    smart_contract_mms::vlp_note::VlpNote,
    utils::{
        notes::Note,
        storage::firestore::{
            start_add_note_thread, start_add_order_tab_thread, start_add_position_thread,
            start_add_vlp_note_thread, start_delete_note_thread, start_delete_order_tab_thread,
        },
        storage::local_storage::BackupStorage,
    },
};

// * ONCHAIN INTERACTIONS ===========================================================================
// * ================================================================================================

/// Update the database after a new order tab has been opened.
pub fn onchain_open_tab_db_updates(
    session: &Arc<Mutex<ServiceSession>>,
    backup_storage: &Arc<Mutex<BackupStorage>>,
    order_tab: Option<OrderTab>,
    position: Option<PerpPosition>,
    vlp_note: VlpNote,
) {
    //

    let _h = start_add_vlp_note_thread(vlp_note, session, backup_storage);

    if let Some(tab) = order_tab {
        let _h: std::thread::JoinHandle<()> =
            start_add_order_tab_thread(tab, session, backup_storage);
    }
    if let Some(pos) = position {
        let _h: std::thread::JoinHandle<()> =
            start_add_position_thread(pos, session, backup_storage);
    }
}

// * ================================================================================================
// * ADD LIQUIDITY * //

/// Update the database after a new order tab has been opened.
pub fn onchain_tab_add_liquidity_db_updates(
    session: &Arc<Mutex<ServiceSession>>,
    backup_storage: &Arc<Mutex<BackupStorage>>,
    order_tab: OrderTab,
    base_notes_in: Vec<Note>,
    quote_notes_in: Vec<Note>,
    base_refund_note: Option<Note>,
    quote_refund_note: Option<Note>,
    vlp_note: Note,
) {
    //

    for note in base_notes_in.into_iter() {
        let _h = start_delete_note_thread(
            session,
            backup_storage,
            note.address.x.to_string(),
            note.index.to_string(),
        );
    }
    for note in quote_notes_in.into_iter() {
        let _h = start_delete_note_thread(
            session,
            backup_storage,
            note.address.x.to_string(),
            note.index.to_string(),
        );
    }
    if let Some(note) = base_refund_note {
        let _h = start_add_note_thread(note, session, backup_storage);
    }
    if let Some(note) = quote_refund_note {
        let _h = start_add_note_thread(note, session, backup_storage);
    }

    let _h = start_add_note_thread(vlp_note, session, backup_storage);

    let _h: std::thread::JoinHandle<()> =
        start_add_order_tab_thread(order_tab, session, backup_storage);
}

/// Update the database after a new order tab has been opened.
pub fn onchain_position_add_liquidity_db_updates(
    session: &Arc<Mutex<ServiceSession>>,
    backup_storage: &Arc<Mutex<BackupStorage>>,
    position: PerpPosition,
    collateral_notes_in: Vec<Note>,
    collateral_refund_note: Option<Note>,
    vlp_note: VlpNote,
) {
    //

    for note in collateral_notes_in.into_iter() {
        let _h = start_delete_note_thread(
            session,
            backup_storage,
            note.address.x.to_string(),
            note.index.to_string(),
        );
    }
    if let Some(note) = collateral_refund_note {
        let _h = start_add_note_thread(note, session, backup_storage);
    }

    let _h = start_add_vlp_note_thread(vlp_note, session, backup_storage);

    let _h: std::thread::JoinHandle<()> =
        start_add_position_thread(position, session, backup_storage);
}

// * ================================================================================================
// * REMOVE LIQUIDITY * //

/// Update the database after a new order tab has been opened.
pub fn onchain_tab_remove_liquidity_db_updates(
    session: &Arc<Mutex<ServiceSession>>,
    backup_storage: &Arc<Mutex<BackupStorage>>,
    tab_idx: u64,
    tab_address: BigUint,
    order_tab: Option<OrderTab>,
    vlp_notes_in: &Vec<Note>,
    base_return_note: Note,
    quote_return_note: Note,
) {
    //

    for note in vlp_notes_in {
        let _h = start_delete_note_thread(
            session,
            backup_storage,
            note.address.x.to_string(),
            note.index.to_string(),
        );
    }

    let _h = start_add_note_thread(base_return_note, session, backup_storage);
    let _h = start_add_note_thread(quote_return_note, session, backup_storage);

    if let Some(tab) = order_tab {
        let _h: std::thread::JoinHandle<()> =
            start_add_order_tab_thread(tab, session, backup_storage);
    } else {
        let _h: std::thread::JoinHandle<()> = start_delete_order_tab_thread(
            session,
            backup_storage,
            tab_address.to_string(),
            tab_idx.to_string(),
        );
    }
}

/// Update the database after a new order tab has been opened.
pub fn onchain_position_remove_liquidity_db_updates(
    session: &Arc<Mutex<ServiceSession>>,
    backup_storage: &Arc<Mutex<BackupStorage>>,
    pos_idx: u64,
    pos_address: BigUint,
    position: Option<PerpPosition>,
    vlp_notes_in: &Vec<VlpNote>,
    collateral_return_note: Note,
) {
    //

    for note in vlp_notes_in {
        let _h = start_delete_note_thread(
            session,
            backup_storage,
            note.address.x.to_string(),
            note.index.to_string(),
        );
    }

    let _h = start_add_note_thread(collateral_return_note, session, backup_storage);

    if let Some(pos) = position {
        let _h: std::thread::JoinHandle<()> =
            start_add_position_thread(pos, session, backup_storage);
    } else {
        let _h: std::thread::JoinHandle<()> = start_delete_order_tab_thread(
            session,
            backup_storage,
            pos_address.to_string(),
            pos_idx.to_string(),
        );
    }
}
