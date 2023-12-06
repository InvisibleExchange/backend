use std::sync::Arc;

use num_bigint::BigUint;
use parking_lot::Mutex;
use starknet::curve::AffinePoint;

use crate::{
    order_tab::OrderTab,
    perpetual::{
        perp_order::CloseOrderFields, COLLATERAL_TOKEN_DECIMALS, DECIMALS_PER_ASSET,
        DUST_AMOUNT_PER_ASSET, PRICE_DECIMALS_PER_ASSET,
    },
    server::grpc::engine_proto::{
        OnChainRemoveLiqTabReq, PositionRemoveLiquidityReq, TabRemoveLiquidityReq,
    },
    smart_contract_mms::vlp_note::VlpNote,
    trees::superficial_tree::SuperficialTree,
    utils::crypto_utils::{pedersen_on_vec, EcPoint},
};

use crate::utils::crypto_utils::{verify, Signature};

/// Verify that the VLP notes are valid and exist in the state
pub fn verify_vlp_notes(
    state_tree: &Arc<Mutex<SuperficialTree>>,
    remove_liquidity_req: &OnChainRemoveLiqTabReq,
) -> Result<(Vec<VlpNote>, u64, u64, BigUint), String> {
    // ? Verify the notes are valid and exist in the state
    let vlp_notes_in = remove_liquidity_req
        .vlp_notes_in
        .iter()
        .map(|n| VlpNote::try_from(n.clone()).unwrap())
        .collect::<Vec<VlpNote>>();
    let vlp_amount = vlp_notes_in.iter().map(|n| n.amount).sum::<u64>();
    let vlp_initial_value = vlp_notes_in.iter().map(|n| n.initial_value).sum::<u64>();

    let mut pub_key_sum: AffinePoint = AffinePoint::identity();

    let vlp_token;
    if let Some(tab) = remove_liquidity_req.tab_remove_liquidity_req.as_ref() {
        let tab = tab.order_tab.as_ref().unwrap();
        vlp_token = tab.tab_header.as_ref().unwrap().vlp_token;
    } else {
        let pos = remove_liquidity_req
            .position_remove_liquidity_req
            .as_ref()
            .unwrap()
            .position
            .as_ref()
            .unwrap();
        vlp_token = pos.position_header.as_ref().unwrap().vlp_token;
    }

    let state_tree_m = state_tree.lock();
    for note in vlp_notes_in.iter() {
        let ec_point = AffinePoint::from(&note.address);
        pub_key_sum = &pub_key_sum + &ec_point;

        if note.token != vlp_token {
            return Err("vLP token mismatch".to_string());
        }

        if state_tree_m.get_leaf_by_index(note.index) != note.hash {
            return Err("vLP note does not exist".to_string());
        }
    }
    drop(state_tree_m);

    let pub_key_sum = EcPoint::from(&pub_key_sum).x.to_biguint().unwrap();

    Ok((vlp_notes_in, vlp_amount, vlp_initial_value, pub_key_sum))
}

pub fn get_close_order_fields(
    tab_remove_liquidity_req: &TabRemoveLiquidityReq,
) -> Result<(CloseOrderFields, CloseOrderFields), String> {
    let base_close_order_fields = CloseOrderFields::try_from(
        tab_remove_liquidity_req
            .base_close_order_fields
            .as_ref()
            .unwrap()
            .clone(),
    );
    if let Err(e) = base_close_order_fields {
        return Err(e.to_string());
    }
    let base_close_order_fields = base_close_order_fields.unwrap();

    let quote_close_order_fields = CloseOrderFields::try_from(
        tab_remove_liquidity_req
            .quote_close_order_fields
            .as_ref()
            .unwrap()
            .clone(),
    );
    if let Err(e) = quote_close_order_fields {
        return Err(e.to_string());
    }
    let quote_close_order_fields = quote_close_order_fields.unwrap();

    Ok((base_close_order_fields, quote_close_order_fields))
}

