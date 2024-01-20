use num_bigint::BigUint;
use serde_json::{Map, Value};
use std::{collections::HashMap, str::FromStr};

use crate::{
    perpetual::{
        get_price,
        perp_position::{PerpPosition, PositionHeader, _hash_position},
        OrderSide, COLLATERAL_TOKEN, COLLATERAL_TOKEN_DECIMALS, DECIMALS_PER_ASSET,
        DUST_AMOUNT_PER_ASSET, LEVERAGE_DECIMALS, PRICE_DECIMALS_PER_ASSET,
    },
    utils::crypto_utils::EcPoint,
    utils::notes::Note,
};

pub fn update_position_open(
    transaction: &Map<String, Value>,
    prev_position: Option<PerpPosition>,
    is_a: bool,
) -> PerpPosition {
    let (
        order,
        order_side,
        spent_synthetic,
        spent_collateral,
        synthetic_token,
        current_funding_idx,
        index,
        fee_taken,
    ) = parse_order_info(transaction, is_a);

    let (_, init_margin) = get_init_margin(order, spent_synthetic);

    if let Some(mut position) = prev_position {
        let leverage = (spent_collateral as u128 * 10_u128.pow(LEVERAGE_DECIMALS as u32)
            / init_margin as u128) as u64;

        position.add_margin_to_position(init_margin, spent_synthetic, leverage, fee_taken);

        return position;
    } else {
        let leverage = (spent_collateral as u128 * 10_u128.pow(LEVERAGE_DECIMALS as u32)
            / (init_margin - fee_taken) as u128) as u64;

        let open_order_fields = order.get("open_order_fields").unwrap();

        let allow_partial_liq = open_order_fields.get("allow_partial_liquidations").unwrap();

        let allow_partial_liquidations;
        if let Some(res) = allow_partial_liq.as_bool() {
            allow_partial_liquidations = res;
        } else {
            // TODO: Delete this later
            allow_partial_liquidations = open_order_fields
                .get("allow_partial_liquidations")
                .unwrap()
                .as_str()
                .unwrap()
                == "true";
        }

        let position_address = open_order_fields
            .get("position_address")
            .unwrap()
            .as_str()
            .unwrap();
        let position_address = BigUint::from_str(position_address).unwrap();

        let position = PerpPosition::new(
            order_side,
            spent_synthetic,
            synthetic_token,
            COLLATERAL_TOKEN,
            init_margin,
            leverage,
            allow_partial_liquidations,
            position_address,
            current_funding_idx,
            index,
            fee_taken,
        );

        return position;
    }
}

pub fn update_position_modify(
    transaction: &Map<String, Value>,
    mut prev_position: PerpPosition,
    is_a: bool,
    funding_rates: &HashMap<u32, Vec<i64>>,
    funding_prices: &HashMap<u32, Vec<u64>>,
) -> PerpPosition {
    let (
        _order,
        order_side,
        spent_synthetic,
        spent_collateral,
        synthetic_token,
        current_funding_idx,
        _index,
        fee_taken,
    ) = parse_order_info(transaction, is_a);

    let price: u64 = get_price(synthetic_token, spent_collateral, spent_synthetic);

    let applicable_funding_rates = &funding_rates[&synthetic_token]
        [prev_position.last_funding_idx as usize..current_funding_idx as usize];
    let applicable_funding_prices = &funding_prices[&synthetic_token]
        [prev_position.last_funding_idx as usize..current_funding_idx as usize];

    if prev_position.order_side == order_side {
        // & Increasing the position size
        prev_position.increase_position_size(
            spent_synthetic,
            price,
            fee_taken,
            applicable_funding_rates.to_vec(),
            applicable_funding_prices.to_vec(),
            current_funding_idx,
        );
    } else {
        if spent_synthetic
            >= prev_position.position_size + DUST_AMOUNT_PER_ASSET[&synthetic_token.to_string()]
        {
            // & Flipping the position side
            prev_position.flip_position_side(
                spent_synthetic,
                price,
                fee_taken,
                applicable_funding_rates.to_vec(),
                applicable_funding_prices.to_vec(),
                current_funding_idx,
            );
        } else {
            // & Decreasing the position size
            prev_position.reduce_position_size(
                spent_synthetic,
                price,
                fee_taken,
                applicable_funding_rates.to_vec(),
                applicable_funding_prices.to_vec(),
                current_funding_idx,
            );
        }
    }

    return prev_position;
}

