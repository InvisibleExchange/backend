// * ==============================================================================
// * OPEN ORDER TRANSACTION

use error_stack::Result;
use parking_lot::Mutex;
use std::{collections::HashMap, sync::Arc};

use crate::{
    perpetual::{
        perp_helpers::perp_swap_helpers::{
            _check_note_sums, _check_prev_fill_consistencies, get_max_leverage, refund_partial_fill,
        },
        perp_order::PerpOrder,
        perp_position::PerpPosition,
        COLLATERAL_TOKEN, DUST_AMOUNT_PER_ASSET, LEVERAGE_DECIMALS,
    },
    trees::superficial_tree::SuperficialTree,
    utils::{
        errors::{send_perp_swap_error, PerpSwapExecutionError},
        notes::Note,
    },
};

pub fn execute_open_order(
    state_tree_m: &Arc<Mutex<SuperficialTree>>,
    partialy_filled_positions_m: &Arc<Mutex<HashMap<String, (PerpPosition, u64)>>>,
    order: &PerpOrder,
    fee_taken: u64,
    perp_state_zero_index: u64,
    funding_idx: u32,
    spent_synthetic: u64,
    spent_collateral: u64,
    init_margin: u64,
    partial_fill_info: Option<(Option<Note>, u64, u64)>,
) -> Result<
    (
        Option<PerpPosition>,
        PerpPosition,
        Option<Note>,
        (Option<Note>, u64, u64),
        u64,
        u32,
        bool,
    ),
    PerpSwapExecutionError,
> {
    let is_first_fill = partial_fill_info.is_none();

    // ? Get the new total amount filled after this swap
    let new_amount_filled = if partial_fill_info.is_some() {
        partial_fill_info.as_ref().unwrap().1 + spent_synthetic
    } else {
        spent_synthetic
    };

    let open_order_fields = order.open_order_fields.as_ref().unwrap();

    // ? Check if the note sums are sufficient for amount spent
    let prev_pfr_note: Option<Note>;
    if is_first_fill {
        _check_note_sums(open_order_fields, order.order_id)?;

        if open_order_fields.refund_note.is_some() {
            if open_order_fields.notes_in[0].index
                != open_order_fields.refund_note.as_ref().unwrap().index
            {
                return Err(send_perp_swap_error(
                    "refund note index is not the same as the first note index".to_string(),
                    Some(order.order_id),
                    None,
                ));
            }
        }

        prev_pfr_note = None;
    } else {
        let pfr_note =
            _check_prev_fill_consistencies(&state_tree_m, &partial_fill_info, order, init_margin)?;
        prev_pfr_note = Some(pfr_note);
    }

    // ? ---------------------------------------------------------------------------------
    // ? Refund unspent margin

    let unspent_margin = if is_first_fill {
        open_order_fields.initial_margin - init_margin
    } else {
        open_order_fields.initial_margin - partial_fill_info.as_ref().unwrap().2 - init_margin
    };

    let new_spent_margin = if is_first_fill {
        init_margin
    } else {
        partial_fill_info.as_ref().unwrap().2 + init_margin
    };

    // let mut new_pfr_idx: u64 = 0;
    let new_partial_refund_note: Option<Note>;

    let mut is_partially_filled =
        unspent_margin > DUST_AMOUNT_PER_ASSET[&order.synthetic_token.to_string()];

    if is_partially_filled {
        if new_amount_filled
            >= order.synthetic_amount - DUST_AMOUNT_PER_ASSET[&order.synthetic_token.to_string()]
        {
            // TODO: Order was filled but there is some excess margin (Insurance fund?)
            is_partially_filled = false;
        }

        // ! Order was partially filled
        let mut state_tree = state_tree_m.lock();
        let new_pfr_idx = state_tree.first_zero_idx();
        drop(state_tree);

        new_partial_refund_note = refund_partial_fill(
            open_order_fields.collateral_token,
            &open_order_fields.notes_in[0].blinding,
            open_order_fields.notes_in[0].address.clone(),
            unspent_margin,
            new_pfr_idx,
        );
    } else {
        new_partial_refund_note = None;
    }

    let new_partial_fill_info = if is_partially_filled {
        (new_partial_refund_note, new_amount_filled, new_spent_margin)
    } else {
        (None, new_amount_filled, 0)
    };

    // ? ---------------------------------------------------------------------------------
    let prev_position: Option<PerpPosition>;
    let position: PerpPosition;
    let prev_funding_idx: u32;
    let new_spent_sythetic: u64;

    if is_first_fill {
        //
        position = open_new_position(
            order,
            init_margin,
            fee_taken,
            perp_state_zero_index,
            funding_idx,
            spent_synthetic,
            spent_collateral,
        )?;

        prev_position = None;
        prev_funding_idx = funding_idx;

        new_spent_sythetic = spent_synthetic;

        //
    } else {
        //
        let (prev_position_, position_, new_spent_sythetic_) = add_margin_to_position(
            partialy_filled_positions_m,
            order,
            init_margin,
            fee_taken,
            spent_synthetic,
            spent_collateral,
        )?;
        new_spent_sythetic = new_spent_sythetic_;

        //finalize_updates
        prev_funding_idx = prev_position_.last_funding_idx;
        position = position_;
        prev_position = Some(prev_position_);
    }

    return Ok((
        prev_position,
        position,
        prev_pfr_note,
        new_partial_fill_info,
        new_spent_sythetic,
        prev_funding_idx,
        !is_partially_filled,
    ));
}