pub fn get_base_close_amounts(
    is_full_close: bool,
    order_tab: &OrderTab,
    base_return_amount_: u64,
    index_price: u64,
    vlp_amount: u64,
) -> Result<(u64, u64), String> {
    let base_return_amount;
    let quote_return_amount;
    if is_full_close {
        base_return_amount = order_tab.base_amount;
        quote_return_amount = order_tab.quote_amount;
    } else {
        // ? make sure:   vlp_amount * vlp_price = base_amount * index_price + quote_amount
        // ? The market maker specifies amount of base_token to return and the quote token is calculated here to make sure the above equation holds
        let base_decimals: &u8 = DECIMALS_PER_ASSET
            .get(&order_tab.tab_header.base_token.to_string())
            .unwrap();
        let base_price_decimals: &u8 = PRICE_DECIMALS_PER_ASSET
            .get(&order_tab.tab_header.base_token.to_string())
            .unwrap();

        let decimal_conversion = *base_decimals + *base_price_decimals - COLLATERAL_TOKEN_DECIMALS;
        let multiplier = 10_u128.pow(decimal_conversion as u32);

        let base_nominal: u128 = base_return_amount_ as u128 * index_price as u128 / multiplier;

        let tab_base_nominal = order_tab.base_amount as u128 * index_price as u128 / multiplier;
        let tab_nominal = tab_base_nominal + order_tab.quote_amount as u128;

        // ? quote_amount = (vLP_amount*tab_nominal)/vLP_supply - base_amount*indexPrice
        quote_return_amount = ((vlp_amount as u128 * tab_nominal) / order_tab.vlp_supply as u128
            - base_nominal) as u64;
        base_return_amount = base_return_amount_;
    }

    if quote_return_amount
        > order_tab.quote_amount
            + DUST_AMOUNT_PER_ASSET[&order_tab.tab_header.quote_token.to_string()]
    {
        return Err("quote_return_amount is too large".to_string());
    }
    if base_return_amount
        > order_tab.base_amount
            + DUST_AMOUNT_PER_ASSET[&order_tab.tab_header.base_token.to_string()]
    {
        return Err("base_return_amount is too large".to_string());
    }

    return Ok((base_return_amount, quote_return_amount));
}

/// Verify the signature for the order tab hash
pub fn verfiy_remove_liquidity_sig(
    index_price: u64,
    slippage: u32,
    base_close_order_fields: &CloseOrderFields,
    quote_close_order_fields: &CloseOrderFields,
    pub_key: &BigUint, // Pub key of position/tab
    pub_key_sum: &BigUint,
    signature: &Signature,
) -> bool {
    //

    // & hash = H({index_price, slippage, base_close_order_fields_hash, quote_close_order_fields_hash, pub_key})

    let mut hash_inputs: Vec<&BigUint> = vec![];

    let index_price = BigUint::from(index_price);
    hash_inputs.push(&index_price);
    let slippage = BigUint::from(slippage);
    hash_inputs.push(&slippage);

    let base_fields_hash = base_close_order_fields.hash();
    hash_inputs.push(&base_fields_hash);
    let quote_fields_hash = quote_close_order_fields.hash();
    hash_inputs.push(&quote_fields_hash);

    hash_inputs.push(pub_key);

    let hash = pedersen_on_vec(&hash_inputs);

    let valid = verify(pub_key_sum, &hash, signature);

    return valid;
}

// * POSITION SMART CONTRACT MM ====================================================================

pub fn position_get_close_order_fields(
    position_remove_liquidity_req: &PositionRemoveLiquidityReq,
) -> Result<CloseOrderFields, String> {
    let collateral_close_order_fields = CloseOrderFields::try_from(
        position_remove_liquidity_req
            .collateral_close_order_fields
            .as_ref()
            .unwrap()
            .clone(),
    );
    if let Err(e) = collateral_close_order_fields {
        return Err(e.to_string());
    }
    let collateral_close_order_fields = collateral_close_order_fields.unwrap();

    Ok(collateral_close_order_fields)
}

/// Verify the signature for the order tab hash
pub fn verfiy_position_remove_liquidity_sig(
    collateral_close_order_fields: &CloseOrderFields,
    position_address: &BigUint, // Pub key of position/tab
    pub_key_sum: &BigUint,
    signature: &Signature,
) -> bool {
    //

    let mut hash_inputs: Vec<&BigUint> = vec![];

    // & hash = H({close_order_fields_hash, position_address})

    let base_fields_hash = collateral_close_order_fields.hash();
    hash_inputs.push(&base_fields_hash);

    hash_inputs.push(position_address);

    let hash = pedersen_on_vec(&hash_inputs);

    let valid = verify(pub_key_sum, &hash, signature);

    return valid;
}

pub fn get_return_collateral_amount(vlp_amount: u64, vlp_supply: u64, margin: u64) -> u64 {
    let return_collateral = (vlp_amount as u128 * margin as u128) / vlp_supply as u128;

    return return_collateral as u64;
}
