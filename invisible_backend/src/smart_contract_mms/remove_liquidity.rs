use std::{collections::HashMap, sync::Arc};

use num_bigint::BigUint;
use parking_lot::Mutex;
use serde_json::Value;

use firestore_db_and_auth::ServiceSession;

use crate::{
    order_tab::OrderTab,
    perpetual::{perp_position::PerpPosition, COLLATERAL_TOKEN, DUST_AMOUNT_PER_ASSET},
    server::grpc::engine_proto::OnChainRemoveLiqTabReq,
    transaction_batch::LeafNodeType,
    trees::superficial_tree::SuperficialTree,
    utils::{notes::Note, storage::local_storage::BackupStorage},
};

use crate::utils::crypto_utils::Signature;

use super::helpers::{
    db_updates::{
        onchain_position_remove_liquidity_db_updates, onchain_tab_remove_liquidity_db_updates,
    },
    json_output::{
        onchain_position_remove_liquidity_json_output, onchain_tab_remove_liquidity_json_output,
    },
    remove_liquidity_helpers::{
        get_base_close_amounts, get_close_order_fields, get_return_collateral_amount,
        position_get_close_order_fields, verfiy_position_remove_liquidity_sig,
        verfiy_remove_liquidity_sig, verify_vlp_notes,
    },
    state_updates::{
        onchain_position_remove_liquidity_state_updates, onchain_tab_remove_liquidity_state_updates,
    },
};

pub type RemoveLiqRes = (
    Option<(Option<OrderTab>, Note, Note)>,
    Option<(Option<PerpPosition>, Note)>,
);

