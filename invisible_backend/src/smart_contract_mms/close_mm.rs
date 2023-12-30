use std::{collections::HashMap, sync::Arc};

use num_bigint::BigUint;
use parking_lot::Mutex;
use serde_json::Value;

use firestore_db_and_auth::ServiceSession;

use crate::utils::storage::backup_storage::BackupStorage;
use crate::utils::storage::local_storage::{MainStorage, OnchainActionType};
use crate::{
    perpetual::perp_position::PerpPosition, server::grpc::engine_proto::OnChainCloseMmReq,
    transaction_batch::LeafNodeType, trees::superficial_tree::SuperficialTree,
    utils::storage::firestore::start_add_position_thread,
};

use crate::utils::crypto_utils::Signature;

use super::helpers::mm_helpers::get_close_mm_commitment;
use super::helpers::{
    json_output::onchain_position_close_json_output,
    mm_helpers::{
        get_return_collateral_amount, onchain_register_mm_state_updates, verfiy_mm_pos_close_sig,
        verify_position_validity,
    },
};

/// Claim the deposit that was created onchain
pub fn close_onchain_mm(
    session: &Arc<Mutex<ServiceSession>>,
    main_storage: &Arc<Mutex<MainStorage>>,
    backup_storage: &Arc<Mutex<BackupStorage>>,
    close_req: OnChainCloseMmReq,
    state_tree: &Arc<Mutex<SuperficialTree>>,
    updated_state_hashes: &Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
    swap_output_json_m: &Arc<Mutex<Vec<serde_json::Map<String, Value>>>>,
) -> std::result::Result<PerpPosition, String> {
    //

    let position = verify_position_validity(&close_req.position, &state_tree)?;

    // ? Verify this is not a smart_contract initiated position
    if position.vlp_supply <= 0 {
        return Err("This is not a smart contract initiated position".to_string());
    }

    // ? Verify the signature ---------------------------------------------------------------------
    let signature = Signature::try_from(close_req.signature.unwrap_or_default())
        .map_err(|err| err.to_string())?;
    let valid = verfiy_mm_pos_close_sig(
        &position,
        close_req.initial_value_sum,
        close_req.vlp_amount_sum,
        &signature,
    );
    if !valid {
        return Err("Invalid Signature".to_string());
    }

    let return_collateral_amount = get_return_collateral_amount(
        close_req.vlp_amount_sum,
        position.vlp_supply,
        position.margin,
    );

    let mm_fee: i64 =
        (return_collateral_amount as i64 - close_req.initial_value_sum as i64) * 20 / 100; // 20% fee
    let mm_fee = std::cmp::max(0, mm_fee) as u64;

    // ? Adding to an existing order tab
    let prev_position = position;

    let mut new_position = prev_position.clone();

    new_position.margin -= return_collateral_amount;
    new_position.vlp_supply = 0;
    new_position.update_position_info();

    // ? Verify the registration has been registered
    let data_commitment = get_close_mm_commitment(
        close_req.mm_action_id,
        &prev_position.position_header.position_address,
        close_req.initial_value_sum,
        close_req.vlp_amount_sum,
    );
    let main_storage_m = main_storage.lock();
    if !main_storage_m.does_commitment_exists(
        OnchainActionType::MMClosePosition,
        close_req.mm_action_id as u64 * 2_u64.pow(20),
        &data_commitment,
    ) {
        return Err("MM Registration not registered".to_string());
    }
    main_storage_m.remove_onchain_action_commitment(close_req.mm_action_id as u64 * 2_u64.pow(20));
    drop(main_storage_m);

    // ? GENERATE THE JSON_OUTPUT -----------------------------------------------------------------
    onchain_position_close_json_output(
        swap_output_json_m,
        &prev_position,
        &new_position,
        close_req.initial_value_sum,
        close_req.vlp_amount_sum,
        return_collateral_amount,
        mm_fee,
        &signature,
    );

    // ? UPDATE THE STATE TREE --------------------------------------------------------------------
    onchain_register_mm_state_updates(state_tree, updated_state_hashes, &new_position);

    // ? UPDATE THE DATABASE ----------------------------------------------------------------------
    let _h = start_add_position_thread(new_position.clone(), session, backup_storage);

    return Ok(new_position);
}
