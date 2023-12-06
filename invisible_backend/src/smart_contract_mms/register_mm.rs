use std::{collections::HashMap, sync::Arc};

use num_bigint::BigUint;
use parking_lot::Mutex;
use serde_json::Value;

use firestore_db_and_auth::ServiceSession;

use crate::{
    order_tab::OrderTab, perpetual::perp_position::PerpPosition,
    server::grpc::engine_proto::OnChainRegisterMmReq, transaction_batch::LeafNodeType,
    trees::superficial_tree::SuperficialTree, utils::storage::local_storage::BackupStorage,
};

use crate::utils::crypto_utils::Signature;

use super::{
    helpers::{
        db_updates::onchain_open_tab_db_updates,
        json_output::onchain_register_json_output,
        register_mm_helpers::{
            get_vlp_amount, verfiy_open_order_sig, verify_close_order_fields,
            verify_order_tab_validity, verify_position_validity,
        },
        state_updates::onchain_register_mm_state_updates,
    },
    vlp_note::VlpNote,
};

// use super::{
//     db_updates::open_tab_db_updates, json_output::open_tab_json_output,
//     state_updates::open_tab_state_updates, OrderTab,
// };

// TODO: Check that the notes exist just before you update the state tree not in the beginning

/// Claim the deposit that was created onchain
pub fn onchain_register_mm(
    session: &Arc<Mutex<ServiceSession>>,
    backup_storage: &Arc<Mutex<BackupStorage>>,
    register_mm_req: OnChainRegisterMmReq,
    state_tree: &Arc<Mutex<SuperficialTree>>,
    updated_state_hashes: &Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
    swap_output_json_m: &Arc<Mutex<Vec<serde_json::Map<String, Value>>>>,
    index_price: u64,
) -> std::result::Result<(Option<OrderTab>, Option<PerpPosition>, VlpNote), String> {
    //

    // ? Get vlp close order fields -------------------
    if register_mm_req.vlp_close_order_fields.is_none() {
        return Err("vlp_close_order_fields is None".to_string());
    }
    let vlp_close_order_fields = verify_close_order_fields(
        register_mm_req
            .vlp_close_order_fields
            .as_ref()
            .unwrap()
            .clone(),
    )?;
    let signature = Signature::try_from(register_mm_req.signature.as_ref().unwrap().clone())
        .map_err(|err| err.to_string())?;

    let prev_order_tab: Option<OrderTab>;
    let new_order_tab: Option<OrderTab>;
    let prev_position: Option<PerpPosition>;
    let new_position: Option<PerpPosition>;
    let vlp_amount: u64;
    if register_mm_req.order_tab.is_some() {
        let mut order_tab = verify_order_tab_validity(&register_mm_req)?;

        prev_order_tab = Some(order_tab.clone());
        prev_position = None;

        let base_token = order_tab.tab_header.base_token;

        // ? Verify the signature ---------------------------------------------------------------------
        let valid = verfiy_open_order_sig(
            &order_tab.tab_header.pub_key,
            &order_tab.hash,
            register_mm_req.vlp_token,
            register_mm_req.max_vlp_supply,
            &vlp_close_order_fields,
            &signature,
        );
        if !valid {
            return Err("Invalid Signature".to_string());
        }

        // ? Calculate the vLP amount ------------
        vlp_amount = get_vlp_amount(
            base_token,
            order_tab.base_amount,
            order_tab.quote_amount,
            index_price,
        );

        // ? Update the order tab -----------------
        order_tab.tab_header.is_smart_contract = true;
        order_tab.tab_header.vlp_token = register_mm_req.vlp_token;
        order_tab.tab_header.max_vlp_supply = register_mm_req.max_vlp_supply;

        order_tab.vlp_supply = vlp_amount;

        order_tab.tab_header.update_hash();
        order_tab.update_hash();

        new_order_tab = Some(order_tab);
        new_position = None;
    } else {
        let mut position = verify_position_validity(&register_mm_req)?;

        prev_position = Some(position.clone());
        prev_order_tab = None;

        // ? Verify the signature ---------------------------------------------------------------------
        let valid = verfiy_open_order_sig(
            &position.position_header.position_address,
            &position.hash,
            register_mm_req.vlp_token,
            register_mm_req.max_vlp_supply,
            &vlp_close_order_fields,
            &signature,
        );
        if !valid {
            return Err("Invalid Signature".to_string());
        }

        vlp_amount = position.margin;

        // ? Update the position -----------------

        position.position_header.vlp_token = register_mm_req.vlp_token;
        position.position_header.max_vlp_supply = register_mm_req.max_vlp_supply;

        position.vlp_supply = vlp_amount;

        position.position_header.update_hash();
        position.hash = position.hash_position();

        new_order_tab = None;
        new_position = Some(position);
    }

    // ? Mint the new amount of vLP tokens using the vLP_close_order_fields
    let mut state_tree_lock = state_tree.lock();
    let zero_idx = state_tree_lock.first_zero_idx();
    drop(state_tree_lock);

    let vlp_note = VlpNote::new(
        zero_idx,
        vlp_close_order_fields.dest_received_address.clone(),
        register_mm_req.vlp_token,
        vlp_amount,
        vlp_amount,
        vlp_close_order_fields.dest_received_blinding.clone(),
    );

    // ? GENERATE THE JSON_OUTPUT -----------------------------------------------------------------
    onchain_register_json_output(
        &swap_output_json_m,
        &prev_order_tab,
        &new_order_tab,
        &prev_position,
        &new_position,
        &vlp_close_order_fields,
        &vlp_note,
        register_mm_req.max_vlp_supply,
        index_price,
        &signature,
    );

    // ? UPDATE THE STATE TREE --------------------------------------------------------------------
    onchain_register_mm_state_updates(
        state_tree,
        updated_state_hashes,
        &new_order_tab,
        &new_position,
        &vlp_note,
    );

    // ? UPDATE THE DATABASE ----------------------------------------------------------------------
    onchain_open_tab_db_updates(
        session,
        backup_storage,
        new_order_tab.clone(),
        new_position.clone(),
        vlp_note.clone(),
    );

    return Ok((new_order_tab, new_position, vlp_note));
}

//

// * HELPERS =======================================================================================
