use num_bigint::BigUint;

use crate::{
    order_tab::OrderTab,
    perpetual::{
        perp_order::CloseOrderFields, perp_position::PerpPosition, COLLATERAL_TOKEN_DECIMALS,
        DECIMALS_PER_ASSET, PRICE_DECIMALS_PER_ASSET,
    },
    server::grpc::engine_proto::{GrpcCloseOrderFields, OnChainRegisterMmReq},
    utils::crypto_utils::{pedersen_on_vec, verify, Signature},
};

/// Verify the order tab is valid
pub fn verify_order_tab_validity(
    register_mm_req: &OnChainRegisterMmReq,
) -> Result<OrderTab, String> {
    if register_mm_req.order_tab.is_none() {
        return Err("Order tab is not defined".to_string());
    }
    let order_tab = OrderTab::try_from(register_mm_req.order_tab.as_ref().unwrap().clone());
    if let Err(e) = order_tab {
        return Err("Order tab is not properly defined: ".to_string() + &e.to_string());
    }
    let order_tab = order_tab.unwrap();

    // ? Verify this is a smart_contract initiated order tab
    if order_tab.tab_header.is_smart_contract {
        return Err("This is already a smart contract initiated order tab".to_string());
    }

    return Ok(order_tab);
}

pub fn verify_close_order_fields(
    vlp_close_order_fields: GrpcCloseOrderFields,
) -> Result<CloseOrderFields, String> {
    let vlp_close_order_fields = CloseOrderFields::try_from(vlp_close_order_fields);
    if let Err(e) = vlp_close_order_fields {
        return Err(e.to_string());
    }
    let vlp_close_order_fields = vlp_close_order_fields.unwrap();

    return Ok(vlp_close_order_fields);
}

pub fn get_vlp_amount(
    base_token: u32,
    base_amount: u64,
    quote_amount: u64,
    index_price: u64,
) -> u64 {
    // ? calculate the right amount of vLP tokens to mint using the index price
    // ? Get the input nominal value with the index price
    // ? init token price is 1
    let base_decimals: &u8 = DECIMALS_PER_ASSET.get(&base_token.to_string()).unwrap();
    let base_price_decimals: &u8 = PRICE_DECIMALS_PER_ASSET
        .get(&base_token.to_string())
        .unwrap();

    let decimal_conversion = *base_decimals + *base_price_decimals - COLLATERAL_TOKEN_DECIMALS;
    let multiplier = 10_u128.pow(decimal_conversion as u32);

    let base_nominal = base_amount as u128 * index_price as u128 / multiplier;
    let vlp_amount = base_nominal as u64 + quote_amount;

    return vlp_amount;
}

// * ----------------------------------------------------------------------------

/// Verify the order tab is valid
pub fn verify_position_validity(
    register_mm_req: &OnChainRegisterMmReq,
) -> Result<PerpPosition, String> {
    if register_mm_req.position.is_none() {
        return Err("Position is not defined".to_string());
    }
    let position = PerpPosition::try_from(register_mm_req.position.as_ref().unwrap().clone());
    if let Err(e) = position {
        return Err("Position is not properly defined: ".to_string() + &e.to_string());
    }
    let position = position.unwrap();

    // ? Verify this is a smart_contract initiated order tab
    if position.vlp_supply > 0 {
        return Err("This is already a smart contract initiated position".to_string());
    }

    return Ok(position);
}

// * ----------------------------------------------------------------------------

/// Verify the signature for the order tab hash
pub fn verfiy_open_order_sig(
    address: &BigUint,
    hash: &BigUint,
    vlp_token: u32,
    max_vlp_supply: u64,
    close_order_fields: &CloseOrderFields,
    signature: &Signature,
) -> bool {
    // & header_hash = H({address, hash, vlp_token, max_vlp_supply, close_order_fields_hash})

    let mut hash_inputs: Vec<&BigUint> = Vec::new();

    hash_inputs.push(&address);
    hash_inputs.push(&hash);

    let vlp_token = BigUint::from(vlp_token);
    hash_inputs.push(&vlp_token);

    let max_vlp_supply = BigUint::from(max_vlp_supply);
    hash_inputs.push(&max_vlp_supply);

    let h = close_order_fields.hash();
    hash_inputs.push(&h);

    let hash = pedersen_on_vec(&hash_inputs);

    let valid = verify(&address, &hash, signature);

    return valid;
}
