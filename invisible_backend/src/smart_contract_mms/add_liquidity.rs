use std::{collections::HashMap, sync::Arc};

use num_bigint::BigUint;
use parking_lot::Mutex;
use serde_json::Value;

use firestore_db_and_auth::ServiceSession;

use crate::utils::storage::backup_storage::BackupStorage;
use crate::utils::storage::firestore::start_add_position_thread;
use crate::utils::storage::local_storage::{MainStorage, OnchainActionType};
use crate::{
    perpetual::perp_position::PerpPosition, server::grpc::engine_proto::OnChainAddLiqReq,
    transaction_batch::LeafNodeType, trees::superficial_tree::SuperficialTree,
};

use crate::utils::crypto_utils::Signature;

use super::helpers::json_output::onchain_position_add_liquidity_json_output;
use super::helpers::mm_helpers::{
    calculate_pos_vlp_amount, get_add_liquidity_commitment, onchain_register_mm_state_updates,
    verfiy_pos_add_liquidity_sig, verify_position_validity,
};

/// Claim the deposit that was created onchain
pub fn add_liquidity_to_mm(
    session: &Arc<Mutex<ServiceSession>>,
    main_storage: &Arc<Mutex<MainStorage>>,
    backup_storage: &Arc<Mutex<BackupStorage>>,
    add_liquidity_req: OnChainAddLiqReq,
    state_tree: &Arc<Mutex<SuperficialTree>>,
    updated_state_hashes: &Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
    swap_output_json_m: &Arc<Mutex<Vec<serde_json::Map<String, Value>>>>,
) -> std::result::Result<PerpPosition, String> {
    //

    let mut position = verify_position_validity(&add_liquidity_req.position, &state_tree)?;

    // ? Verify this is not a smart_contract initiated position
    if position.vlp_supply <= 0 {
        return Err("This is not a smart contract initiated position".to_string());
    }

    // ? Verify the signature ---------------------------------------------------------------------
    let signature = Signature::try_from(add_liquidity_req.signature.unwrap_or_default())
        .map_err(|err| err.to_string())?;

    let valid = verfiy_pos_add_liquidity_sig(
        &position,
        &add_liquidity_req.depositor,
        add_liquidity_req.initial_value,
        &signature,
    );
    if !valid {
        return Err("Invalid Signature".to_string());
    }

    let vlp_amount = calculate_pos_vlp_amount(&position, add_liquidity_req.initial_value);

    // ? Verify the registration has been registered
    let data_commitment = get_add_liquidity_commitment(
        add_liquidity_req.mm_action_id,
        &add_liquidity_req.depositor,
        &position.position_header.position_address,
        add_liquidity_req.initial_value,
    )?;
    let main_storage_m = main_storage.lock();
    if !main_storage_m.does_commitment_exists(
        OnchainActionType::MMAddLiquidity,
        add_liquidity_req.mm_action_id as u64,
        &data_commitment,
    ) {
        return Err("Add Liquidity request not registered".to_string());
    }
    main_storage_m.remove_onchain_action_commitment(add_liquidity_req.mm_action_id as u64);
    drop(main_storage_m);

    // ? Update the position ---------------------------------------------------------------------

    // ? Adding to an existing position
    let prev_position = position.clone();

    position.margin += add_liquidity_req.initial_value;
    position.vlp_supply += vlp_amount;
    position.update_position_info();

    // ? GENERATE THE JSON_OUTPUT -----------------------------------------------------------------
    onchain_position_add_liquidity_json_output(
        &swap_output_json_m,
        &prev_position,
        &position.hash,
        &add_liquidity_req.depositor,
        add_liquidity_req.initial_value,
        vlp_amount,
        &signature,
    );

    // ? UPDATE THE STATE TREE --------------------------------------------------------------------
    onchain_register_mm_state_updates(state_tree, updated_state_hashes, &position);

    // ? UPDATE THE DATABASE ----------------------------------------------------------------------
    let _h = start_add_position_thread(position.clone(), session, backup_storage);

    return Ok(position);
}

//
