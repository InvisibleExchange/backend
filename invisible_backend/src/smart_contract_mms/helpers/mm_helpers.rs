use std::{collections::HashMap, str::FromStr, sync::Arc};

use num_bigint::BigUint;
use num_traits::FromPrimitive;
use parking_lot::Mutex;

use crate::{
    perpetual::{perp_position::PerpPosition, SYNTHETIC_ASSETS},
    server::grpc::engine_proto::GrpcPerpPosition,
    transaction_batch::LeafNodeType,
    trees::superficial_tree::SuperficialTree,
    utils::crypto_utils::{hash_many, verify, Signature},
};

// * ----------------------------------------------------------------------------

/// Verify the order tab is valid
pub fn verify_position_validity(
    position_req: &Option<GrpcPerpPosition>,
    state_tree: &Arc<Mutex<SuperficialTree>>,
) -> Result<PerpPosition, String> {
    if position_req.is_none() {
        return Err("Position is not defined".to_string());
    }
    let position = PerpPosition::try_from(position_req.as_ref().unwrap().clone());

    if let Err(e) = position {
        return Err("Position is not properly defined: ".to_string() + &e.to_string());
    }
    let position = position.unwrap();

    if !SYNTHETIC_ASSETS.contains(&position.position_header.synthetic_token) {
        return Err("Synthetic token is invalid".to_string());
    }

    // ? Verify that the position exists
    let state_tree_m = state_tree.lock();

    let leaf_hash = state_tree_m.get_leaf_by_index(position.index as u64);
    if leaf_hash != position.hash {
        return Err("position does not exist".to_string());
    }
    drop(state_tree_m);

    return Ok(position);
}

// * ----------------------------------------------------------------------------

/// Verify the signature
pub fn verfiy_register_mm_sig(
    position: &PerpPosition,
    vlp_token: u32,
    max_vlp_supply: u64,
    signature: &Signature,
) -> bool {
    // & header_hash = H({position.hash, vlp_token, max_vlp_supply})

    let mut hash_inputs: Vec<&BigUint> = Vec::new();

    hash_inputs.push(&position.hash);

    let vlp_token = BigUint::from(vlp_token);
    hash_inputs.push(&vlp_token);

    let max_vlp_supply = BigUint::from(max_vlp_supply);
    hash_inputs.push(&max_vlp_supply);

    let hash = hash_many(&hash_inputs);

    let valid = verify(&position.position_header.position_address, &hash, signature);

    return valid;
}

// * ----------------------------------------------------------------------------

pub fn onchain_register_mm_state_updates(
    state_tree: &Arc<Mutex<SuperficialTree>>,
    updated_state_hashes: &Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
    position: &PerpPosition,
) {
    let mut state_tree_m = state_tree.lock();
    let mut updated_state_hashes_m = updated_state_hashes.lock();

    // ? add it to the positons state
    state_tree_m.update_leaf_node(&position.hash, position.index as u64);
    updated_state_hashes_m.insert(
        position.index as u64,
        (LeafNodeType::Position, position.hash.clone()),
    );

    drop(state_tree_m);
    drop(updated_state_hashes_m);
}

// * ----------------------------------------------------------------------------

/// Verify the signature for the order tab hash
pub fn verfiy_remove_liquidity_sig(
    position: &PerpPosition,
    depositor: &String,
    initial_value: u64,
    vlp_amount: u64,
    signature: &Signature,
) -> bool {
    //

    // & hash = H({position.hash, depositor, intial_value, vlp_amount})
    let mut hash_inputs: Vec<&BigUint> = vec![];

    hash_inputs.push(&position.hash);

    let depositor = BigUint::from_str(depositor).unwrap();
    hash_inputs.push(&depositor);

    let initial_value = BigUint::from_u64(initial_value).unwrap();
    hash_inputs.push(&initial_value);

    let vlp_amount = BigUint::from_u64(vlp_amount).unwrap();
    hash_inputs.push(&vlp_amount);

    let hash = hash_many(&hash_inputs);

    let valid = verify(&position.position_header.position_address, &hash, signature);

    return valid;
}

pub fn get_return_collateral_amount(vlp_amount: u64, vlp_supply: u64, margin: u64) -> u64 {
    let return_collateral = (vlp_amount as u128 * margin as u128) / vlp_supply as u128;

    return return_collateral as u64;
}

// * ----------------------------------------------------------------------------

pub fn calculate_pos_vlp_amount(position: &PerpPosition, collateral_amount: u64) -> u64 {
    // ? calculate the right amount of vLP tokens to mint
    let vlp_supply = position.vlp_supply;
    let total_margin = position.margin;

    let vlp_amount = (collateral_amount as u128 * vlp_supply as u128) / total_margin as u128;

    return vlp_amount as u64;
}

