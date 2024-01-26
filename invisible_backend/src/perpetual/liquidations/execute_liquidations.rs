use std::sync::Arc;

use crate::{
    perpetual::{
        get_price, perp_helpers::perp_swap_helpers::get_max_leverage, perp_position::PerpPosition,
        OrderSide, COLLATERAL_TOKEN_DECIMALS, DECIMALS_PER_ASSET, LEVERAGE_DECIMALS,
        PRICE_DECIMALS_PER_ASSET, SYNTHETIC_ASSETS,
    },
    transaction_batch::tx_batch_structs::SwapFundingInfo,
    trees::superficial_tree::SuperficialTree,
    utils::errors::{send_perp_swap_error, PerpSwapExecutionError},
};

use super::liquidation_order::LiquidationOrder;
use error_stack::Result;
use parking_lot::Mutex;

pub fn execute_liquidation(
    market_price: u64,
    index_price: u64,
    swap_funding_info: &SwapFundingInfo,
    liquidation_order: &LiquidationOrder,
    liquidated_position: &mut PerpPosition,
) -> Result<(u64, u64, i64, bool), PerpSwapExecutionError> {
    //

    liquidation_consistency_checks(liquidation_order, market_price)?;

    let idx_diff = liquidated_position.last_funding_idx - swap_funding_info.min_swap_funding_idx;

    let applicable_funding_rates = &swap_funding_info.swap_funding_rates[idx_diff as usize..];
    let applicable_funding_prices = &swap_funding_info.swap_funding_prices[idx_diff as usize..];

    is_position_liquidatable(liquidation_order, market_price, index_price)?;

    let (liquidated_size, liquidator_fee, leftover_collateral, is_partial_liquidation) =
        liquidated_position.liquidate_position(
            market_price,
            index_price,
            applicable_funding_rates.to_vec(),
            applicable_funding_prices.to_vec(),
            swap_funding_info.current_funding_idx,
        )?;

    Ok((
        liquidated_size,
        liquidator_fee,
        leftover_collateral,
        is_partial_liquidation,
    ))
}

pub fn open_new_position_after_liquidation(
    liquidation_order: &LiquidationOrder,
    liquidated_size: u64,
    liquidator_fee: u64,
    market_price: u64,
    current_funding_index: u32,
    new_idx: u64,
) -> Result<PerpPosition, PerpSwapExecutionError> {
    //

    let init_margin = liquidation_order.open_order_fields.initial_margin + liquidator_fee;

    let synthetic_token = liquidation_order.synthetic_token;

    let multiplier: u128 = 10_u128.pow(
        (DECIMALS_PER_ASSET[&synthetic_token.to_string()]
            + PRICE_DECIMALS_PER_ASSET[&synthetic_token.to_string()]
            - COLLATERAL_TOKEN_DECIMALS) as u32,
    );
    let scaler = 10_u128.pow(LEVERAGE_DECIMALS as u32);

    let leverage = (liquidated_size as u128 * market_price as u128 * scaler
        / (init_margin as u128 * multiplier)) as u64;

    // ? Check that leverage is valid relative to the notional position size
    let max_leverage = get_max_leverage(liquidation_order.synthetic_token, liquidated_size);
    if max_leverage < leverage {
        return Err(send_perp_swap_error(
            "Leverage is too high".to_string(),
            None,
            None,
        ));
    }

    let position = PerpPosition::new(
        liquidation_order.order_side.clone(),
        liquidated_size,
        liquidation_order.synthetic_token,
        liquidation_order.open_order_fields.collateral_token,
        init_margin,
        leverage,
        liquidation_order
            .open_order_fields
            .allow_partial_liquidations,
        liquidation_order.open_order_fields.position_address.clone(),
        current_funding_index,
        new_idx,
        0,
    );

    return Ok(position);
}

// * HELPERS ======================================================================================