pub fn update_position_close(
    transaction: &Map<String, Value>,
    mut prev_position: PerpPosition,
    is_a: bool,
    funding_rates: &HashMap<u32, Vec<i64>>,
    funding_prices: &HashMap<u32, Vec<u64>>,
) -> (u64, Option<PerpPosition>) {
    let (
        _order,
        _order_side,
        spent_synthetic,
        spent_collateral,
        synthetic_token,
        current_funding_idx,
        _index,
        fee_taken,
    ) = parse_order_info(transaction, is_a);

    let close_price: u64 = get_price(synthetic_token, spent_collateral, spent_synthetic);

    let is_full_close = prev_position.position_size - spent_synthetic
        <= DUST_AMOUNT_PER_ASSET[&synthetic_token.to_string()];

    let applicable_funding_rates = &funding_rates[&synthetic_token]
        [prev_position.last_funding_idx as usize..current_funding_idx as usize];
    let applicable_funding_prices = &funding_prices[&synthetic_token]
        [prev_position.last_funding_idx as usize..current_funding_idx as usize];

    let collateral_returned;
    if is_full_close {
        // ! close position fully
        collateral_returned = prev_position
            .close_position(
                close_price,
                fee_taken,
                applicable_funding_rates.to_vec(),
                applicable_funding_prices.to_vec(),
                current_funding_idx,
            )
            .unwrap();
    } else {
        // ! close position partially
        collateral_returned = prev_position
            .close_position_partialy(
                spent_synthetic,
                close_price,
                fee_taken,
                applicable_funding_rates.to_vec(),
                applicable_funding_prices.to_vec(),
                current_funding_idx,
            )
            .unwrap();
    }

    let updated_position = if is_full_close {
        None
    } else {
        Some(prev_position)
    };

    return (collateral_returned, updated_position);
}

// * Liquiditations * //

pub fn update_liquidated_position(
    transaction: &Map<String, Value>,
    mut liquidated_position: PerpPosition,
    funding_rates: &HashMap<u32, Vec<i64>>,
    funding_prices: &HashMap<u32, Vec<u64>>,
) -> (u64, u64, Option<PerpPosition>) {
    let (_, _, _, _, synthetic_token, current_funding_idx, _) =
        parse_liquidation_order_info(transaction);

    let market_price = transaction.get("market_price").unwrap().as_u64().unwrap();
    let index_price = transaction.get("index_price").unwrap().as_u64().unwrap();

    let applicable_funding_rates = &funding_rates[&synthetic_token]
        [liquidated_position.last_funding_idx as usize..current_funding_idx as usize];
    let applicable_funding_prices = &funding_prices[&synthetic_token]
        [liquidated_position.last_funding_idx as usize..current_funding_idx as usize];

    let (liquidated_size, liquidator_fee, _, is_partial_liquidation) = liquidated_position
        .liquidate_position(
            market_price,
            index_price,
            applicable_funding_rates.to_vec(),
            applicable_funding_prices.to_vec(),
            current_funding_idx,
        )
        .unwrap();

    if is_partial_liquidation {
        return (liquidated_size, liquidator_fee, Some(liquidated_position));
    } else {
        return (liquidated_size, liquidator_fee, None);
    }
}

pub fn open_pos_after_liquidations(
    transaction: &Map<String, Value>,
    liquidated_size: u64,
    liquidator_fee: u64,
) -> PerpPosition {
    let (order, order_side, _, _, synthetic_token, current_funding_idx, index) =
        parse_liquidation_order_info(transaction);

    let market_price = transaction.get("market_price").unwrap().as_u64().unwrap();

    let open_order_fields = order.get("open_order_fields").unwrap();
    let initial_margin = open_order_fields.get("initial_margin").unwrap();
    let init_margin = initial_margin.as_u64().unwrap() + liquidator_fee;

    let multiplier: u128 = 10_u128.pow(
        (DECIMALS_PER_ASSET[&synthetic_token.to_string()]
            + PRICE_DECIMALS_PER_ASSET[&synthetic_token.to_string()]
            - COLLATERAL_TOKEN_DECIMALS) as u32,
    );
    let scaler = 10_u128.pow(LEVERAGE_DECIMALS as u32);

    let leverage = (liquidated_size as u128 * market_price as u128 * scaler
        / (init_margin as u128 * multiplier)) as u64;

    let allow_partial_liquidations = open_order_fields
        .get("allow_partial_liquidations")
        .unwrap()
        .as_bool()
        .unwrap();
    let position_address = open_order_fields
        .get("position_address")
        .unwrap()
        .as_str()
        .unwrap();
    let position_address = BigUint::from_str(position_address).unwrap();

    let position = PerpPosition::new(
        order_side,
        liquidated_size,
        synthetic_token,
        COLLATERAL_TOKEN,
        init_margin,
        leverage,
        allow_partial_liquidations,
        position_address,
        current_funding_idx,
        index,
        0,
    );

    return position;
}