/// Verify the signature for the order tab hash
pub fn verfiy_pos_add_liquidity_sig(
    position: &PerpPosition,
    depositor: &String,
    collateral_amount: u64,
    signature: &Signature,
) -> bool {
    // & header_hash = H({pos_hash, depositor, collateral_amount})

    let mut hash_inputs: Vec<&BigUint> = vec![];

    hash_inputs.push(&position.hash);

    let depositor = BigUint::from_str(&depositor).unwrap();
    hash_inputs.push(&depositor);

    let collateral_amount = BigUint::from_u64(collateral_amount).unwrap();
    hash_inputs.push(&collateral_amount);

    let hash = hash_many(&hash_inputs);

    let valid = verify(&position.position_header.position_address, &hash, signature);

    return valid;
}

// * ----------------------------------------------------------------------------

pub fn verfiy_mm_pos_close_sig(
    position: &PerpPosition,
    initial_value_sum: u64,
    vlp_amount_sum: u64,
    signature: &Signature,
) -> bool {
    // & header_hash = H({pos_hash, initial_value_sum, vlp_amount_sum})

    let mut hash_inputs: Vec<&BigUint> = vec![];

    hash_inputs.push(&position.hash);

    let initial_value_sum = BigUint::from_u64(initial_value_sum).unwrap();
    hash_inputs.push(&initial_value_sum);

    let vlp_amount_sum = BigUint::from_u64(vlp_amount_sum).unwrap();
    hash_inputs.push(&vlp_amount_sum);

    let hash = hash_many(&hash_inputs);

    let valid = verify(&position.position_header.position_address, &hash, signature);

    return valid;
}

// * ----------------------------------------------------------------------------

pub fn get_mm_register_commitment(
    mm_action_id: u32,
    synthetic_asset: u32,
    position_address: &BigUint,
    vlp_token: u32,
) -> BigUint {
    // & hash = H({mm_action_id, synthetic_asset, position_address, vlp_token})
    let mut hash_inputs: Vec<&BigUint> = vec![];

    let mm_action_id = BigUint::from_u32(mm_action_id).unwrap();
    hash_inputs.push(&mm_action_id);

    let synthetic_asset = BigUint::from_u32(synthetic_asset).unwrap();
    hash_inputs.push(&synthetic_asset);

    hash_inputs.push(&position_address);

    let vlp_token = BigUint::from_u32(vlp_token).unwrap();
    hash_inputs.push(&vlp_token);

    let commitment = hash_many(&hash_inputs);

    return commitment;
}

pub fn get_add_liquidity_commitment(
    mm_action_id: u32,
    depositor: &BigUint,
    position_address: &BigUint,
    usdc_amount: u64,
) -> BigUint {
    // & hash = H({ mm_action_id, depositor, position_address, usdc_amount})
    let mut hash_inputs: Vec<&BigUint> = vec![];

    let mm_action_id = BigUint::from_u32(mm_action_id).unwrap();
    hash_inputs.push(&mm_action_id);

    hash_inputs.push(&depositor);
    hash_inputs.push(&position_address);

    let usdc_amount = BigUint::from_u64(usdc_amount).unwrap();
    hash_inputs.push(&usdc_amount);

    let commitment = hash_many(&hash_inputs);

    return commitment;
}

pub fn get_remove_liquidity_commitment(
    mm_action_id: u32,
    depositor: &BigUint,
    position_address: &BigUint,
    initial_value: u64,
    vlp_amount: u64,
) -> BigUint {
    // & hash = H({ mm_action_id, depositor, position_address, initial_value, vlp_amount})
    let mut hash_inputs: Vec<&BigUint> = vec![];

    let mm_action_id = BigUint::from_u32(mm_action_id).unwrap();
    hash_inputs.push(&mm_action_id);

    hash_inputs.push(&depositor);
    hash_inputs.push(&position_address);

    let initial_value = BigUint::from_u64(initial_value).unwrap();
    hash_inputs.push(&initial_value);

    let vlp_amount = BigUint::from_u64(vlp_amount).unwrap();
    hash_inputs.push(&vlp_amount);

    let commitment = hash_many(&hash_inputs);

    return commitment;
}

pub fn get_close_mm_commitment(
    mm_action_id: u32,
    position_address: &BigUint,
    initial_value_sum: u64,
    vlp_amount_sum: u64,
) -> BigUint {
    // & hash = H({ mm_action_id, position_address, initial_value_sum, vlp_amount_sum})
    let mut hash_inputs: Vec<&BigUint> = vec![];

    let mm_action_id = BigUint::from_u32(mm_action_id).unwrap();
    hash_inputs.push(&mm_action_id);

    hash_inputs.push(&position_address);

    let initial_value_sum = BigUint::from_u64(initial_value_sum).unwrap();
    hash_inputs.push(&initial_value_sum);

    let vlp_amount_sum = BigUint::from_u64(vlp_amount_sum).unwrap();
    hash_inputs.push(&vlp_amount_sum);

    let commitment = hash_many(&hash_inputs);

    return commitment;
}