/// Claim the deposit that was created onchain
pub fn remove_liquidity_from_order_tab(
    session: &Arc<Mutex<ServiceSession>>,
    backup_storage: &Arc<Mutex<BackupStorage>>,
    remove_liquidity_req: OnChainRemoveLiqTabReq,
    state_tree: &Arc<Mutex<SuperficialTree>>,
    updated_state_hashes: &Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
    swap_output_json_m: &Arc<Mutex<Vec<serde_json::Map<String, Value>>>>,
    index_price: u64,
) -> std::result::Result<RemoveLiqRes, String> {
    //

    // ? Get vlp notes
    let (vlp_notes_in, vlp_amount, pub_key_sum) =
        verify_vlp_notes(state_tree, &remove_liquidity_req)?;

    if remove_liquidity_req.tab_remove_liquidity_req.is_some() {
        let tab_remove_liquidity_req = remove_liquidity_req.tab_remove_liquidity_req.unwrap();

        let order_tab =
            OrderTab::try_from(tab_remove_liquidity_req.order_tab.as_ref().unwrap().clone());
        if let Err(e) = order_tab {
            return Err("Order tab is not properly defined: ".to_string() + &e.to_string());
        }
        let order_tab = order_tab.unwrap();

        let (base_close_order_fields, quote_close_order_fields) =
            get_close_order_fields(&tab_remove_liquidity_req)?;

        // ? Verify the signature ---------------------------------------------------------------------
        let signature = Signature::try_from(remove_liquidity_req.signature.unwrap_or_default())
            .map_err(|err| err.to_string())?;
        let valid = verfiy_remove_liquidity_sig(
            tab_remove_liquidity_req.index_price,
            tab_remove_liquidity_req.slippage,
            &base_close_order_fields,
            &quote_close_order_fields,
            &order_tab.tab_header.pub_key,
            &pub_key_sum,
            &signature,
        );
        if !valid {
            return Err("Invalid Signature".to_string());
        }

        let is_full_close = vlp_amount
            >= order_tab.vlp_supply - DUST_AMOUNT_PER_ASSET[&COLLATERAL_TOKEN.to_string()];

        // ? Verify the execution index price is within slippage range of users price
        // slippage: 10_000 = 100% ; 100 = 1%; 1 = 0.01%
        let max_slippage_price = tab_remove_liquidity_req.index_price
            * (10_000 - tab_remove_liquidity_req.slippage as u64)
            / 10_000;
        if index_price < max_slippage_price {
            return Err("Execution price is not within slippage range".to_string());
        }

        let (base_return_amount, quote_return_amount) = get_base_close_amounts(
            is_full_close,
            &order_tab,
            tab_remove_liquidity_req.base_return_amount,
            index_price,
            vlp_amount,
        )?;

        // ? create the new notes for the user
        let mut state_tree_m = state_tree.lock();
        let zero_idx1 = state_tree_m.first_zero_idx();
        let zero_idx2 = state_tree_m.first_zero_idx();
        drop(state_tree_m);

        let base_return_note = Note::new(
            zero_idx1,
            base_close_order_fields.dest_received_address.clone(),
            order_tab.tab_header.base_token,
            base_return_amount,
            base_close_order_fields.dest_received_blinding.clone(),
        );
        let quote_return_note = Note::new(
            zero_idx2,
            quote_close_order_fields.dest_received_address.clone(),
            order_tab.tab_header.quote_token,
            quote_return_amount,
            quote_close_order_fields.dest_received_blinding.clone(),
        );

        // ? Adding to an existing order tab
        let prev_order_tab = order_tab;

        let mut new_order_tab: Option<OrderTab> = None;
        if !is_full_close {
            let mut new_tab = prev_order_tab.clone();

            new_tab.base_amount -= base_return_amount;
            new_tab.quote_amount -= quote_return_amount;
            new_tab.vlp_supply -= vlp_amount;

            new_tab.update_hash();

            new_order_tab = Some(new_tab)
        }

        // ? update the state tree, json_output and database
        // ? GENERATE THE JSON_OUTPUT -----------------------------------------------------------------
        onchain_tab_remove_liquidity_json_output(
            swap_output_json_m,
            &vlp_notes_in,
            tab_remove_liquidity_req.index_price,
            tab_remove_liquidity_req.slippage,
            &base_close_order_fields,
            &quote_close_order_fields,
            tab_remove_liquidity_req.base_return_amount,
            index_price,
            &prev_order_tab,
            &new_order_tab,
            &base_return_note,
            &quote_return_note,
            &signature,
        );

        // ? UPDATE THE STATE TREE --------------------------------------------------------------------
        onchain_tab_remove_liquidity_state_updates(
            state_tree,
            updated_state_hashes,
            prev_order_tab.tab_idx as u64,
            &new_order_tab,
            &vlp_notes_in,
            &base_return_note,
            &quote_return_note,
        );

        // ? UPDATE THE DATABASE ----------------------------------------------------------------------
        onchain_tab_remove_liquidity_db_updates(
            session,
            backup_storage,
            prev_order_tab.tab_idx as u64,
            prev_order_tab.tab_header.pub_key,
            new_order_tab.clone(),
            &vlp_notes_in,
            base_return_note.clone(),
            quote_return_note.clone(),
        );

        return Ok((
            Some((new_order_tab, base_return_note, quote_return_note)),
            None,
        ));
    } else {
        let position_remove_liquidity_req =
            remove_liquidity_req.position_remove_liquidity_req.unwrap();

        let position = PerpPosition::try_from(
            position_remove_liquidity_req
                .position
                .as_ref()
                .unwrap()
                .clone(),
        );
        if let Err(e) = position {
            return Err("Order tab is not properly defined: ".to_string() + &e.to_string());
        }
        let position = position.unwrap();

        let collateral_close_order_fields =
            position_get_close_order_fields(&position_remove_liquidity_req)?;

        // ? Verify the signature ---------------------------------------------------------------------
        let signature = Signature::try_from(remove_liquidity_req.signature.unwrap_or_default())
            .map_err(|err| err.to_string())?;
        let valid = verfiy_position_remove_liquidity_sig(
            &collateral_close_order_fields,
            &position.position_header.position_address,
            &pub_key_sum,
            &signature,
        );
        if !valid {
            return Err("Invalid Signature".to_string());
        }

        let is_full_close = vlp_amount
            >= position.vlp_supply - DUST_AMOUNT_PER_ASSET[&COLLATERAL_TOKEN.to_string()];

        let return_collateral_amount =
            get_return_collateral_amount(vlp_amount, position.vlp_supply, position.margin);

        // ? create the new note for the user
        let mut state_tree_m = state_tree.lock();
        let zero_idx1 = state_tree_m.first_zero_idx();
        drop(state_tree_m);

        let collateral_return_note = Note::new(
            zero_idx1,
            collateral_close_order_fields.dest_received_address.clone(),
            COLLATERAL_TOKEN,
            return_collateral_amount,
            collateral_close_order_fields.dest_received_blinding.clone(),
        );

        // ? Adding to an existing order tab
        let prev_position = position;

        let mut new_position: Option<PerpPosition> = None;
        if !is_full_close {
            let mut new_pos = prev_position.clone();

            new_pos.margin -= return_collateral_amount;
            new_pos.vlp_supply -= vlp_amount;
            new_pos.update_position_info();

            new_position = Some(new_pos)
        }

        // ? GENERATE THE JSON_OUTPUT -----------------------------------------------------------------
        onchain_position_remove_liquidity_json_output(
            swap_output_json_m,
            &vlp_notes_in,
            &collateral_close_order_fields,
            &prev_position,
            &new_position,
            &collateral_return_note,
            &signature,
        );

        // ? UPDATE THE STATE TREE --------------------------------------------------------------------
        onchain_position_remove_liquidity_state_updates(
            state_tree,
            updated_state_hashes,
            prev_position.index as u64,
            &new_position,
            &vlp_notes_in,
            &collateral_return_note,
        );

        // ? UPDATE THE DATABASE ----------------------------------------------------------------------
        onchain_position_remove_liquidity_db_updates(
            session,
            backup_storage,
            prev_position.index as u64,
            prev_position.position_header.position_address,
            new_position.clone(),
            &vlp_notes_in,
            collateral_return_note.clone(),
        );

        return Ok((None, Some((new_position, collateral_return_note))));
    }
}
