use std::{collections::HashMap, sync::Arc};

use crate::utils::crypto_utils::Signature;
use crate::{
    perpetual::{
        get_price, perp_order::PerpOrder, perp_position::PerpPosition, DUST_AMOUNT_PER_ASSET,
    },
    transaction_batch::tx_batch_structs::SwapFundingInfo,
    utils::{
        errors::{send_perp_swap_error, PerpSwapExecutionError},
        notes::Note,
    },
};

use error_stack::Result;
use parking_lot::Mutex;

pub fn execute_close_order(
    swap_funding_info: &SwapFundingInfo,
    partialy_filled_positions_m: &Arc<Mutex<HashMap<String, (PerpPosition, u64)>>>,
    order: &PerpOrder,
    signature: &Signature,
    fee_taken: u64,
    spent_collateral: u64,
    spent_synthetic: u64,
    prev_position: &PerpPosition,
    partial_fill_info: Option<(Option<Note>, u64, u64)>,
) -> Result<
    (
        u32,
        Option<PerpPosition>,
        (Option<Note>, u64, u64),
        u64,
        u64,
        u32,
        bool,
    ),
    PerpSwapExecutionError,
> {
    // ? Get the new total amount filled after this swap
    let new_amount_filled = if partial_fill_info.is_some() {
        partial_fill_info.as_ref().unwrap().1 + spent_synthetic
    } else {
        spent_synthetic
    };

    let is_fully_filled = new_amount_filled
        >= order.synthetic_amount - DUST_AMOUNT_PER_ASSET[&order.synthetic_token.to_string()];

    let (collateral_returned, new_spent_synthetic, position_index, position) = close_position(
        partialy_filled_positions_m,
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
        position_index,
        position,
        new_partial_fill_info,
        collateral_returned,
        new_spent_synthetic,
        prev_funding_idx,
        is_fully_filled,
    ));
}

// * ======================================================================================================
// * ======================================================================================================

fn close_position(
    partially_filled_positions_m: &Arc<Mutex<HashMap<String, (PerpPosition, u64)>>>,
    swap_funding_info: &SwapFundingInfo,
    order: &PerpOrder,
    prev_position: &PerpPosition,
    signature: &Signature,
    fee_taken: u64,
    spent_collateral: u64,
    spent_synthetic: u64,
) -> Result<(u64, u64, u32, Option<PerpPosition>), PerpSwapExecutionError> {
    let mut position: PerpPosition = prev_position.clone();
    let mut prev_spent_synthetic: u64 = 0;

    if let Some(pos) = &order.position {
        let mut pf_positions = partially_filled_positions_m.lock();
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

    // ? Verify order signature
    order.verify_order_signature(signature, Some(&position.position_header.position_address))?;

    if spent_synthetic > position.position_size {
        return Err(send_perp_swap_error(
            "over spending in position close".to_string(),
            None,
            Some(format!(
                "spent_synthetic: {}, position_size: {}",
                spent_synthetic, position.position_size
            )),
        ));
    }

    if prev_position.position_header.synthetic_token != order.synthetic_token {
        return Err(send_perp_swap_error(
            "Position and order should have same synthetic token".to_string(),
            Some(order.order_id),
            None,
        ));
    }

    // ? If order_side == Long and position_modification_type == Close then it should be a Short position
    if position.order_side == order.order_side {
        return Err(send_perp_swap_error("position should have opposite order_side than order when position_modification_type == Close".to_string(), 
            Some(order.order_id),
            None,
        ));
    }

    let position_index = position.index;

    let close_price: u64 = get_price(order.synthetic_token, spent_collateral, spent_synthetic);

    let new_spent_synthetic = spent_synthetic + prev_spent_synthetic;

    let is_full_close = position.position_size - spent_synthetic
        <= DUST_AMOUNT_PER_ASSET[&order.synthetic_token.to_string()];

    let collateral_returned: u64;
    if is_full_close {
        let idx_diff = position.last_funding_idx - swap_funding_info.min_swap_funding_idx;

        let applicable_funding_rates = &swap_funding_info.swap_funding_rates[idx_diff as usize..];
        let applicable_funding_prices = &swap_funding_info.swap_funding_prices[idx_diff as usize..];

        // ! close position fully
        collateral_returned = position.close_position(
            close_price,
            fee_taken,
            applicable_funding_rates.to_vec(),
            applicable_funding_prices.to_vec(),
            swap_funding_info.current_funding_idx,
        )?;

        return Ok((
            collateral_returned,
            new_spent_synthetic,
            position_index,
            None,
        ));
    } else {
        let idx_diff = position.last_funding_idx - swap_funding_info.min_swap_funding_idx;

        let applicable_funding_rates = &swap_funding_info.swap_funding_rates[idx_diff as usize..];
        let applicable_funding_prices = &swap_funding_info.swap_funding_prices[idx_diff as usize..];

        // ! close position partially
        collateral_returned = position.close_position_partialy(
            spent_synthetic,
            close_price,
            fee_taken,
            applicable_funding_rates.to_vec(),
            applicable_funding_prices.to_vec(),
            swap_funding_info.current_funding_idx,
        )?;

        return Ok((
            collateral_returned,
            new_spent_synthetic,
            position_index,
            Some(position),
        ));
    }

    // ? In case of partiall fills
}
