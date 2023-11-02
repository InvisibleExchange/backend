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
    order_tab::{close_tab::close_order_tab, open_tab::open_order_tab},
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

pub fn _split_notes_inner(
    state_tree: &Arc<Mutex<SuperficialTree>>,
    updated_state_hashes: &Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
    firebase_session: &Arc<Mutex<ServiceSession>>,
    backup_storage: &Arc<Mutex<BackupStorage>>,
    swap_output_json: &Arc<Mutex<Vec<serde_json::Map<String, Value>>>>,
    notes_in: Vec<Note>,
    mut new_note: Note,
    mut refund_note: Option<Note>,
) -> std::result::Result<Vec<u64>, String> {
    let token = notes_in[0].token;

    let mut sum_in: u64 = 0;

    let mut state_tree = state_tree.lock();
    for note in notes_in.iter() {
        if note.token != token {
            return Err("Invalid token".to_string());
        }

        let leaf_hash = state_tree.get_leaf_by_index(note.index);

        if leaf_hash != note.hash {
            return Err("Note does not exist".to_string());
        }

        sum_in += note.amount;
    }

    if new_note.token != token {
        return Err("Invalid token".to_string());
    }

    let note_in1 = &notes_in[0];
    if new_note.blinding != note_in1.blinding || new_note.address.x != note_in1.address.x {
        return Err("Mismatch od address and blinding between input/output notes".to_string());
    }
    let new_amount = new_note.amount;

    // ? get and set new index
    let new_index = state_tree.first_zero_idx();
    new_note.index = new_index;

    let mut new_indexes = vec![new_index];

    let mut refund_amount: u64 = 0;
    if refund_note.is_some() {
        let refund_note = refund_note.as_mut().unwrap();

        if refund_note.token != token {
            return Err("Invalid token".to_string());
        }

        let note_in2 = &notes_in[notes_in.len() - 1];
        if refund_note.blinding != note_in2.blinding || refund_note.address.x != note_in2.address.x
        {
            return Err("Mismatch of address and blinding between input/output notes".to_string());
        }

        refund_amount = refund_note.amount;

        let new_index = state_tree.first_zero_idx();
        refund_note.index = new_index;
        new_indexes.push(new_index)
    }

    if sum_in != new_amount + refund_amount {
        return Err("New note amounts exceed old note amounts".to_string());
    }

    // ? Remove notes in from state
    let mut updated_state_hashes = updated_state_hashes.lock();
    for note in notes_in.iter() {
        state_tree.update_leaf_node(&BigUint::zero(), note.index);
        updated_state_hashes.insert(note.index, (LeafNodeType::Note, BigUint::zero()));
    }

    // ? Add return in to state
    state_tree.update_leaf_node(&new_note.hash, new_note.index);
    updated_state_hashes.insert(new_note.index, (LeafNodeType::Note, new_note.hash.clone()));

    if let Some(note) = refund_note.as_ref() {
        state_tree.update_leaf_node(&note.hash, note.index);
        updated_state_hashes.insert(note.index, (LeafNodeType::Note, note.hash.clone()));
    }

    drop(updated_state_hashes);
    drop(state_tree);

    // ----------------------------------------------

    update_db_after_note_split(
        &firebase_session,
        &backup_storage,
        &notes_in,
        new_note.clone(),
        refund_note.clone(),
    );

    // ----------------------------------------------

    let mut json_map = serde_json::map::Map::new();
    json_map.insert(
        String::from("transaction_type"),
        serde_json::to_value("note_split").unwrap(),
    );
    json_map.insert(
            String::from("note_split"),
            json!({"token": token, "notes_in": notes_in, "new_note": new_note, "refund_note": refund_note}),
        );

    let mut swap_output_json = swap_output_json.lock();
    swap_output_json.push(json_map);
    drop(swap_output_json);

    Ok(new_indexes)
}