//  ** ============================================================================================================
pub fn refund_partial_fill(transaction: &Map<String, Value>, is_a: bool) -> Option<Note> {
    let order = transaction
        .get(if is_a { "order_a" } else { "order_b" })
        .unwrap();

    let prev_pfr_note = transaction
        .get(if is_a {
            "prev_pfr_note_a"
        } else {
            "prev_pfr_note_b"
        })
        .unwrap();

    let spent_synthetic = transaction
        .get("swap_data")
        .unwrap()
        .get("spent_synthetic")
        .unwrap()
        .as_u64()
        .unwrap();

    let (initial_margin, init_margin) = get_init_margin(order, spent_synthetic);

    let unspent_margin = if prev_pfr_note.is_null() {
        initial_margin - init_margin as u64
    } else {
        prev_pfr_note.get("amount").unwrap().as_u64().unwrap() - init_margin as u64
    };

    let synthetic_token = order.get("synthetic_token").unwrap().as_u64().unwrap() as u32;
    if unspent_margin <= DUST_AMOUNT_PER_ASSET[&synthetic_token.to_string()] {
        return None;
    };

    let indexes = transaction
        .get("indexes")
        .unwrap()
        .get(if is_a { "order_a" } else { "order_b" })
        .unwrap();
    let pfr_index = indexes.get("new_pfr_idx").unwrap().as_u64().unwrap();

    let address;
    let blinding;
    if !prev_pfr_note.is_null() {
        let prev_pfr_address = prev_pfr_note.get("address").unwrap();

        address = EcPoint::new(
            &BigUint::from_str(prev_pfr_address.get("x").unwrap().as_str().unwrap()).unwrap(),
            &BigUint::from_str(prev_pfr_address.get("y").unwrap().as_str().unwrap()).unwrap(),
        );

        blinding =
            BigUint::from_str(prev_pfr_note.get("blinding").unwrap().as_str().unwrap()).unwrap();
    } else {
        let open_order_fields = order.get("open_order_fields").unwrap();
        let note0 = &open_order_fields
            .get("notes_in")
            .unwrap()
            .as_array()
            .unwrap()[0];
        let note0_address = note0.get("address").unwrap();

        address = EcPoint::new(
            &BigUint::from_str(note0_address.get("x").unwrap().as_str().unwrap()).unwrap(),
            &BigUint::from_str(note0_address.get("y").unwrap().as_str().unwrap()).unwrap(),
        );

        blinding = BigUint::from_str(note0.get("blinding").unwrap().as_str().unwrap()).unwrap()
    }

    return Some(Note::new(
        pfr_index,
        address,
        COLLATERAL_TOKEN,
        unspent_margin,
        blinding,
    ));
}

pub fn return_collateral_on_close(
    transaction: &Map<String, Value>,
    is_a: bool,
    return_collateral_amount: u64,
) -> Note {
    let order = transaction
        .get(if is_a { "order_a" } else { "order_b" })
        .unwrap();

    let indexes = transaction
        .get("indexes")
        .unwrap()
        .get(if is_a { "order_a" } else { "order_b" })
        .unwrap();
    let index = indexes
        .get("return_collateral_idx")
        .unwrap()
        .as_u64()
        .unwrap();

    let close_order_fields = order.get("close_order_fields").unwrap();

    let dest_received_address = close_order_fields.get("dest_received_address").unwrap();
    let address = EcPoint::new(
        &BigUint::from_str(dest_received_address.get("x").unwrap().as_str().unwrap()).unwrap(),
        &BigUint::from_str(dest_received_address.get("y").unwrap().as_str().unwrap()).unwrap(),
    );

    let dest_received_blinding = close_order_fields.get("dest_received_blinding").unwrap();
    let blinding = BigUint::from_str(dest_received_blinding.as_str().unwrap()).unwrap();

    return Note::new(
        index,
        address,
        COLLATERAL_TOKEN,
        return_collateral_amount,
        blinding,
    );
}

// * =============================================================================================================

// * UTILS * //

