use std::{collections::HashMap, sync::Arc};

use crate::{
    perpetual::{
        get_price, perp_helpers::perp_swap_helpers::get_max_leverage, perp_order::PerpOrder,
        perp_position::PerpPosition, DUST_AMOUNT_PER_ASSET,
    },
    transaction_batch::tx_batch_structs::SwapFundingInfo,
    utils::{
        errors::{send_perp_swap_error, PerpSwapExecutionError},
        notes::Note,
    },
};
use error_stack::Result;
use parking_lot::Mutex;

use crate::utils::crypto_utils::Signature;

pub fn execute_modify_order(
    swap_funding_info: &SwapFundingInfo,
    index_price: u64,
    fee_taken: u64,
    partialy_filled_positions_m: &Arc<Mutex<HashMap<String, (PerpPosition, u64)>>>,
    order: &PerpOrder,
    signature: &Signature,
    spent_collateral: u64,
    spent_synthetic: u64,
    prev_position: &PerpPosition,
    partial_fill_info: Option<(Option<Note>, u64, u64)>,
) -> Result<(PerpPosition, (Option<Note>, u64, u64), u64, u32, bool), PerpSwapExecutionError> {
    // ? Get the new total amount filled after this swap
    let new_amount_filled = if partial_fill_info.is_some() {
        partial_fill_info.as_ref().unwrap().1 + spent_synthetic
    } else {
        spent_synthetic
    };

    let is_fully_filled = new_amount_filled
        >= order.synthetic_amount - DUST_AMOUNT_PER_ASSET[&order.synthetic_token.to_string()];

    let (position, new_spent_synthetic) = modify_position(
        partialy_filled_positions_m,
        index_price,
        swap_funding_info,
        order,
        prev_position,
        signature,
        fee_taken,
        spent_collateral,
        spent_synthetic,
    )?;

    let prev_funding_idx = prev_position.last_funding_idx;

    let new_partial_fill_info: (Option<Note>, u64, u64) = (None, new_amount_filled, 0);

    return Ok((
        position,
        new_partial_fill_info,
        new_spent_synthetic,
        prev_funding_idx,
        is_fully_filled,
    ));
}

// * ======================================================================================================
// * ======================================================================================================

fn modify_position(
    partialy_filled_positions_m: &Arc<Mutex<HashMap<String, (PerpPosition, u64)>>>,
    index_price: u64,
    swap_funding_info: &SwapFundingInfo,
    order: &PerpOrder,
    prev_position: &PerpPosition,
    signature: &Signature,
    fee_taken: u64,
    spent_collateral: u64,
    spent_synthetic: u64,
) -> Result<(PerpPosition, u64), PerpSwapExecutionError> {
    let mut position: PerpPosition = prev_position.clone();
    let mut prev_spent_synthetic: u64 = 0;

    if let Some(pos) = &order.position {
        let mut pf_positions = partialy_filled_positions_m.lock();
        let pf_pos = pf_positions.remove(&pos.position_header.position_address.to_string());

        if let Some(position_) = pf_pos {
            prev_spent_synthetic = position_.1;
        }
    } else {
        return Err(send_perp_swap_error(
            "Position not defined in modify order".to_string(),
            Some(order.order_id),
            None,
        ));
    }

    order.verify_order_signature(signature, Some(&position.position_header.position_address))?;

    // ? Check that order token matches synthetic token
    if prev_position.position_header.synthetic_token != order.synthetic_token {
        return Err(send_perp_swap_error(
            "Position and order should have same synthetic token".to_string(),
            Some(order.order_id),
            None,
        ));
    }

    let price: u64 = get_price(order.synthetic_token, spent_collateral, spent_synthetic);

    if position.order_side == order.order_side {
        let idx_diff = position.last_funding_idx - swap_funding_info.min_swap_funding_idx;

        let applicable_funding_rates = &swap_funding_info.swap_funding_rates[idx_diff as usize..];
        let applicable_funding_prices = &swap_funding_info.swap_funding_prices[idx_diff as usize..];

        // & Increasing the position size
        position.increase_position_size(
            spent_synthetic,
            price,
            fee_taken,
            applicable_funding_rates.to_vec(),
            applicable_funding_prices.to_vec(),
            swap_funding_info.current_funding_idx,
        );

        let leverage = position.get_current_leverage(index_price)?;

        // ? Check that leverage is valid relative to the notional position size after increasing size
        if get_max_leverage(order.synthetic_token, position.position_size) * 103 / 100 < leverage {
            return Err(send_perp_swap_error(
                "Leverage would be too high".to_string(),
                Some(order.order_id),
                None,
            ));
        }
    } else {
        let idx_diff = position.last_funding_idx - swap_funding_info.min_swap_funding_idx;

        let applicable_funding_rates = &swap_funding_info.swap_funding_rates[idx_diff as usize..];
        let applicable_funding_prices = &swap_funding_info.swap_funding_prices[idx_diff as usize..];

        if spent_synthetic
            >= position.position_size + DUST_AMOUNT_PER_ASSET[&order.synthetic_token.to_string()]
        {
            // & Flipping the position side
            position.flip_position_side(
                spent_synthetic,
                price,
                fee_taken,
                applicable_funding_rates.to_vec(),
                applicable_funding_prices.to_vec(),
                swap_funding_info.current_funding_idx,
            );

            let leverage = position.get_current_leverage(index_price)?;

            // ? Check that leverage is valid relative to the notional position size after increasing size
            if get_max_leverage(order.synthetic_token, position.position_size) * 103 / 100
                < leverage
            {
                return Err(send_perp_swap_error(
                    "Leverage would be too high".to_string(),
                    Some(order.order_id),
                    None,
                ));
            }
        } else {
            // & Decreasing the position size
            position.reduce_position_size(
                spent_synthetic,
                price,
                fee_taken,
                applicable_funding_rates.to_vec(),
                applicable_funding_prices.to_vec(),
                swap_funding_info.current_funding_idx,
            );
        }
    }

    let new_spent_synthetic = spent_synthetic + prev_spent_synthetic;

    return Ok((position, new_spent_synthetic));
}
