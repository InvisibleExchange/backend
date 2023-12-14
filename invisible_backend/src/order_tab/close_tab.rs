use std::{collections::HashMap, sync::Arc};

use num_bigint::BigUint;
use num_traits::FromPrimitive;
use parking_lot::Mutex;

use firestore_db_and_auth::ServiceSession;
use serde_json::Value;

use crate::{
    perpetual::{perp_order::CloseOrderFields, DUST_AMOUNT_PER_ASSET},
    server::grpc::engine_proto::CloseOrderTabReq,
    transaction_batch::LeafNodeType,
    trees::superficial_tree::SuperficialTree,
    utils::{crypto_utils::hash_many, notes::Note, storage::local_storage::BackupStorage},
};

use crate::utils::crypto_utils::{verify, Signature};

use super::{
    db_updates::close_tab_db_updates, json_output::close_tab_json_output,
    state_updates::close_tab_state_updates, OrderTab,
};

pub fn close_order_tab(
    session: &Arc<Mutex<ServiceSession>>,
    backup_storage: &Arc<Mutex<BackupStorage>>,
    state_tree: &Arc<Mutex<SuperficialTree>>,
    updated_state_hashes: &Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
    swap_output_json_m: &Arc<Mutex<Vec<serde_json::Map<String, Value>>>>,
    //
    close_order_tab_req: CloseOrderTabReq,
) -> std::result::Result<(Note, Note), String> {
    // ? CHECK THAT THE REQUEST IS VALID ---------------------------------------------------
    if close_order_tab_req.order_tab.is_none() {
        return Err("order tab is missing".to_string());
    }
    if close_order_tab_req.base_close_order_fields.is_none()
        || close_order_tab_req.quote_close_order_fields.is_none()
    {
        return Err("close order fields are not defined".to_string());
    }
    if close_order_tab_req.signature.is_none() {
        return Err("signature is missing".to_string());
    }

    // ? PARSE THE REQUEST -----------------------------------------------------------------
    let order_tab = OrderTab::try_from(close_order_tab_req.order_tab.unwrap());
    let base_close_order_fields =
        CloseOrderFields::try_from(close_order_tab_req.base_close_order_fields.unwrap());
    let quote_close_order_fields =
        CloseOrderFields::try_from(close_order_tab_req.quote_close_order_fields.unwrap());

    if order_tab.is_err() || base_close_order_fields.is_err() || quote_close_order_fields.is_err() {
        return Err("failed to parse order tab or close order fields".to_string());
    }
    let order_tab = order_tab.unwrap();
    let base_close_order_fields = base_close_order_fields.unwrap();
    let quote_close_order_fields = quote_close_order_fields.unwrap();

    let base_token = order_tab.tab_header.base_token;
    let base_amount_change = close_order_tab_req.base_amount_change;
    let quote_token = order_tab.tab_header.quote_token;
    let quote_amount_change = close_order_tab_req.quote_amount_change;

    if base_amount_change > order_tab.base_amount || quote_amount_change > order_tab.quote_amount {
        return Err("amount change is greater than the tab amount".to_string());
    }

    // ? CHECK THAT THE ORDER TAB EXISTS ---------------------------------------------------
    let mut state_tree_m = state_tree.lock();
    let leaf_hash = state_tree_m.get_leaf_by_index(order_tab.tab_idx as u64);
    if leaf_hash != order_tab.hash {
        return Err("order tab does not exist".to_string());
    }

    let zero_idx1 = state_tree_m.first_zero_idx();
    let zero_idx2 = state_tree_m.first_zero_idx();
    drop(state_tree_m);

    // ? VERIFY THE SIGNATURE --------------------------------------------------------------
    let signature = Signature::try_from(close_order_tab_req.signature.unwrap_or_default())
        .map_err(|err| err.to_string())?;
    let valid = verfiy_close_order_hash(
        &order_tab,
        base_amount_change,
        quote_amount_change,
        &base_close_order_fields,
        &quote_close_order_fields,
        &signature,
    );
    if !valid {
        return Err("Invalid Signature".to_string());
    }

    // ? GENERATE THE RETURN NOTES ---------------------------------------------------------
    let base_return_note = Note::new(
        zero_idx1,
        base_close_order_fields.dest_received_address.clone(),
        base_token,
        base_amount_change,
        base_close_order_fields.dest_received_blinding.clone(),
    );
    let quote_return_note = Note::new(
        zero_idx2,
        quote_close_order_fields.dest_received_address.clone(),
        quote_token,
        quote_amount_change,
        quote_close_order_fields.dest_received_blinding.clone(),
    );

    // ? UPDATE THE ORDER TAB --------------------------------------------------------------
    let updated_base_amount = order_tab.base_amount - base_amount_change;
    let updated_quote_amount = order_tab.quote_amount - quote_amount_change;

    let updated_order_tab;
    if (updated_base_amount > DUST_AMOUNT_PER_ASSET[&base_token.to_string()])
        && (updated_quote_amount > DUST_AMOUNT_PER_ASSET[&quote_token.to_string()])
    {
        updated_order_tab = Some(OrderTab::new(
            order_tab.tab_header.clone(),
            updated_base_amount,
            updated_quote_amount,
        ));
    } else {
        updated_order_tab = None;
    }

    // ? GENERATE THE JSON_OUTPUT ----------------------------------------------------------
    close_tab_json_output(
        &swap_output_json_m,
        base_amount_change,
        quote_amount_change,
        &base_return_note,
        &quote_return_note,
        &base_close_order_fields,
        &quote_close_order_fields,
        &order_tab,
        &updated_order_tab,
        &signature,
    );

    // ? UPDATE THE STATE -----------------------------------------------------------------
    close_tab_state_updates(
        state_tree,
        updated_state_hashes,
        &order_tab,
        &updated_order_tab,
        base_return_note.clone(),
        quote_return_note.clone(),
    );

    // ? UPDATE THE DATABASE ---------------------------------------------------------------
    close_tab_db_updates(
        session,
        backup_storage,
        &order_tab,
        &updated_order_tab,
        base_return_note.clone(),
        quote_return_note.clone(),
    );

    Ok((base_return_note, quote_return_note))
}

/// Verify the signature for the order tab hash
pub fn verfiy_close_order_hash(
    order_tab: &OrderTab,
    base_amount_change: u64,
    quote_amount_change: u64,
    base_close_order_fields: &CloseOrderFields,
    quote_close_order_fields: &CloseOrderFields,
    signature: &Signature,
) -> bool {
    // & header_hash = H({order_tab_hash, base_amount_change, quote_amount_change, base_close_order_fields.hash, quote_close_order_fields.hash})

    let mut hash_inputs: Vec<&BigUint> = Vec::new();

    hash_inputs.push(&order_tab.hash);

    let base_change = BigUint::from_u64(base_amount_change).unwrap();
    let quote_change = BigUint::from_u64(quote_amount_change).unwrap();
    hash_inputs.push(&base_change);
    hash_inputs.push(&quote_change);

    let base_close_order_fields_hash = base_close_order_fields.hash();
    hash_inputs.push(&base_close_order_fields_hash);
    let quote_close_order_fields_hash = quote_close_order_fields.hash();
    hash_inputs.push(&quote_close_order_fields_hash);

    let hash = hash_many(&hash_inputs);

    println!("hash: {:?}", hash);
    println!("signature: {:?}", signature);
    println!("pub_key: {:?}", order_tab.tab_header.pub_key);

    let valid = verify(&order_tab.tab_header.pub_key, &hash, signature);

    return valid;
}