// * ======================================================================================================
// * ======================================================================================================

fn open_new_position(
    order: &PerpOrder,
    init_margin: u64,
    fee_taken: u64,
    zero_idx: u64,
    funding_idx: u32,
    spent_synthetic: u64,
    spent_collateral: u64,
) -> Result<PerpPosition, PerpSwapExecutionError> {
    let position: PerpPosition;

    let leverage = (spent_collateral as u128 * 10_u128.pow(LEVERAGE_DECIMALS as u32)
        / (init_margin - fee_taken) as u128) as u64;

    // ? Check that leverage is valid relative to the notional position size
    let max_leverage = get_max_leverage(order.synthetic_token, spent_synthetic);
    if max_leverage * 103 / 100 < leverage {
        return Err(send_perp_swap_error(
            "Leverage is too high".to_string(),
            Some(order.order_id),
            Some(format!(
                "Max leverage for {} with size {} is {}",
                order.synthetic_token, spent_synthetic, max_leverage
            )),
        ));
    }

    position = PerpPosition::new(
        order.order_side.clone(),
        spent_synthetic,
        order.synthetic_token,
        order.open_order_fields.as_ref().unwrap().collateral_token,
        init_margin,
        leverage,
        order
            .open_order_fields
            .as_ref()
            .unwrap()
            .allow_partial_liquidations,
        order
            .open_order_fields
            .as_ref()
            .unwrap()
            .position_address
            .clone(),
        funding_idx,
        zero_idx as u32,
        fee_taken,
    );

    return Ok(position);
}

fn add_margin_to_position(
    partialy_filled_positions_m: &Arc<Mutex<HashMap<String, (PerpPosition, u64)>>>,
    order: &PerpOrder,
    init_margin: u64,
    fee_taken: u64,
    spent_synthetic: u64,
    spent_collateral: u64,
) -> Result<(PerpPosition, PerpPosition, u64), PerpSwapExecutionError> {
    let mut position: PerpPosition;
    let prev_spent_synthetic: u64;

    let addr_string = order
        .open_order_fields
        .as_ref()
        .unwrap()
        .position_address
        .to_string();

    let mut partialy_filled_positions = partialy_filled_positions_m.lock();
    let position__ = partialy_filled_positions.remove(&addr_string);
    drop(partialy_filled_positions);

    if let Some(pos) = position__ {
        position = pos.0;
        prev_spent_synthetic = pos.1;
    } else {
        return Err(send_perp_swap_error(
            "Position does not exist".to_string(),
            Some(order.order_id),
            None,
        ));
    }

    // ? If order_side == Long and position_modification_type == AddMargin then it should be a Long position
    if position.order_side != order.order_side {
        return Err(send_perp_swap_error(
            "position should have same order_side as order when position_modification_type == Open"
                .to_string(),
            Some(order.order_id),
            None,
        ));
    }

    let prev_position: PerpPosition = position.clone();
    if prev_position.position_header.synthetic_token != order.synthetic_token {
        return Err(send_perp_swap_error(
            "Position and order should have same synthetic token".to_string(),
            Some(order.order_id),
            None,
        ));
    }

    let leverage = (spent_collateral as u128 * 10_u128.pow(LEVERAGE_DECIMALS as u32)
        / init_margin as u128) as u64;

    position.add_margin_to_position(init_margin, spent_synthetic, leverage, fee_taken);

    // ? Check that leverage is valid relative to the notional position size
    if get_max_leverage(order.synthetic_token, position.position_size) * 103 / 100 < leverage {
        return Err(send_perp_swap_error(
            "Leverage is too high".to_string(),
            Some(order.order_id),
            Some(format!(
                "Max leverage for {} with size {} is {}",
                order.synthetic_token,
                order.synthetic_amount,
                get_max_leverage(order.synthetic_token, order.synthetic_amount)
            )),
        ));
    }

    let new_spent_sythetic = prev_spent_synthetic + spent_synthetic;

    return Ok((prev_position, position, new_spent_sythetic));
}

// * ======================================================================================================

pub fn check_valid_collateral_token(order: &PerpOrder) -> Result<(), PerpSwapExecutionError> {
    // ? Collateral token is invalid
    if COLLATERAL_TOKEN != order.open_order_fields.as_ref().unwrap().collateral_token {
        return Err(send_perp_swap_error(
            "collateral token not valid".to_string(),
            Some(order.order_id),
            None,
        ));
    }

    return Ok(());
}

pub fn get_init_margin(order: &PerpOrder, spent_synthetic: u64) -> u64 {
    let margin = (order.open_order_fields.as_ref().unwrap().initial_margin as u128
        * spent_synthetic as u128)
        / order.synthetic_amount as u128;

    return margin as u64;
}
