use std::sync::Arc;

use num_bigint::BigUint;
use parking_lot::Mutex;
use serde_json::Value;

use crate::{
    order_tab::OrderTab,
    perpetual::{perp_order::CloseOrderFields, perp_position::PerpPosition},
    smart_contract_mms::vlp_note::VlpNote,
    utils::{crypto_utils::Signature, notes::Note},
};

// * ONCHAIN OPEN ORDER TAB JSON OUTPUT
pub fn onchain_register_json_output(
    swap_output_json_m: &Arc<Mutex<Vec<serde_json::Map<String, Value>>>>,
    prev_order_tab: &Option<OrderTab>,
    new_order_tab: &Option<OrderTab>,
    prev_position: &Option<PerpPosition>,
    new_position: &Option<PerpPosition>,
    vlp_close_order_fields: &CloseOrderFields,
    vlp_note: &VlpNote,
    max_vlp_supply: u64,
    index_price: u64,
    signature: &Signature,
) {
    let mut json_map = serde_json::map::Map::new();
    json_map.insert(
        String::from("transaction_type"),
        serde_json::to_value(&"onchain_register_mm").unwrap(),
    );
    json_map.insert(
        String::from("is_order_tab"),
        serde_json::to_value(&prev_order_tab.is_some()).unwrap(),
    );
    // ----------
    json_map.insert(
        String::from("prev_position"),
        serde_json::to_value(prev_position).unwrap(),
    );
    json_map.insert(
        String::from("new_position_hash"),
        serde_json::to_value(new_position.as_ref().map(|p| p.hash.to_string())).unwrap(),
    );
    json_map.insert(
        String::from("prev_order_tab"),
        serde_json::to_value(prev_order_tab).unwrap(),
    );
    json_map.insert(
        String::from("new_order_tab_hash"),
        serde_json::to_value(new_order_tab.as_ref().map(|t| t.hash.to_string())).unwrap(),
    );
    // ----------
    json_map.insert(
        String::from("vlp_close_order_fields"),
        serde_json::to_value(&vlp_close_order_fields).unwrap(),
    );
    json_map.insert(
        String::from("vlp_token"),
        serde_json::to_value(&vlp_note.token).unwrap(),
    );
    json_map.insert(
        String::from("vlp_note_idx"),
        serde_json::to_value(&vlp_note.index).unwrap(),
    );
    json_map.insert(
        String::from("vlp_note_hash"),
        serde_json::to_value(&vlp_note.hash.to_string()).unwrap(),
    );
    json_map.insert(
        String::from("max_vlp_supply"),
        serde_json::to_value(&max_vlp_supply).unwrap(),
    );
    json_map.insert(
        String::from("index_price"),
        serde_json::to_value(&index_price).unwrap(),
    );
    json_map.insert(
        String::from("signature"),
        serde_json::to_value(&signature).unwrap(),
    );

    let mut swap_output_json = swap_output_json_m.lock();
    swap_output_json.push(json_map);
    drop(swap_output_json);
}

// * ================================================================================================
// * ADD LIQUIDITY * //