pub fn _change_position_margin_inner(
    state_tree: &Arc<Mutex<SuperficialTree>>,
    updated_state_hashes: &Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
    firebase_session: &Arc<Mutex<ServiceSession>>,
    backup_storage: &Arc<Mutex<BackupStorage>>,
    swap_output_json: &Arc<Mutex<Vec<serde_json::Map<String, Value>>>>,
    latest_index_price: &HashMap<u32, u64>,
    margin_change: ChangeMarginMessage,
) -> std::result::Result<(u64, PerpPosition), String> {
    let current_index_price = *latest_index_price
        .get(&margin_change.position.position_header.synthetic_token)
        .unwrap();

    verify_margin_change_signature(&margin_change)?;

    let mut position = margin_change.position.clone();
    verify_position_existence(&position, &state_tree)?;

    position.modify_margin(margin_change.margin_change)?;

    let leverage = position
        .get_current_leverage(current_index_price)
        .map_err(|e| e.to_string())?;

    // ? Check that leverage is valid relative to the notional position size after increasing size
    if get_max_leverage(
        position.position_header.synthetic_token,
        position.position_size,
    ) < leverage
    {
        println!(
            "Leverage would be too high {} > {}",
            leverage,
            get_max_leverage(
                position.position_header.synthetic_token,
                position.position_size
            ),
        );
        return Err("Leverage would be too high".to_string());
    }

    let mut z_index: u64 = 0;
    let mut valid: bool = true;
    if margin_change.margin_change >= 0 {
        let amount_in = margin_change
            .notes_in
            .as_ref()
            .unwrap()
            .iter()
            .fold(0, |acc, n| {
                if n.token != COLLATERAL_TOKEN {
                    valid = true;
                }
                return acc + n.amount;
            });
        let refund_amount = if margin_change.refund_note.is_some() {
            margin_change.refund_note.as_ref().unwrap().amount
        } else {
            0
        };

        if !valid {
            return Err("Invalid token".to_string());
        }
        if amount_in < margin_change.margin_change.abs() as u64 + refund_amount {
            return Err("Invalid amount in".to_string());
        }

        add_margin_state_updates(
            &state_tree,
            &updated_state_hashes,
            margin_change.notes_in.as_ref().unwrap(),
            margin_change.refund_note.clone(),
            position.index as u64,
            &position.hash.clone(),
        )?;

        let _handle =
            start_add_position_thread(position.clone(), &firebase_session, &backup_storage);

        let delete_notes = margin_change
            .notes_in
            .as_ref()
            .unwrap()
            .iter()
            .map(|n| (n.index, n.address.x.to_string()))
            .collect::<Vec<(u64, String)>>();
        let mut add_notes = vec![];
        if margin_change.refund_note.is_some() {
            add_notes.push(margin_change.refund_note.as_ref().unwrap());
        }

        let updater = DbNoteUpdater {
            session: &firebase_session,
            backup_storage: &backup_storage,
            delete_notes,
            add_notes,
        };

        let _handles = updater.update_db();
    } else {
        let mut tree = state_tree.lock();

        let index = tree.first_zero_idx();
        drop(tree);

        let return_collateral_note = Note::new(
            index,
            margin_change
                .close_order_fields
                .as_ref()
                .unwrap()
                .dest_received_address
                .clone(),
            COLLATERAL_TOKEN,
            margin_change.margin_change.abs() as u64,
            margin_change
                .close_order_fields
                .as_ref()
                .unwrap()
                .dest_received_blinding
                .clone(),
        );

        reduce_margin_state_updates(
            &state_tree,
            &updated_state_hashes,
            return_collateral_note.clone(),
            position.index as u64,
            &position.hash.clone(),
        );

        let _handle =
            start_add_position_thread(position.clone(), &firebase_session, &backup_storage);

        let _handle =
            start_add_note_thread(return_collateral_note, &firebase_session, &backup_storage);

        z_index = index;
    }

    // ----------------------------------------------

    let mut json_map = serde_json::map::Map::new();
    json_map.insert(
        String::from("transaction_type"),
        serde_json::to_value("margin_change").unwrap(),
    );
    json_map.insert(
        String::from("margin_change"),
        serde_json::to_value(margin_change).unwrap(),
    );
    json_map.insert(
        String::from("new_position_hash"),
        serde_json::to_value(position.hash.to_string()).unwrap(),
    );
    json_map.insert(
        String::from("zero_idx"),
        serde_json::to_value(z_index).unwrap(),
    );

    let mut swap_output_json = swap_output_json.lock();
    swap_output_json.push(json_map);
    drop(swap_output_json);

    Ok((z_index, position))
}