pub fn position_from_json(position_json: &Value) -> PerpPosition {
    let pos_header = position_json.get("position_header").unwrap();

    let position_header = PositionHeader::new(
        pos_header.get("synthetic_token").unwrap().as_u64().unwrap() as u32,
        pos_header
            .get("allow_partial_liquidations")
            .unwrap()
            .as_bool()
            .unwrap(),
        BigUint::from_str(
            pos_header
                .get("position_address")
                .unwrap()
                .as_str()
                .unwrap(),
        )
        .unwrap(),
        pos_header.get("vlp_token").unwrap().as_u64().unwrap() as u32,
    );

    let order_side = if position_json.get("order_side").unwrap().as_str().unwrap() == "Long" {
        OrderSide::Long
    } else {
        OrderSide::Short
    };
    let position_size = position_json
        .get("position_size")
        .unwrap()
        .as_u64()
        .unwrap();
    let entry_price = position_json.get("entry_price").unwrap().as_u64().unwrap();
    let liquidation_price = position_json
        .get("liquidation_price")
        .unwrap()
        .as_u64()
        .unwrap();
    let last_funding_idx = position_json
        .get("last_funding_idx")
        .unwrap()
        .as_u64()
        .unwrap() as u32;
    let vlp_supply = position_json.get("vlp_supply").unwrap().as_u64().unwrap();
    let margin = position_json.get("margin").unwrap().as_u64().unwrap();
    let index = position_json.get("index").unwrap().as_u64().unwrap() as u32;
    let bankruptcy_price = position_json
        .get("bankruptcy_price")
        .unwrap()
        .as_u64()
        .unwrap();

    let position_hash = _hash_position(
        &position_header.hash,
        &order_side,
        position_size,
        entry_price,
        liquidation_price,
        last_funding_idx,
        vlp_supply,
    );

    let position = PerpPosition {
        position_header,
        order_side,
        position_size,
        margin,
        entry_price,
        liquidation_price,
        bankruptcy_price,
        last_funding_idx,
        index,
        vlp_supply,
        hash: position_hash,
    };

    return position;
}

pub fn get_init_margin(order_json: &Value, spent_synthetic: u64) -> (u64, u64) {
    let open_order_fields = order_json.get("open_order_fields").unwrap();

    let initial_margin = open_order_fields
        .get("initial_margin")
        .unwrap()
        .as_u64()
        .unwrap();

    let order_amount = order_json
        .get("synthetic_amount")
        .unwrap()
        .as_u64()
        .unwrap();

    let init_margin = (initial_margin as u128 * spent_synthetic as u128) / order_amount as u128;

    return (initial_margin, init_margin as u64);
}

pub fn parse_order_info(
    transaction: &Map<String, Value>,
    is_a: bool,
) -> (&serde_json::Value, OrderSide, u64, u64, u32, u32, u32, u64) {
    let order = transaction
        .get(if is_a { "order_a" } else { "order_b" })
        .unwrap();

    let order_side = if order.get("order_side").unwrap().as_str().unwrap() == "Long" {
        OrderSide::Long
    } else {
        OrderSide::Short
    };

    let swap_data = transaction.get("swap_data").unwrap();
    let spent_synthetic = swap_data.get("spent_synthetic").unwrap().as_u64().unwrap();
    let spent_collateral = swap_data.get("spent_collateral").unwrap().as_u64().unwrap();

    let synthetic_token = order.get("synthetic_token").unwrap().as_u64().unwrap() as u32;

    let indexes = transaction
        .get("indexes")
        .unwrap()
        .get(if is_a { "order_a" } else { "order_b" })
        .unwrap();
    let current_funding_idx = indexes.get("new_funding_idx").unwrap().as_u64().unwrap() as u32;
    let index = indexes.get("position_idx").unwrap().as_u64().unwrap() as u32;

    let fee_taken = swap_data
        .get(if is_a { "fee_taken_a" } else { "fee_taken_b" })
        .unwrap()
        .as_u64()
        .unwrap();

    return (
        order,
        order_side,
        spent_synthetic,
        spent_collateral,
        synthetic_token,
        current_funding_idx,
        index,
        fee_taken,
    );
}

pub fn parse_liquidation_order_info(
    transaction: &Map<String, Value>,
) -> (&serde_json::Value, OrderSide, u64, u64, u32, u32, u32) {
    let order = transaction.get("liquidation_order").unwrap();

    let order_side = if order.get("order_side").unwrap().as_str().unwrap() == "Long" {
        OrderSide::Long
    } else {
        OrderSide::Short
    };

    let synthetic_amount = order.get("synthetic_amount").unwrap().as_u64().unwrap();
    let collateral_amount = order.get("collateral_amount").unwrap().as_u64().unwrap();

    let synthetic_token = order.get("synthetic_token").unwrap().as_u64().unwrap() as u32;

    let indexes = transaction.get("indexes").unwrap();
    let current_funding_idx = indexes.get("new_funding_idx").unwrap().as_u64().unwrap() as u32;
    let index = indexes.get("new_position_index").unwrap().as_u64().unwrap() as u32;

    return (
        order,
        order_side,
        synthetic_amount,
        collateral_amount,
        synthetic_token,
        current_funding_idx,
        index,
    );
}