pub fn onchain_tab_add_liquidity_json_output(
    swap_output_json_m: &Arc<Mutex<Vec<serde_json::Map<String, Value>>>>,
    prev_order_tab: &OrderTab,
    base_notes_in: &Vec<Note>,
    base_refund_note: &Option<Note>,
    quote_notes_in: &Vec<Note>,
    quote_refund_note: &Option<Note>,
    new_order_tab_hash: &BigUint,
    vlp_close_order_fields: &CloseOrderFields,
    vlp_note: &Note,
    index_price: u64,
    signature: &Signature,
) {
    let mut json_map = serde_json::map::Map::new();
    json_map.insert(
        String::from("transaction_type"),
        serde_json::to_value(&"add_liquidity").unwrap(),
    );
    json_map.insert(
        String::from("is_order_tab"),
        serde_json::to_value(&true).unwrap(),
    );
    json_map.insert(
        String::from("prev_order_tab"),
        serde_json::to_value(prev_order_tab).unwrap(),
    );
    json_map.insert(
        String::from("new_order_tab_hash"),
        serde_json::to_value(&new_order_tab_hash.to_string()).unwrap(),
    );
    json_map.insert(
        String::from("vlp_close_order_fields"),
        serde_json::to_value(&vlp_close_order_fields).unwrap(),
    );
    //
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
    //
    json_map.insert(
        String::from("vlp_note_idx"),
        serde_json::to_value(&vlp_note.index).unwrap(),
    );
    json_map.insert(
        String::from("vlp_note_hash"),
        serde_json::to_value(&vlp_note.hash.to_string()).unwrap(),
    );
    //
    json_map.insert(
        String::from("index_price"),
        serde_json::to_value(&index_price).unwrap(),
    );
    json_map.insert(
        String::from("signature"),
        serde_json::to_value(&signature).unwrap(),
    );

    let mut swap_output_json = swap_output_json_m.lock();
    swap_output_json.push(json_map);
    drop(swap_output_json);
}

pub fn onchain_position_add_liquidity_json_output(
    swap_output_json_m: &Arc<Mutex<Vec<serde_json::Map<String, Value>>>>,
    prev_position: &PerpPosition,
    collateral_notes_in: &Vec<Note>,
    collateral_refund_note: &Option<Note>,
    new_position_hash: &BigUint,
    vlp_close_order_fields: &CloseOrderFields,
    vlp_note: &VlpNote,
    signature: &Signature,
) {
    let mut json_map = serde_json::map::Map::new();
    json_map.insert(
        String::from("transaction_type"),
        serde_json::to_value(&"add_liquidity").unwrap(),
    );
    json_map.insert(
        String::from("is_order_tab"),
        serde_json::to_value(&false).unwrap(),
    );
    json_map.insert(
        String::from("prev_position"),
        serde_json::to_value(prev_position).unwrap(),
    );
    json_map.insert(
        String::from("new_position_hash"),
        serde_json::to_value(&new_position_hash.to_string()).unwrap(),
    );
    json_map.insert(
        String::from("vlp_close_order_fields"),
        serde_json::to_value(&vlp_close_order_fields).unwrap(),
    );
    //
    json_map.insert(
        String::from("collateral_notes_in"),
        serde_json::to_value(&collateral_notes_in).unwrap(),
    );
    json_map.insert(
        String::from("collateral_refund_note"),
        serde_json::to_value(&collateral_refund_note).unwrap(),
    );
    //
    json_map.insert(
        String::from("vlp_note_idx"),
        serde_json::to_value(&vlp_note.index).unwrap(),
    );
    json_map.insert(
        String::from("vlp_note_hash"),
        serde_json::to_value(&vlp_note.hash.to_string()).unwrap(),
    );
    json_map.insert(
        String::from("signature"),
        serde_json::to_value(&signature).unwrap(),
    );

    let mut swap_output_json = swap_output_json_m.lock();
    swap_output_json.push(json_map);
    drop(swap_output_json);
}

// * ================================================================================================
// * REMOVE LIQUIDITY * //

