use std::{collections::HashMap, sync::Arc};

use error_stack::Result;
use num_bigint::BigUint;
use parking_lot::{Mutex, MutexGuard};

use crate::{
    order_tab::OrderTab,
    perpetual::DUST_AMOUNT_PER_ASSET,
    transaction_batch::{LeafNodeType, StateUpdate, TxOutputJson},
    transactions::limit_order::LimitOrder,
    trees::superficial_tree::SuperficialTree,
    utils::errors::{send_swap_error, SwapThreadExecutionError},
};

// * CHECK ORDER VALIDITY FUNCTION * --------------------------------------------
pub fn check_tab_order_validity(
    state_tree: &Arc<Mutex<SuperficialTree>>,
    order: &LimitOrder,
    order_tab: &Option<OrderTab>,
    spent_amount: u64,
) -> Result<(), SwapThreadExecutionError> {
    // ? Check that the order tab is valid --------------------------------------------
    if order_tab.is_none() {
        return Err(send_swap_error(
            "order_tab is not defined".to_string(),
            Some(order.order_id),
            None,
        ));
    }

    let order_tab = order_tab.as_ref().unwrap();

    // ? Check that the tokens are valid --------------------------------------------
    if (order_tab.tab_header.base_token != order.token_spent
        || order_tab.tab_header.quote_token != order.token_received)
        && (order_tab.tab_header.base_token != order.token_received
            || order_tab.tab_header.quote_token != order.token_spent)
    {
        return Err(send_swap_error(
            "tokens swapped are invalid".to_string(),
            Some(order.order_id),
            None,
        ));
    }

    let is_buy = order_tab.tab_header.base_token == order.token_received;

    // ? Check that the order tab has sufficiennt balance to cover the order ---------------
    if is_buy {
        if spent_amount > order_tab.quote_amount {
            return Err(send_swap_error(
                "order_tab does not have enough funds for this the order".to_string(),
                Some(order.order_id),
                None,
            ));
        }
    } else {
        if spent_amount > order_tab.base_amount {
            return Err(send_swap_error(
                "order_tab does not have enough funds for this the order".to_string(),
                Some(order.order_id),
                None,
            ));
        }
    }

    // ? Check that the order is not overspending --------------------------------------------
    if spent_amount > order.amount_spent + DUST_AMOUNT_PER_ASSET[&order.token_spent.to_string()] {
        return Err(send_swap_error(
            "order is overspending".to_string(),
            Some(order.order_id),
            None,
        ));
    }

    // 378_357

    // ? Check that the order tab hash exists in the state --------------------------------------------
    let state_tree_ = state_tree.lock();
    let leaf_hash = state_tree_.get_leaf_by_index(order_tab.tab_idx as u64);

    if leaf_hash != order_tab.hash {
        return Err(send_swap_error(
            "order_tab hash does not exist in the state".to_string(),
            Some(order.order_id),
            None,
        ));
    }

    Ok(())
}
//

//

pub fn execute_tab_order_modifications(
    prev_filled_amount: u64,
    order: &LimitOrder,
    mut order_tab: OrderTab,
    spent_amount_x: u64,
    spent_amount_y: u64,
    fee_taken_x: u64,
) -> (bool, OrderTab, u64) {
    let is_buy = order.token_received == order_tab.tab_header.base_token;

    if is_buy {
        order_tab.quote_amount -= spent_amount_x;
        order_tab.base_amount += spent_amount_y - fee_taken_x;
    } else {
        order_tab.base_amount -= spent_amount_x;
        order_tab.quote_amount += spent_amount_y - fee_taken_x;
    }

    order_tab.update_hash();

    let new_amount_filled = prev_filled_amount + spent_amount_y;
    let is_partially_filled = new_amount_filled
        + DUST_AMOUNT_PER_ASSET[&order.token_spent.to_string()]
        < order.amount_received;

    return (is_partially_filled, order_tab, new_amount_filled);
}

//

//

// ? update the state with the new order tab hash
pub fn update_state_after_tab_order(
    state_tree: &Arc<Mutex<SuperficialTree>>,
    updated_state_hashes_m: &Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
    transaction_output_json: &mut MutexGuard<TxOutputJson>,
    updated_order_tab: &OrderTab,
) {
    let mut state_tree_ = state_tree.lock();
    let mut updated_state_hashes = updated_state_hashes_m.lock();

    state_tree_.update_leaf_node(&updated_order_tab.hash, updated_order_tab.tab_idx as u64);
    updated_state_hashes.insert(
        updated_order_tab.tab_idx as u64,
        (LeafNodeType::OrderTab, updated_order_tab.hash.clone()),
    );
    transaction_output_json
        .state_updates
        .push(StateUpdate::OrderTab {
            order_tab: updated_order_tab.clone(),
        });

    drop(state_tree_);
    drop(updated_state_hashes);
}