pub fn _execute_order_tab_modification_inner(
    state_tree: &Arc<Mutex<SuperficialTree>>,
    updated_state_hashes: &Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
    firebase_session: &Arc<Mutex<ServiceSession>>,
    backup_storage: &Arc<Mutex<BackupStorage>>,
    swap_output_json: &Arc<Mutex<Vec<serde_json::Map<String, Value>>>>,
    latest_index_price: &HashMap<u32, u64>,
    tab_action_message: OrderTabActionMessage,
) -> JoinHandle<OrderTabActionResponse> {
    let state_tree = state_tree.clone();
    let updated_state_hashes = updated_state_hashes.clone();
    let session = firebase_session.clone();
    let backup_storage = backup_storage.clone();
    let swap_output_json = swap_output_json.clone();
    let latest_index_price = latest_index_price.clone();

    let handle = thread::spawn(move || {
        if tab_action_message.open_order_tab_req.is_some() {
            let open_order_tab_req = tab_action_message.open_order_tab_req.unwrap();

            let new_order_tab = open_order_tab(
                &session,
                &backup_storage,
                open_order_tab_req,
                &state_tree,
                &updated_state_hashes,
                &swap_output_json,
            );

            let order_tab_action_response = OrderTabActionResponse {
                open_tab_response: Some(new_order_tab),
                close_tab_response: None,
                add_liq_response: None,
                register_mm_response: None,
                remove_liq_response: None,
            };

            return order_tab_action_response;
        } else if tab_action_message.close_order_tab_req.is_some() {
            let close_order_tab_req = tab_action_message.close_order_tab_req.unwrap();

            let close_tab_response = close_order_tab(
                &session,
                &backup_storage,
                &state_tree,
                &updated_state_hashes,
                &swap_output_json,
                close_order_tab_req,
            );

            let order_tab_action_response = OrderTabActionResponse {
                open_tab_response: None,
                close_tab_response: Some(close_tab_response),
                add_liq_response: None,
                register_mm_response: None,
                remove_liq_response: None,
            };

            return order_tab_action_response;
        } else if tab_action_message.onchain_register_mm_req.is_some() {
            let register_mm_req = tab_action_message.onchain_register_mm_req.unwrap();

            let index_price = *latest_index_price
                .get(&register_mm_req.base_token)
                .unwrap_or(&0);

            let register_mm_response = onchain_register_mm(
                &session,
                &backup_storage,
                register_mm_req,
                &state_tree,
                &updated_state_hashes,
                &swap_output_json,
                index_price,
            );

            let order_tab_action_response = OrderTabActionResponse {
                open_tab_response: None,
                close_tab_response: None,
                add_liq_response: None,
                register_mm_response: Some(register_mm_response),
                remove_liq_response: None,
            };

            return order_tab_action_response;
        } else if tab_action_message.onchain_add_liq_req.is_some() {
            let add_liquidity_req = tab_action_message.onchain_add_liq_req.unwrap();

            let index_price = *latest_index_price
                .get(&add_liquidity_req.base_token)
                .unwrap_or(&0);

            let result = add_liquidity_to_mm(
                &session,
                &backup_storage,
                add_liquidity_req,
                &state_tree,
                &updated_state_hashes,
                &swap_output_json,
                index_price,
            );

            let order_tab_action_response = OrderTabActionResponse {
                open_tab_response: None,
                close_tab_response: None,
                add_liq_response: Some(result),
                register_mm_response: None,
                remove_liq_response: None,
            };

            return order_tab_action_response;
        } else {
            let remove_liquidity_req = tab_action_message.onchain_remove_liq_req.unwrap();

            let index_price = *latest_index_price
                .get(&remove_liquidity_req.base_token)
                .unwrap_or(&0);

            let remove_liq_response = remove_liquidity_from_order_tab(
                &session,
                &backup_storage,
                remove_liquidity_req,
                &state_tree,
                &updated_state_hashes,
                &swap_output_json,
                index_price,
            );

            let order_tab_action_response = OrderTabActionResponse {
                open_tab_response: None,
                close_tab_response: None,
                add_liq_response: None,
                register_mm_response: None,
                remove_liq_response: Some(remove_liq_response),
            };

            return order_tab_action_response;
        }
    });

    return handle;
}