pub fn onchain_tab_remove_liquidity_json_output(
    swap_output_json_m: &Arc<Mutex<Vec<serde_json::Map<String, Value>>>>,
    //
    vlp_notes_in: &Vec<Note>,
    user_index_price: u64,
    slippage: u32,
    base_close_order_fields: &CloseOrderFields,
    quote_close_order_fields: &CloseOrderFields,
    //
    base_return_amount: u64,
    index_price: u64,
    prev_order_tab: &OrderTab,
    new_order_tab: &Option<OrderTab>,
    base_return_note: &Note,
    quote_return_note: &Note,
    signature: &Signature,
) {
    let mut json_map = serde_json::map::Map::new();
    json_map.insert(
        String::from("transaction_type"),
        serde_json::to_value(&"remove_liquidity").unwrap(),
    );
    json_map.insert(
        String::from("is_order_tab"),
        serde_json::to_value(&true).unwrap(),
    );
    //
    json_map.insert(
        String::from("vlp_notes_in"),
        serde_json::to_value(&vlp_notes_in).unwrap(),
    );
    json_map.insert(
        String::from("user_index_price"),
        serde_json::to_value(&user_index_price).unwrap(),
    );
    json_map.insert(
        String::from("slippage"),
        serde_json::to_value(&slippage).unwrap(),
    );
    json_map.insert(
        String::from("base_close_order_fields"),
        serde_json::to_value(&base_close_order_fields).unwrap(),
    );
    json_map.insert(
        String::from("quote_close_order_fields"),
        serde_json::to_value(&quote_close_order_fields).unwrap(),
    );
    //
    json_map.insert(
        String::from("base_return_amount"),
        serde_json::to_value(&base_return_amount).unwrap(),
    );
    json_map.insert(
        String::from("index_price"),
        serde_json::to_value(&index_price).unwrap(),
    );
    json_map.insert(
        String::from("signature"),
        serde_json::to_value(&signature).unwrap(),
    );
    //
    json_map.insert(
        String::from("prev_order_tab"),
        serde_json::to_value(prev_order_tab).unwrap(),
    );
    let new_order_tab_hash = new_order_tab.as_ref().map(|tab| tab.hash.to_string());
    json_map.insert(
        String::from("new_order_tab_hash"),
        serde_json::to_value(&new_order_tab_hash).unwrap(),
    );
    json_map.insert(
        String::from("base_return_note_index"),
        serde_json::to_value(base_return_note.index).unwrap(),
    );
    json_map.insert(
        String::from("base_return_note_hash"),
        serde_json::to_value(&base_return_note.hash.to_string()).unwrap(),
    );
    json_map.insert(
        String::from("quote_return_note_index"),
        serde_json::to_value(quote_return_note.index).unwrap(),
    );
    json_map.insert(
        String::from("quote_return_note_hash"),
        serde_json::to_value(&quote_return_note.hash.to_string()).unwrap(),
    );

    let mut swap_output_json = swap_output_json_m.lock();
    swap_output_json.push(json_map);
    drop(swap_output_json);
}

pub fn onchain_position_remove_liquidity_json_output(
    swap_output_json_m: &Arc<Mutex<Vec<serde_json::Map<String, Value>>>>,
    //
    vlp_notes_in: &Vec<VlpNote>,
    collateral_close_order_fields: &CloseOrderFields,
    //
    prev_position: &PerpPosition,
    new_position: &Option<PerpPosition>,
    collateral_return_note: &Note,
    signature: &Signature,
) {
    let mut json_map = serde_json::map::Map::new();
    json_map.insert(
        String::from("transaction_type"),
        serde_json::to_value(&"remove_liquidity").unwrap(),
    );
    json_map.insert(
        String::from("is_order_tab"),
        serde_json::to_value(&false).unwrap(),
    );
    //
    json_map.insert(
        String::from("vlp_notes_in"),
        serde_json::to_value(&vlp_notes_in).unwrap(),
    );
    json_map.insert(
        String::from("collateral_close_order_fields"),
        serde_json::to_value(&collateral_close_order_fields).unwrap(),
    );
    //
    json_map.insert(
        String::from("signature"),
        serde_json::to_value(&signature).unwrap(),
    );
    //
    json_map.insert(
        String::from("prev_position"),
        serde_json::to_value(prev_position).unwrap(),
    );
    let new_position_hash = new_position.as_ref().map(|pos| pos.hash.to_string());
    json_map.insert(
        String::from("new_position_hash"),
        serde_json::to_value(&new_position_hash).unwrap(),
    );
    json_map.insert(
        String::from("collateral_return_note_index"),
        serde_json::to_value(collateral_return_note.index).unwrap(),
    );
    json_map.insert(
        String::from("collateral_return_note_hash"),
        serde_json::to_value(&collateral_return_note.hash.to_string()).unwrap(),
    );

    let mut swap_output_json = swap_output_json_m.lock();
    swap_output_json.push(json_map);
    drop(swap_output_json);
}
