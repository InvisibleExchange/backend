use std::sync::Arc;

use num_bigint::BigUint;
use parking_lot::Mutex;

use crate::{
    perpetual::perp_position::PerpPosition, transaction_batch::TxOutputJson,
    utils::crypto_utils::Signature,
};

// * ONCHAIN OPEN ORDER TAB JSON OUTPUT
pub fn onchain_register_json_output(
    transaction_output_json_m: &Arc<Mutex<TxOutputJson>>,
    prev_position: &PerpPosition,
    new_position: &PerpPosition,
    vlp_token: u32,
    max_vlp_supply: u64,
    signature: &Signature,
) {
    let mut json_map = serde_json::map::Map::new();
    json_map.insert(
        String::from("transaction_type"),
        serde_json::to_value(&"onchain_mm_action").unwrap(),
    );
    json_map.insert(
        String::from("action_type"),
        serde_json::to_value(&"register_mm").unwrap(),
    );
    json_map.insert(
        String::from("prev_position"),
        serde_json::to_value(&prev_position).unwrap(),
    );
    json_map.insert(
        String::from("new_position_hash"),
        serde_json::to_value(&new_position.hash.to_string()).unwrap(),
    );
    json_map.insert(
        String::from("vlp_token"),
        serde_json::to_value(&vlp_token).unwrap(),
    );
    json_map.insert(
        String::from("max_vlp_supply"),
        serde_json::to_value(&max_vlp_supply).unwrap(),
    );
    json_map.insert(
        String::from("signature"),
        serde_json::to_value(&signature).unwrap(),
    );

    let mut transaction_output_json = transaction_output_json_m.lock();
    transaction_output_json.tx_micro_batch.push(json_map);
    drop(transaction_output_json);
}

// * ================================================================================================
// * ADD LIQUIDITY * //

pub fn onchain_position_add_liquidity_json_output(
    transaction_output_json_m: &Arc<Mutex<TxOutputJson>>,
    prev_position: &PerpPosition,
    new_position_hash: &BigUint,
    depositor: &String,
    initial_value: u64,
    vlp_amount: u64,
    signature: &Signature,
) {
    let mut json_map = serde_json::map::Map::new();
    json_map.insert(
        String::from("transaction_type"),
        serde_json::to_value(&"onchain_mm_action").unwrap(),
    );
    json_map.insert(
        String::from("action_type"),
        serde_json::to_value(&"add_liquidity").unwrap(),
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
        String::from("depositor"),
        serde_json::to_value(&depositor).unwrap(),
    );
    json_map.insert(
        String::from("initial_value"),
        serde_json::to_value(&initial_value).unwrap(),
    );
    json_map.insert(
        String::from("vlp_amount"),
        serde_json::to_value(&vlp_amount).unwrap(),
    );
    json_map.insert(
        String::from("signature"),
        serde_json::to_value(&signature).unwrap(),
    );

    let mut transaction_output_json = transaction_output_json_m.lock();
    transaction_output_json.tx_micro_batch.push(json_map);
    drop(transaction_output_json);
}

// * ================================================================================================
// * REMOVE LIQUIDITY * //

pub fn onchain_position_remove_liquidity_json_output(
    transaction_output_json_m: &Arc<Mutex<TxOutputJson>>,
    prev_position: &PerpPosition,
    new_position: &PerpPosition,
    depositor: &String,
    initial_value: u64,
    vlp_amount: u64,
    return_collateral_amount: u64,
    mm_fee: u64,
    signature: &Signature,
) {
    let mut json_map = serde_json::map::Map::new();
    json_map.insert(
        String::from("transaction_type"),
        serde_json::to_value(&"onchain_mm_action").unwrap(),
    );
    json_map.insert(
        String::from("action_type"),
        serde_json::to_value(&"remove_liquidity").unwrap(),
    );
    json_map.insert(
        String::from("signature"),
        serde_json::to_value(&signature).unwrap(),
    );
    json_map.insert(
        String::from("prev_position"),
        serde_json::to_value(prev_position).unwrap(),
    );
    json_map.insert(
        String::from("new_position_hash"),
        serde_json::to_value(&new_position.hash.to_string()).unwrap(),
    );
    json_map.insert(
        String::from("depositor"),
        serde_json::to_value(&depositor).unwrap(),
    );
    json_map.insert(
        String::from("initial_value"),
        serde_json::to_value(&initial_value).unwrap(),
    );
    json_map.insert(
        String::from("vlp_amount"),
        serde_json::to_value(&vlp_amount).unwrap(),
    );
    json_map.insert(
        String::from("return_collateral_amount"),
        serde_json::to_value(&return_collateral_amount).unwrap(),
    );
    json_map.insert(
        String::from("mm_fee"),
        serde_json::to_value(&mm_fee).unwrap(),
    );

    let mut transaction_output_json = transaction_output_json_m.lock();
    transaction_output_json.tx_micro_batch.push(json_map);
    drop(transaction_output_json);
}

// * ================================================================================================
// * CLOSE MM * //

pub fn onchain_position_close_json_output(
    transaction_output_json_m: &Arc<Mutex<TxOutputJson>>,
    prev_position: &PerpPosition,
    new_position: &PerpPosition,
    initial_value_sum: u64,
    vlp_amount_sum: u64,
    return_collateral_amount: u64,
    mm_fee: u64,
    signature: &Signature,
) {
    let mut json_map = serde_json::map::Map::new();
    json_map.insert(
        String::from("transaction_type"),
        serde_json::to_value(&"onchain_mm_action").unwrap(),
    );
    json_map.insert(
        String::from("action_type"),
        serde_json::to_value(&"close_mm_position").unwrap(),
    );
    json_map.insert(
        String::from("signature"),
        serde_json::to_value(&signature).unwrap(),
    );
    json_map.insert(
        String::from("prev_position"),
        serde_json::to_value(prev_position).unwrap(),
    );
    json_map.insert(
        String::from("new_position_hash"),
        serde_json::to_value(&new_position.hash.to_string()).unwrap(),
    );
    json_map.insert(
        String::from("initial_value_sum"),
        serde_json::to_value(&initial_value_sum).unwrap(),
    );
    json_map.insert(
        String::from("vlp_amount_sum"),
        serde_json::to_value(&vlp_amount_sum).unwrap(),
    );
    json_map.insert(
        String::from("return_collateral_amount"),
        serde_json::to_value(&return_collateral_amount).unwrap(),
    );
    json_map.insert(
        String::from("mm_fee"),
        serde_json::to_value(&mm_fee).unwrap(),
    );

    let mut transaction_output_json = transaction_output_json_m.lock();
    transaction_output_json.tx_micro_batch.push(json_map);
    drop(transaction_output_json);
}
