use std::{collections::HashMap, sync::Arc};

use num_bigint::BigUint;
use parking_lot::Mutex;
use serde_json::Value;

use firestore_db_and_auth::ServiceSession;

use crate::utils::storage::backup_storage::BackupStorage;
use crate::utils::storage::firestore::start_add_position_thread;
use crate::utils::storage::local_storage::{MainStorage, OnchainActionType};
use crate::{
    perpetual::perp_position::PerpPosition, server::grpc::engine_proto::OnChainRegisterMmReq,
    transaction_batch::LeafNodeType, trees::superficial_tree::SuperficialTree,
};

use crate::utils::crypto_utils::Signature;

use super::helpers::mm_helpers::{
    get_mm_register_commitment, onchain_register_mm_state_updates, verfiy_register_mm_sig,
};
use super::helpers::{
    json_output::onchain_register_json_output, mm_helpers::verify_position_validity,
};

/// Claim the deposit that was created onchain
pub fn onchain_register_mm(
    session: &Arc<Mutex<ServiceSession>>,
    main_storage: &Arc<Mutex<MainStorage>>,
    backup_storage: &Arc<Mutex<BackupStorage>>,
    register_mm_req: OnChainRegisterMmReq,
    state_tree: &Arc<Mutex<SuperficialTree>>,
    updated_state_hashes: &Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
    swap_output_json_m: &Arc<Mutex<Vec<serde_json::Map<String, Value>>>>,
) -> std::result::Result<PerpPosition, String> {
    //

    let mut position = verify_position_validity(&register_mm_req.position, &state_tree)?;

    // ? Verify this is not a smart_contract initiated position
    if position.vlp_supply > 0 {
        return Err("This is already a smart contract initiated position".to_string());
    }

    let prev_position = position.clone();

    // ? Verify the signature ---------------------------------------------------------------------
    let signature = Signature::try_from(register_mm_req.signature.as_ref().unwrap().clone())
        .map_err(|err| err.to_string())?;

    let valid = verfiy_register_mm_sig(
        &position,
        register_mm_req.vlp_token,
        register_mm_req.max_vlp_supply,
        &signature,
    );
    if !valid {
        return Err("Invalid Signature".to_string());
    }

    let vlp_amount = position.margin;

    // ? Verify the registration has been registered
    let data_commitment = get_mm_register_commitment(
        register_mm_req.mm_action_id,
        register_mm_req.synthetic_token,
        &position.position_header.position_address,
        register_mm_req.vlp_token,
    );
    let main_storage_m = main_storage.lock();
    if !main_storage_m.does_commitment_exists(
        OnchainActionType::MMRegistration,
        register_mm_req.mm_action_id as u64,
        &data_commitment,
    ) {
        return Err("MM Registration not registered".to_string());
    }
    main_storage_m.remove_onchain_action_commitment(register_mm_req.mm_action_id as u64);
    drop(main_storage_m);

    // ? Update the position -----------------

    position.position_header.vlp_token = register_mm_req.vlp_token;
    position.position_header.max_vlp_supply = register_mm_req.max_vlp_supply;

    position.vlp_supply = vlp_amount;

    position.position_header.update_hash();
    position.hash = position.hash_position();

    // ? GENERATE THE JSON_OUTPUT -----------------------------------------------------------------
    onchain_register_json_output(
        &swap_output_json_m,
        &prev_position,
        &position,
        register_mm_req.vlp_token,
        register_mm_req.max_vlp_supply,
        &signature,
    );

    // ? UPDATE THE STATE TREE --------------------------------------------------------------------
    onchain_register_mm_state_updates(state_tree, updated_state_hashes, &position);

    // ? UPDATE THE DATABASE ----------------------------------------------------------------------
    let _h = start_add_position_thread(position.clone(), session, backup_storage);

    return Ok(position);
}

//

// * HELPERS =======================================================================================
