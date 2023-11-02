use std::{collections::HashMap, sync::Arc};

use num_bigint::BigUint;
use parking_lot::Mutex;
use serde_json::Value;

use firestore_db_and_auth::ServiceSession;

use crate::{
    order_tab::OrderTab,
    perpetual::perp_position::PerpPosition,
    server::grpc::engine_proto::OnChainAddLiqTabReq,
    transaction_batch::LeafNodeType,
    trees::superficial_tree::SuperficialTree,
    utils::{notes::Note, storage::local_storage::BackupStorage},
};

use crate::utils::crypto_utils::Signature;

use super::helpers::{
    add_liquidity_helpers::{
        calculate_pos_vlp_amount, calculate_vlp_amount, verfiy_add_liquidity_sig,
        verfiy_pos_add_liquidity_sig, verify_close_order_fields, verify_note_validity,
        verify_order_tab_validity, verify_pos_note_validity, verify_position_validity,
    },
    db_updates::{onchain_position_add_liquidity_db_updates, onchain_tab_add_liquidity_db_updates},
    json_output::{
        onchain_position_add_liquidity_json_output, onchain_tab_add_liquidity_json_output,
    },
    state_updates::{
        onchain_position_add_liquidity_state_updates, onchain_tab_add_liquidity_state_updates,
    },
};

