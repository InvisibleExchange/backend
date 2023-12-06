use serde::Serialize;
use serde_json::{json, Value};

use crate::utils::notes::Note;

use super::super::{perp_order::PerpOrder, perp_position::PerpPosition, perp_swap::PerpSwap};

pub struct PerpSwapOutput<'a> {
    pub swap: &'a PerpSwap,
    pub order_a: &'a PerpOrder,
    pub order_b: &'a PerpOrder,
}

impl PerpSwapOutput<'_> {
    pub fn new<'a>(
        swap: &'a PerpSwap,
        order_a: &'a PerpOrder,
        order_b: &'a PerpOrder,
    ) -> PerpSwapOutput<'a> {
        PerpSwapOutput {
            swap,
            order_a,
            order_b,
        }
    }

    pub fn wrap_output(
        &self,
        is_first_fill_a: bool,
        is_first_fill_b: bool,
        prev_pfr_note_a: &Option<Note>,
        prev_pfr_note_b: &Option<Note>,
        new_pfr_note_hash_a: &Option<String>,
        new_pfr_note_hash_b: &Option<String>,
        prev_position_a: &Option<PerpPosition>,
        prev_position_b: &Option<PerpPosition>,
        new_position_hash_a: &Option<String>,
        new_position_hash_b: &Option<String>,
        position_index_a: u32,
        position_index_b: u32,
        new_pfr_idx_a: u64,
        new_pfr_idx_b: u64,
        return_collateral_idx_a: u64,
        return_collateral_idx_b: u64,
        return_collateral_hash_a: &Option<String>,
        return_collateral_hash_b: &Option<String>,
        prev_funding_idx_a: u32,
        prev_funding_idx_b: u32,
        new_funding_idx: u32,
    ) -> serde_json::map::Map<String, Value> {
        let is_first_fill_a = serde_json::to_value(&is_first_fill_a).unwrap();
        let is_first_fill_b = serde_json::to_value(&is_first_fill_b).unwrap();
        let swap_json1 = serde_json::to_value(&self.swap).unwrap();
        let order_json1 = serde_json::to_value(&self.order_a).unwrap();
        let order_json2 = serde_json::to_value(&self.order_b).unwrap();
        let prev_position_a_json = serde_json::to_value(&prev_position_a).unwrap();
        let prev_position_b_json2 = serde_json::to_value(&prev_position_b).unwrap();
        let pfr_note_a_json = serde_json::to_value(&prev_pfr_note_a).unwrap();
        let pfr_note_b_json2 = serde_json::to_value(&prev_pfr_note_b).unwrap();

        let new_pfr_note_hash_a = serde_json::to_value(&new_pfr_note_hash_a).unwrap();
        let new_pfr_note_hash_b = serde_json::to_value(&new_pfr_note_hash_b).unwrap();
        let new_position_hash_a = serde_json::to_value(&new_position_hash_a).unwrap();
        let new_position_hash_b = serde_json::to_value(&new_position_hash_b).unwrap();
        let return_collateral_hash_a = serde_json::to_value(&return_collateral_hash_a).unwrap();
        let return_collateral_hash_b = serde_json::to_value(&return_collateral_hash_b).unwrap();

        let indexes_json = json!({
            "order_a": {
                "position_idx": position_index_a,
                "new_pfr_idx": new_pfr_idx_a,
                "return_collateral_idx": return_collateral_idx_a,
                "prev_funding_idx": prev_funding_idx_a,
                "new_funding_idx": new_funding_idx
            },
            "order_b": {
                "position_idx": position_index_b,
                "new_pfr_idx": new_pfr_idx_b,
                "return_collateral_idx": return_collateral_idx_b,
                "prev_funding_idx": prev_funding_idx_b,
                "new_funding_idx": new_funding_idx
            },
        });

        let mut json_map = serde_json::map::Map::new();

        json_map.insert(
            String::from("transaction_type"),
            serde_json::to_value(&"perpetual_swap").unwrap(),
        );
        json_map.insert(String::from("swap_data"), swap_json1);
        json_map.insert(String::from("order_a"), order_json1);
        json_map.insert(String::from("order_b"), order_json2);
        json_map.insert(String::from("prev_pfr_note_a"), pfr_note_a_json);
        json_map.insert(String::from("prev_pfr_note_b"), pfr_note_b_json2);
        json_map.insert(String::from("new_pfr_note_hash_a"), new_pfr_note_hash_a);
        json_map.insert(String::from("new_pfr_note_hash_b"), new_pfr_note_hash_b);
        json_map.insert(String::from("prev_position_a"), prev_position_a_json);
        json_map.insert(String::from("prev_position_b"), prev_position_b_json2);
        json_map.insert(String::from("new_position_hash_a"), new_position_hash_a);
        json_map.insert(String::from("new_position_hash_b"), new_position_hash_b);
        json_map.insert(
            String::from("return_collateral_hash_a"),
            return_collateral_hash_a,
        );
        json_map.insert(
            String::from("return_collateral_hash_b"),
            return_collateral_hash_b,
        );
        json_map.insert(String::from("is_first_fill_a"), is_first_fill_a);
        json_map.insert(String::from("is_first_fill_b"), is_first_fill_b);
        json_map.insert(String::from("indexes"), indexes_json);

        return json_map;
    }
}

