use std::sync::Arc;

use parking_lot::Mutex;

use crate::{
    perpetual::perp_order::CloseOrderFields,
    transaction_batch::TxOutputJson,
    utils::{crypto_utils::Signature, notes::Note},
};

use super::OrderTab;

// * OPEN ORDER TAB JSON OUTPUT
pub fn open_tab_json_output(
    transaction_output_json_m: &Arc<Mutex<TxOutputJson>>,
    base_notes_in: &Vec<Note>,
    base_refund_note: &Option<Note>,
    quote_notes_in: &Vec<Note>,
    quote_refund_note: &Option<Note>,
    add_only: bool,
    prev_order_tab: &Option<OrderTab>, // & prev_order_tab in case of modify and new_order_tab in case of open
    new_order_tab: &OrderTab,
    signature: &Signature,
) {
    let mut json_map = serde_json::map::Map::new();
    json_map.insert(
        String::from("transaction_type"),
        serde_json::to_value(&"open_order_tab").unwrap(),
    );
    json_map.insert(
        String::from("is_onchain_interaction"),
        serde_json::to_value(&false).unwrap(),
    );
    json_map.insert(
        String::from("base_notes_in"),
        serde_json::to_value(&base_notes_in).unwrap(),
    );
    json_map.insert(
        String::from("base_refund_note"),
        serde_json::to_value(&base_refund_note).unwrap(),
    );
    json_map.insert(
        String::from("quote_notes_in"),
        serde_json::to_value(&quote_notes_in).unwrap(),
    );
    json_map.insert(
        String::from("quote_refund_note"),
        serde_json::to_value(&quote_refund_note).unwrap(),
    );
    json_map.insert(
        String::from("add_only"),
        serde_json::to_value(&add_only).unwrap(),
    );
    json_map.insert(
        String::from("order_tab"),
        serde_json::to_value(if add_only {
            prev_order_tab.as_ref().unwrap()
        } else {
            &new_order_tab
        })
        .unwrap(),
    );
    json_map.insert(
        String::from("updated_tab_hash"),
        serde_json::to_value(&new_order_tab.hash.to_string()).unwrap(),
    );
    json_map.insert(
        String::from("signature"),
        serde_json::to_value(&signature).unwrap(),
    );

    let mut transaction_output_json = transaction_output_json_m.lock();
    transaction_output_json.tx_micro_batch.push(json_map);
    drop(transaction_output_json);
}

// * CLOSE ORDER TAB JSON OUTPUT
pub fn close_tab_json_output(
    transaction_output_json_m: &Arc<Mutex<TxOutputJson>>,
    base_amount_change: u64,
    quote_amount_change: u64,
    base_return_note: &Note,
    quote_return_note: &Note,
    base_close_order_fields: &CloseOrderFields,
    quote_close_order_fields: &CloseOrderFields,
    prev_order_tab: &OrderTab,
    updated_order_tab: &Option<OrderTab>,
    signature: &Signature,
) {
    let mut json_map = serde_json::map::Map::new();
    json_map.insert(
        String::from("transaction_type"),
        serde_json::to_value(&"close_order_tab").unwrap(),
    );
    json_map.insert(
        String::from("is_onchain_interaction"),
        serde_json::to_value(&false).unwrap(),
    );
    json_map.insert(
        String::from("base_return_note_idx"),
        serde_json::to_value(&base_return_note.index).unwrap(),
    );
    json_map.insert(
        String::from("base_return_note_hash"),
        serde_json::to_value(&base_return_note.hash.to_string()).unwrap(),
    );
    json_map.insert(
        String::from("quote_return_note_idx"),
        serde_json::to_value(&quote_return_note.index).unwrap(),
    );
    json_map.insert(
        String::from("quote_return_note_hash"),
        serde_json::to_value(&quote_return_note.hash.to_string()).unwrap(),
    );
    json_map.insert(
        String::from("base_amount_change"),
        serde_json::to_value(&base_amount_change).unwrap(),
    );
    json_map.insert(
        String::from("quote_amount_change"),
        serde_json::to_value(&quote_amount_change).unwrap(),
    );
    json_map.insert(
        String::from("base_close_order_fields"),
        serde_json::to_value(&base_close_order_fields).unwrap(),
    );
    json_map.insert(
        String::from("quote_close_order_fields"),
        serde_json::to_value(&quote_close_order_fields).unwrap(),
    );
    json_map.insert(
        String::from("order_tab"),
        serde_json::to_value(&prev_order_tab).unwrap(),
    );
    let updated_tab_hash = if updated_order_tab.is_some() {
        updated_order_tab.as_ref().unwrap().hash.to_string()
    } else {
        "0".to_string()
    };
    json_map.insert(
        String::from("updated_tab_hash"),
        serde_json::to_value(&updated_tab_hash).unwrap(),
    );
    json_map.insert(
        String::from("signature"),
        serde_json::to_value(&signature).unwrap(),
    );

    let mut transaction_output_json = transaction_output_json_m.lock();
    transaction_output_json.tx_micro_batch.push(json_map);
    drop(transaction_output_json);
}
