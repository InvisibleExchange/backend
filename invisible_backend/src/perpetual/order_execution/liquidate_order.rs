use std::{collections::HashMap, sync::Arc};

use crate::{
    perpetual::{
        get_price, perp_helpers::perp_swap_helpers::block_until_prev_fill_finished,
        perp_order::PerpOrder, perp_position::PerpPosition, DUST_AMOUNT_PER_ASSET,
    },
    transaction_batch::tx_batch_structs::SwapFundingInfo,
    utils::{
        errors::{send_perp_swap_error, PerpSwapExecutionError},
        notes::Note,
    },
};

use error_stack::Result;
use parking_lot::Mutex;

pub fn execute_liquidation(
    swap_funding_info: &SwapFundingInfo,
    partialy_filled_positions_m: &Arc<Mutex<HashMap<String, (PerpPosition, u64)>>>,
    perpetual_partial_fill_tracker_m: &Arc<Mutex<HashMap<u64, (Option<Note>, u64, u64)>>>,
    blocked_perp_order_ids_m: &Arc<Mutex<HashMap<u64, bool>>>,
    index_price: u64,
    order: &PerpOrder,
    spent_collateral: u64,
    spent_synthetic: u64,
) -> Result<
    (
        u32,
        PerpPosition,
        Option<PerpPosition>,
        (Option<Note>, u64, u64),
        i64,
        u64,
        u32,
        bool,
    ),
    PerpSwapExecutionError,
> {
    // ? In case of sequential partial fills block threads updating the same order id untill previous thread is finsihed and fetch the previous partial fill info
    let partial_fill_info = block_until_prev_fill_finished(
        perpetual_partial_fill_tracker_m,
        blocked_perp_order_ids_m,
        order.order_id,
    )?;

    if partial_fill_info.is_some()
        && partial_fill_info.as_ref().unwrap().1 == 69
        && partial_fill_info.as_ref().unwrap().2 == 69
    {
        return Err(send_perp_swap_error(
            "Order rejected".to_string(),
            Some(order.order_id),
            None,
        ));
    }

    let (leftover_value, new_spent_synthetic, position_index, prev_position, position) =
        liquidate_position(
            partialy_filled_positions_m,
            index_price,
            swap_funding_info,
            order,
            spent_collateral,
            spent_synthetic,
        )?;

    let prev_funding_idx = prev_position.last_funding_idx;

    // ? Get the new total amount filled after this swap
    let new_amount_filled = if partial_fill_info.is_some() {
        partial_fill_info.as_ref().unwrap().1 + spent_synthetic
    } else {
        spent_synthetic
    };
    let is_fully_filled = new_amount_filled
        >= order.synthetic_amount - DUST_AMOUNT_PER_ASSET[&order.synthetic_token.to_string()];

    let new_partial_fill_info: (Option<Note>, u64, u64) = (None, new_amount_filled, 0);

    return Ok((
        position_index,
        prev_position,
        position,
        new_partial_fill_info,
        leftover_value,
        new_spent_synthetic,
        prev_funding_idx,
        is_fully_filled,
    ));
}

fn liquidate_position(
    partialy_filled_positions_m: &Arc<Mutex<HashMap<String, (PerpPosition, u64)>>>,
    index_price: u64,
    swap_funding_info: &SwapFundingInfo,
    order: &PerpOrder,
    spent_collateral: u64,
    spent_synthetic: u64,
) -> Result<(i64, u64, u32, PerpPosition, Option<PerpPosition>), PerpSwapExecutionError> {
    let mut position: PerpPosition;
    let mut prev_spent_synthetic: u64 = 0;

    if let Some(pos) = &order.position {
        let mut pf_positions = partialy_filled_positions_m.lock();
        let pf_pos = pf_positions.remove(&pos.position_address.to_string());

        if let Some(position_) = pf_pos {
            position = position_.0;
            prev_spent_synthetic = position_.1;
        } else {
            position = pos.clone();
        }
    } else {
        return Err(send_perp_swap_error(
            "Position not defined in modify order".to_string(),
            Some(order.order_id),
            None,
        ));
    }

    if spent_synthetic > position.position_size {
        return Err(send_perp_swap_error(
            "over spending in position liquidation".to_string(),
            Some(order.order_id),
            None,
        ));
    }

    let prev_position: PerpPosition = position.clone();
    if prev_position.synthetic_token != order.synthetic_token {
        return Err(send_perp_swap_error(
            "Position and order should have same synthetic token".to_string(),
            Some(order.order_id),
            None,
        ));
    }

    // ? If order_side == Long and position_modification_type == Close then it should be a Short position
    if position.order_side == order.order_side {
        return Err(send_perp_swap_error("position should have oposite order_side than order when position_modification_type == Liquidate".to_string(),Some(order.order_id), None));
    }

    let position_index = position.index;

    let close_price: u64 = get_price(order.synthetic_token, spent_collateral, spent_synthetic);

    let new_spent_synthetic = spent_synthetic + prev_spent_synthetic;

    let leftover_value: i64;
    if spent_synthetic
        >= position.position_size - DUST_AMOUNT_PER_ASSET[&order.synthetic_token.to_string()]
    {
        let idx_diff = position.last_funding_idx - swap_funding_info.min_swap_funding_idx;

        let applicable_funding_rates = &swap_funding_info.swap_funding_rates[idx_diff as usize..];
        let applicable_funding_prices = &swap_funding_info.swap_funding_prices[idx_diff as usize..];

        // ? Apply funding to position
        position.apply_funding(
            applicable_funding_rates.to_vec(),
            applicable_funding_prices.to_vec(),
        );

        // ! close position fully
        leftover_value = position.liquidate_position(close_price, index_price)?;

        return Ok((
            leftover_value,
            new_spent_synthetic,
            position_index,
            prev_position,
            None,
        ));
    } else {
        let idx_diff = position.last_funding_idx - swap_funding_info.min_swap_funding_idx;

        let applicable_funding_rates = &swap_funding_info.swap_funding_rates[idx_diff as usize..];
        let applicable_funding_prices = &swap_funding_info.swap_funding_prices[idx_diff as usize..];

        // ? Apply funding to position
        position.apply_funding(
            applicable_funding_rates.to_vec(),
            applicable_funding_prices.to_vec(),
        );

        // ! close position partially
        leftover_value = position.liquidate_position_partialy(
            spent_synthetic,
            close_price,
            index_price,
            swap_funding_info.current_funding_idx,
        )?;

        return Ok((
            leftover_value,
            new_spent_synthetic,
            position_index,
            prev_position,
            Some(position),
        ));
    }
}