#[derive(Debug, Serialize, Clone)]
pub struct PerpSwapResponse {
    pub position_a: Option<PerpPosition>,
    pub position_b: Option<PerpPosition>,
    pub new_pfr_info_a: (Option<Note>, u64, u64),
    pub new_pfr_info_b: (Option<Note>, u64, u64),
    pub return_collateral_note_a: Option<Note>,
    pub return_collateral_note_b: Option<Note>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PerpOrderFillResponse {
    pub position: Option<PerpPosition>,
    pub new_pfr_info: (Option<Note>, u64, u64),
    pub return_collateral_note: Option<Note>,
    pub synthetic_token: u32,
    pub qty: u64,
    pub fee_taken: u64,
}

impl PerpOrderFillResponse {
    pub fn from_swap_response(
        req: &PerpSwapResponse,
        is_a: bool,
        qty: u64,
        synthetic_token: u32,
        fee_taken: u64,
    ) -> Self {
        if is_a {
            return PerpOrderFillResponse {
                position: req.position_a.clone(),
                new_pfr_info: req.new_pfr_info_a.clone(),
                return_collateral_note: req.return_collateral_note_a.clone(),
                synthetic_token,
                qty,
                fee_taken,
            };
        } else {
            return PerpOrderFillResponse {
                position: req.position_b.clone(),
                new_pfr_info: req.new_pfr_info_b.clone(),
                return_collateral_note: req.return_collateral_note_b.clone(),
                synthetic_token,
                qty,
                fee_taken,
            };
        }
    }
}

// Execution thread output

#[derive(Clone)]
pub struct TxExecutionThreadOutput {
    pub prev_pfr_note: Option<Note>, // Index of the previous partial fill refund note
    pub new_pfr_info: (Option<Note>, u64, u64), // info about the new partial fill (pfr note, amount_filled, collateral_left)
    pub is_fully_filled: bool,                  // whether the order was fully filled
    pub prev_funding_idx: u32, // Index of the last time funding was applied to this position
    pub position_index: u32,   // index of the modified position in the perp_state_tree
    pub position: Option<PerpPosition>, // The position after being modified
    pub prev_position: Option<PerpPosition>, // The position before being modified
    pub collateral_returned: u64, // amount of collateral returned when closing a position
    pub return_collateral_note: Option<Note>, // note of the collateral returned when closing a position
    pub synthetic_amount_filled: u64,         // the new amount filled (for partial fills)
}
