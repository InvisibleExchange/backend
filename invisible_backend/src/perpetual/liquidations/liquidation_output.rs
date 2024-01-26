use num_bigint::BigUint;
use serde_json::{json, Value};

use crate::utils::crypto_utils::Signature;

use super::{super::perp_position::PerpPosition, liquidation_order::LiquidationOrder};

pub fn wrap_liquidation_output(
    liquidation_order: &LiquidationOrder,
    signature: &Signature,
    new_liquidated_position_hash: &Option<String>,
    new_position_hash: &String,
    new_position_index: u64,
    prev_funding_idx: u32,
    new_funding_idx: u32,
    market_price: u64,
    index_price: u64,
) -> serde_json::map::Map<String, Value> {
    let order_json1 = serde_json::to_value(&liquidation_order).unwrap();
    let new_liquidated_position_hash_json =
        serde_json::to_value(&new_liquidated_position_hash).unwrap();
    let new_position_hash_json = serde_json::to_value(&new_position_hash).unwrap();

    let indexes_json = json!({
        "new_position_index": new_position_index,
        "prev_funding_idx": prev_funding_idx,
        "new_funding_idx": new_funding_idx

    });

    let mut json_map = serde_json::map::Map::new();

    json_map.insert(
        String::from("transaction_type"),
        serde_json::to_value(&"liquidation_order").unwrap(),
    );
    json_map.insert(String::from("liquidation_order"), order_json1);
    json_map.insert(
        String::from("signature"),
        serde_json::to_value(&signature).unwrap(),
    );
    json_map.insert(
        String::from("new_liquidated_position_hash"),
        new_liquidated_position_hash_json,
    );
    json_map.insert(String::from("new_position_hash"), new_position_hash_json);

    json_map.insert(
        String::from("market_price"),
        serde_json::to_value(&market_price).unwrap(),
    );
    json_map.insert(
        String::from("index_price"),
        serde_json::to_value(&index_price).unwrap(),
    );

    json_map.insert(String::from("indexes"), indexes_json);

    return json_map;
}

#[derive(Clone)]
pub struct LiquidationResponse {
    pub liquidated_position_address: BigUint,
    pub liquidated_position_index: u64,
    pub liquidated_position: Option<PerpPosition>,
    pub new_position: PerpPosition,
}