/// ## Checks:
pub fn liquidation_consistency_checks(
    liquidation_order: &LiquidationOrder,
    market_price: u64,
) -> Result<(), PerpSwapExecutionError> {
    // ? Check that synthetic tokens are valid
    if !SYNTHETIC_ASSETS.contains(&liquidation_order.synthetic_token) {
        return Err(send_perp_swap_error(
            "synthetic token not valid".to_string(),
            None,
            Some(format!(
                "invalid synthetic token {:?}",
                liquidation_order.synthetic_token
            )),
        ));
    }

    if liquidation_order.position.position_header.synthetic_token
        != liquidation_order.synthetic_token
    {
        return Err(send_perp_swap_error(
            "order and position token mismatch".to_string(),
            None,
            None,
        ));
    }

    // ? Check that the orders are the opposite sides
    // ? for simplicity, we require order_a to be the "buyer" and order_b to be the "seller"
    if liquidation_order.position.order_side != liquidation_order.order_side {
        return Err(send_perp_swap_error(
            "order and position order side mismatch".to_string(),
            None,
            None,
        ));
    }

    let price = get_price(
        liquidation_order.synthetic_token,
        liquidation_order.collateral_amount,
        liquidation_order.synthetic_amount,
    );

    // ? Check that the market price is at least as good as the order price
    if liquidation_order.order_side == OrderSide::Long {
        if market_price > price {
            return Err(send_perp_swap_error(
                "invalid market price".to_string(),
                None,
                None,
            ));
        }
    } else {
        if market_price < price {
            return Err(send_perp_swap_error(
                "invalid market price".to_string(),
                None,
                None,
            ));
        }
    }

    // ? Check that the notes spent are all different for both orders (different indexes)
    let mut valid = true;

    let mut spent_indexes_a: Vec<u64> = Vec::new();
    let mut note_in_sum: u64 = 0;

    liquidation_order
        .open_order_fields
        .notes_in
        .iter()
        .for_each(|note| {
            if spent_indexes_a.contains(&note.index) {
                valid = false;
            }
            spent_indexes_a.push(note.index);
            note_in_sum += note.amount;
        });

    if !valid {
        return Err(send_perp_swap_error(
            "Notes spent are not unique".to_string(),
            None,
            None,
        ));
    }

    let refund_amount = if liquidation_order.open_order_fields.refund_note.is_some() {
        liquidation_order
            .open_order_fields
            .refund_note
            .as_ref()
            .unwrap()
            .amount
    } else {
        0
    };
    if note_in_sum - refund_amount != liquidation_order.open_order_fields.initial_margin {
        return Err(send_perp_swap_error(
            "Notes spent do not match expected margin amount".to_string(),
            None,
            None,
        ));
    }

    Ok(())
}

pub fn is_position_liquidatable(
    liquidation_order: &LiquidationOrder,
    market_price: u64,
    index_price: u64,
) -> Result<(), PerpSwapExecutionError> {
    // Get liquidatable amount
    let (is_liquidatable, liquidatable_amount) = liquidation_order
        .position
        .is_position_liquidatable(market_price, index_price);

    if !is_liquidatable {
        return Err(send_perp_swap_error(
            "position is not liquidatable".to_string(),
            None,
            None,
        ));
    }

    if liquidation_order.synthetic_amount < liquidatable_amount {
        return Err(send_perp_swap_error(
            "overspending in liquidation".to_string(),
            None,
            None,
        ));
    }

    Ok(())
}

pub fn verify_position_existence(
    perpetual_state_tree: &Arc<Mutex<SuperficialTree>>,
    position: &PerpPosition,
) -> Result<(), PerpSwapExecutionError> {
    let perp_state_tree = perpetual_state_tree.lock();

    // ? Verify the position hash is valid and exists in the state
    if position.hash != position.hash_position() {
        return Err(send_perp_swap_error(
            "position hash not valid".to_string(),
            None,
            None,
        ));
    }

    // ? Check that the position being updated exists in the state
    if perp_state_tree.get_leaf_by_index(position.index as u64) != position.hash {
        return Err(send_perp_swap_error(
            "position does not exist in the state".to_string(),
            None,
            None,
        ));
    }

    Ok(())
}