/// Claim the deposit that was created onchain
pub fn add_liquidity_to_mm(
    session: &Arc<Mutex<ServiceSession>>,
    backup_storage: &Arc<Mutex<BackupStorage>>,
    add_liquidity_req: OnChainAddLiqTabReq,
    state_tree: &Arc<Mutex<SuperficialTree>>,
    updated_state_hashes: &Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
    swap_output_json_m: &Arc<Mutex<Vec<serde_json::Map<String, Value>>>>,
    index_price: u64,
) -> std::result::Result<(Option<OrderTab>, Option<PerpPosition>, Note), String> {
    //

    if add_liquidity_req.tab_add_liquidity_req.is_some() {
        let tab_add_liquidity_req = add_liquidity_req.tab_add_liquidity_req.unwrap();

        let mut order_tab = verify_order_tab_validity(&tab_add_liquidity_req)?;

        if add_liquidity_req.vlp_close_order_fields.is_none() {
            return Err("vlp_close_order_fields is None".to_string());
        }
        let vlp_close_order_fields =
            verify_close_order_fields(add_liquidity_req.vlp_close_order_fields.unwrap())?;

        let (
            base_notes_in,
            base_refund_note,
            quote_notes_in,
            quote_refund_note,
            base_amount,
            quote_amount,
            pub_key_sum,
        ) = verify_note_validity(
            state_tree,
            tab_add_liquidity_req,
            order_tab.tab_header.base_token,
            order_tab.tab_header.quote_token,
        )?;

        // ? Verify the signature ---------------------------------------------------------------------
        let signature = Signature::try_from(add_liquidity_req.signature.unwrap_or_default())
            .map_err(|err| err.to_string())?;

        let valid = verfiy_add_liquidity_sig(
            &base_refund_note,
            &quote_refund_note,
            &order_tab.tab_header.pub_key,
            &vlp_close_order_fields,
            &pub_key_sum,
            &signature,
        );
        if !valid {
            return Err("Invalid Signature".to_string());
        }

        let vlp_amount = calculate_vlp_amount(&order_tab, base_amount, quote_amount, index_price);

        // ? Update the order tab ---------------------------------------------------------------------
        // ? Verify that the order tab exists
        let mut state_tree_m = state_tree.lock();
        let zero_idx = state_tree_m.first_zero_idx();

        let leaf_hash = state_tree_m.get_leaf_by_index(order_tab.tab_idx as u64);
        if leaf_hash != order_tab.hash {
            return Err("order tab does not exist".to_string());
        }
        drop(state_tree_m);

        // ? Adding to an existing order tab
        let prev_order_tab = order_tab.clone();

        order_tab.base_amount += base_amount;
        order_tab.quote_amount += quote_amount;
        order_tab.vlp_supply += vlp_amount;

        order_tab.update_hash();

        // ? Mint the new amount of vLP tokens using the vLP_close_order_fields

        let vlp_note = Note::new(
            zero_idx,
            vlp_close_order_fields.dest_received_address.clone(),
            order_tab.tab_header.vlp_token,
            vlp_amount,
            vlp_close_order_fields.dest_received_blinding.clone(),
        );

        // ? GENERATE THE JSON_OUTPUT -----------------------------------------------------------------
        onchain_tab_add_liquidity_json_output(
            &swap_output_json_m,
            &prev_order_tab,
            &base_notes_in,
            &base_refund_note,
            &quote_notes_in,
            &quote_refund_note,
            &order_tab.hash,
            &vlp_close_order_fields,
            &vlp_note,
            index_price,
            &signature,
        );

        // ? UPDATE THE STATE TREE --------------------------------------------------------------------
        onchain_tab_add_liquidity_state_updates(
            state_tree,
            updated_state_hashes,
            &order_tab,
            &base_notes_in,
            &quote_notes_in,
            &base_refund_note,
            &quote_refund_note,
            &vlp_note,
        );

        // ? UPDATE THE DATABASE ----------------------------------------------------------------------
        onchain_tab_add_liquidity_db_updates(
            session,
            backup_storage,
            order_tab.clone(),
            base_notes_in,
            quote_notes_in,
            base_refund_note,
            quote_refund_note,
            vlp_note.clone(),
        );

        return Ok((Some(order_tab), None, vlp_note));
    } else {
        let position_add_liquidity_req = add_liquidity_req.position_add_liquidity_req.unwrap();

        let mut position = verify_position_validity(&position_add_liquidity_req)?;

        if add_liquidity_req.vlp_close_order_fields.is_none() {
            return Err("vlp_close_order_fields is None".to_string());
        }
        let vlp_close_order_fields =
            verify_close_order_fields(add_liquidity_req.vlp_close_order_fields.unwrap())?;

        let (collateral_notes_in, collateral_refund_note, collateral_amount, pub_key_sum) =
            verify_pos_note_validity(state_tree, position_add_liquidity_req)?;

        // ? Verify the signature ---------------------------------------------------------------------
        let signature = Signature::try_from(add_liquidity_req.signature.unwrap_or_default())
            .map_err(|err| err.to_string())?;

        let valid = verfiy_pos_add_liquidity_sig(
            &collateral_refund_note,
            &position.position_header.position_address,
            &vlp_close_order_fields,
            &pub_key_sum,
            &signature,
        );
        if !valid {
            return Err("Invalid Signature".to_string());
        }

        let vlp_amount = calculate_pos_vlp_amount(&position, collateral_amount);

        // ? Update the position ---------------------------------------------------------------------
        // ? Verify that the position exists
        let mut state_tree_m = state_tree.lock();
        let zero_idx = state_tree_m.first_zero_idx();

        let leaf_hash = state_tree_m.get_leaf_by_index(position.index as u64);
        if leaf_hash != position.hash {
            return Err("position does not exist".to_string());
        }
        drop(state_tree_m);

        // ? Adding to an existing position
        let prev_position = position.clone();

        position.margin += collateral_amount;
        position.vlp_supply += vlp_amount;
        position.update_position_info();

        // ? Mint the new amount of vLP tokens using the vLP_close_order_fields
        let vlp_note = Note::new(
            zero_idx,
            vlp_close_order_fields.dest_received_address.clone(),
            position.position_header.vlp_token,
            vlp_amount,
            vlp_close_order_fields.dest_received_blinding.clone(),
        );

        // ? GENERATE THE JSON_OUTPUT -----------------------------------------------------------------
        onchain_position_add_liquidity_json_output(
            &swap_output_json_m,
            &prev_position,
            &collateral_notes_in,
            &collateral_refund_note,
            &position.hash,
            &vlp_close_order_fields,
            &vlp_note,
            &signature,
        );

        // ? UPDATE THE STATE TREE --------------------------------------------------------------------
        onchain_position_add_liquidity_state_updates(
            state_tree,
            updated_state_hashes,
            &position,
            &collateral_notes_in,
            &collateral_refund_note,
            &vlp_note,
        );

        // ? UPDATE THE DATABASE ----------------------------------------------------------------------
        onchain_position_add_liquidity_db_updates(
            session,
            backup_storage,
            position.clone(),
            collateral_notes_in,
            collateral_refund_note,
            vlp_note.clone(),
        );

        return Ok((None, Some(position), vlp_note));
    }
}

//
