use std::fmt::Debug;
use std::time::{Duration, SystemTime};

use crate::matching_engine::get_qty_from_quote;
use crate::matching_engine::orders::amend_inner;
use crate::perpetual::perp_order::PerpOrder;
use crate::perpetual::perp_position::PerpPosition;
use crate::perpetual::{DUST_AMOUNT_PER_ASSET, PRICE_DECIMALS_PER_ASSET};
use crate::server::grpc::engine_proto::{
    PerpOrderRestoreMessageInner, SpotOrderRestoreMessageInner,
};
use crate::transactions::limit_order::LimitOrder;
use crate::utils::crypto_utils::Signature;

use super::domain::{Order, OrderSide, OrderType, OrderWrapper};
use super::order_queues::OrderQueue;
use super::orders::{link_order_tab, OrderRequest};
use super::validation::OrderRequestValidator;
use super::{get_quote_qty, sequence};

const MIN_SEQUENCE_ID: u64 = 1;
const MAX_SEQUENCE_ID: u64 = 100_000;
const MAX_STALLED_INDICES_IN_QUEUE: u64 = 10;
const ORDER_QUEUE_INIT_CAPACITY: usize = 500;

pub type OrderProcessingResult = Vec<Result<Success, Failed>>;

#[derive(Debug)]
pub enum Success {
    Accepted {
        id: u64,
        order_type: OrderType,
        ts: SystemTime,
    },

    Filled {
        order: Order,
        signature: Signature,
        side: OrderSide,
        order_type: OrderType,
        price: f64,
        qty: u64,
        quote_qty: u64,
        partially_filled: bool,
        ts: SystemTime,
        user_id: u64,
    },

    // PartiallyFilled {
    //     order: Order,
    //     side: OrderSide,
    //     order_type: OrderType,
    //     price: f64,
    //     qty: u64,
    //     ts: SystemTime,
    // },
    Amended {
        id: u64,
        new_price: f64,
        ts: SystemTime,
    },
    Cancelled {
        id: u64,
        ts: SystemTime,
    },
}

#[derive(Debug)]
pub enum Failed {
    ValidationFailed(String),
    DuplicateOrderID(u64),
    NoMatch(u64),
    OrderNotFound(u64),
    TooMuchSlippage(u64),
}

pub struct OrderBook {
    pub order_asset: u32,
    pub price_asset: u32,
    pub bid_queue: OrderQueue,
    pub ask_queue: OrderQueue,
    seq: sequence::TradeSequence,
    order_validator: OrderRequestValidator,
    pub market_id: u16, // This is used to prepend the order id with a unique number for each orderbook
}

impl OrderBook {
    pub fn new(order_asset: u32, price_asset: u32, market_id: u16) -> Self {
        OrderBook {
            order_asset,
            price_asset,
            bid_queue: OrderQueue::new(
                OrderSide::Bid,
                MAX_STALLED_INDICES_IN_QUEUE,
                ORDER_QUEUE_INIT_CAPACITY,
            ),
            ask_queue: OrderQueue::new(
                OrderSide::Ask,
                MAX_STALLED_INDICES_IN_QUEUE,
                ORDER_QUEUE_INIT_CAPACITY,
            ),
            seq: sequence::new_sequence_gen(MIN_SEQUENCE_ID, MAX_SEQUENCE_ID),
            order_validator: OrderRequestValidator::new(
                order_asset,
                price_asset,
                MIN_SEQUENCE_ID,
                MAX_SEQUENCE_ID,
            ),
            market_id,
        }
    }

    pub fn process_order(&mut self, order: OrderRequest) -> OrderProcessingResult {
        // processing result accumulator
        let mut proc_result: OrderProcessingResult = vec![];

        // validate request
        if let Err(reason) = self.order_validator.validate(&order) {
            proc_result.push(Err(Failed::ValidationFailed(String::from(reason))));
            return proc_result;
        }

        match order {
            OrderRequest::NewLimitOrder {
                order_asset,
                price_asset,
                side,
                price,
                qty,
                quote_qty,
                mut order,
                ts,
                is_market,
            } => {
                let seq_id = self.seq.next_id();

                let order_id = (seq_id as u64) * 2_u64.pow(16) + self.market_id as u64;

                proc_result.push(Ok(Success::Accepted {
                    id: order_id,
                    order_type: OrderType::Limit,
                    ts: SystemTime::now(),
                }));

                // ? If the order tab already exists as part of a different order link this order to that Mutex
                if let Order::Spot(limit_order) = &mut order.order {
                    link_order_tab(limit_order, &self.bid_queue, &self.ask_queue);
                }

                self.process_order_internal(
                    &mut proc_result,
                    order_id,
                    order_asset,
                    price_asset,
                    side,
                    price,
                    qty,
                    quote_qty,
                    order,
                    ts,
                    is_market,
                    false,
                    false,
                );
            }

            OrderRequest::CancelOrder { id, side, user_id } => {
                self.process_order_cancel(&mut proc_result, id, side, user_id);
            }

            OrderRequest::AmendOrder {
                id,
                side,
                new_price,
                new_expiration,
                signature,
                user_id,
                match_only,
            } => {
                self.process_order_amend(
                    &mut proc_result,
                    id,
                    side,
                    new_price,
                    new_expiration,
                    signature,
                    user_id,
                    match_only,
                );
            }
        }

        // return collected processing results
        proc_result
    }

    pub fn retry_order(
        &mut self,
        order: OrderRequest,
        qty: u64,
        order_id: u64,
        failed_order_ids: Option<Vec<u64>>,
    ) -> OrderProcessingResult {
        // processing result accumulator
        let mut proc_result: OrderProcessingResult = vec![];

        match order {
            OrderRequest::NewLimitOrder {
                order_asset,
                price_asset,
                side,
                price,
                qty: _,
                quote_qty,
                order,
                ts,
                is_market,
            } => {
                proc_result.push(Ok(Success::Accepted {
                    id: order_id,
                    order_type: OrderType::Limit,
                    ts,
                }));

                let mut pending_orders: Vec<(OrderWrapper, SystemTime)> = vec![];

                if let Some(failed_order_ids) = failed_order_ids {
                    let opposite_queue = match side {
                        OrderSide::Bid => &mut self.ask_queue,
                        OrderSide::Ask => &mut self.bid_queue,
                    };

                    let mut other_orders = vec![];

                    // loop over all the orders in the opposite_queue and store the orders with failed_order_ids in pending_orders
                    // and the rest in other_orders
                    loop {
                        if let Some((order, ts)) = opposite_queue.pop() {
                            //
                            if failed_order_ids.contains(&order.order_id) {
                                pending_orders.push((order, ts));
                            } else {
                                other_orders.push((order, ts));
                            }
                        }
                        //
                        else {
                            break;
                        }
                    }

                    for (order, ts) in other_orders.into_iter().rev() {
                        opposite_queue.insert(
                            order.order_id,
                            order.order.get_price(order.order_side, None),
                            ts,
                            order,
                        );
                    }
                }

                self.process_order_internal(
                    &mut proc_result,
                    order_id,
                    order_asset,
                    price_asset,
                    side,
                    price,
                    qty,
                    quote_qty,
                    order,
                    ts,
                    is_market,
                    true,
                    false,
                );

                for (order, ts) in pending_orders {
                    let opposite_queue = match side {
                        OrderSide::Bid => &mut self.ask_queue,
                        OrderSide::Ask => &mut self.bid_queue,
                    };

                    opposite_queue.insert(
                        order.order_id,
                        order.order.get_price(order.order_side, None),
                        ts,
                        order,
                    );
                }
            }
            _ => {}
        }

        // return collected processing results
        proc_result
    }

    /// Returns whether the order was fully filled or not
    pub fn process_order_internal(
        &mut self,
        results: &mut OrderProcessingResult,
        order_id: u64,
        order_asset: u32,
        price_asset: u32,
        side: OrderSide,
        price: f64,
        qty: u64,
        quote_qty: u64, // useful when is_market_order and side == OrderSide::Bid, quote_qty = qty * price
        mut order: OrderWrapper,
        ts: SystemTime,
        is_market_order: bool,
        is_retry: bool,
        is_amend: bool,
    ) -> u64 {
        let qty_left = order.qty_left;

        // take a look at current opposite limit order
        let opposite_order_result = {
            let opposite_queue = match side {
                OrderSide::Bid => &mut self.ask_queue,
                OrderSide::Ask => &mut self.bid_queue,
            };
            opposite_queue.peek().cloned()
        };

        if let Some(mut opposite_order) = opposite_order_result {
            // ? Check that the order being matched has not expired
            if opposite_order.order.has_expired() {
                let order_queue = match opposite_order.order_side {
                    OrderSide::Bid => &mut self.bid_queue,
                    OrderSide::Ask => &mut self.ask_queue,
                };

                order_queue.cancel(opposite_order.order_id, 0, true);

                return self.process_order_internal(
                    results,
                    order_id,
                    order_asset,
                    price_asset,
                    side,
                    price,
                    qty,
                    quote_qty,
                    order,
                    ts,
                    is_market_order,
                    is_retry,
                    is_amend,
                );
            }

            let mut could_be_matched = true;

            // ? If the positions/order_tabs are the same then the orders can't be matched
            match &order.order {
                Order::Spot(spot_order) => {
                    if let Order::Spot(opposite_order) = &opposite_order.order {
                        if spot_order.order_tab.is_some() && opposite_order.order_tab.is_some() {
                            let tab1_hash =
                                spot_order.order_tab.as_ref().unwrap().lock().hash.clone();
                            let tab2_hash = opposite_order
                                .order_tab
                                .as_ref()
                                .unwrap()
                                .lock()
                                .hash
                                .clone();

                            if tab1_hash == tab2_hash {
                                could_be_matched = false;
                            }
                        }
                    }
                }
                Order::Perp(perp_order) => {
                    if let Order::Perp(opposite_order) = &opposite_order.order {
                        if perp_order.position.is_some() && opposite_order.position.is_some() {
                            let pos1_hash = perp_order.position.as_ref().unwrap().hash.clone();
                            let pos2_hash = opposite_order.position.as_ref().unwrap().hash.clone();

                            if pos1_hash == pos2_hash {
                                could_be_matched = false;
                            }
                        }
                    }
                }
            }

            // ? Check if the price is good enough to match
            match side {
                // verify bid/ask price overlap
                OrderSide::Bid => {

                    if price
                        < opposite_order
                            .order
                            .get_price(opposite_order.order_side, Some(true))
                    {
                        could_be_matched = false;
                    }
                }
                OrderSide::Ask => {

                    if price
                        > opposite_order
                            .order
                            .get_price(opposite_order.order_side, Some(false))
                    {
                        could_be_matched = false;
                    }
                }
            };

            if could_be_matched {
                let opposite_qty = opposite_order.qty_left;
                let opposite_quote_qty: u64;

                let matching_complete: bool;
                let is_spot: bool;
                if let Order::Spot(_) = order.order {
                    is_spot = true;
                } else {
                    is_spot = false;
                }

                // ? Set the order hashes are correct
                match &mut opposite_order.order {
                    Order::Spot(ord) => {
                        ord.set_hash();
                    }
                    Order::Perp(ord) => {
                        ord.set_hash();
                    }
                }
                if is_amend {
                    match &mut order.order {
                        Order::Spot(ord) => {
                            ord.set_hash();
                        }
                        Order::Perp(ord) => {
                            ord.set_hash();
                        }
                    }
                }

                if is_market_order && side == OrderSide::Bid && is_spot {
                    // If its a market buy order than we take into account the base qty not the qty
                    let opposite_price = opposite_order
                        .order
                        .get_price(opposite_order.order_side, Some(true));
                    opposite_quote_qty = get_quote_qty(
                        opposite_qty,
                        opposite_price,
                        self.order_asset,
                        self.price_asset,
                        Some(side),
                    );

                    matching_complete = self.order_matching_market_bid(
                        results,
                        opposite_order,
                        order_id,
                        &order,
                        OrderType::Market,
                        side,
                        quote_qty,
                        opposite_quote_qty,
                    )
                } else {
                    matching_complete = self.order_matching(
                        results,
                        opposite_order,
                        order_id,
                        &order,
                        OrderType::Limit,
                        side,
                        qty,
                    );
                    opposite_quote_qty = 0;
                }

                if !matching_complete {
                    let new_qty: u64 = if qty > opposite_qty {
                        qty - opposite_qty
                    } else {
                        0
                    };
                    order.qty_left = new_qty;

                    // process the rest of new limit order
                    return self.process_order_internal(
                        results,
                        order_id,
                        order_asset,
                        price_asset,
                        side,
                        price,
                        new_qty,
                        quote_qty - opposite_quote_qty,
                        order,
                        ts,
                        is_market_order,
                        is_retry,
                        false,
                    );
                }

                0
            } else {
                if !is_market_order {
                    // just insert new order in queue
                    self.store_new_limit_order(results, order_id, side, price, order, ts);
                }

                qty_left
            }
        } else {
            if !is_market_order {
                // just insert new order in queue
                self.store_new_limit_order(results, order_id, side, price, order, ts);
            }

            qty_left
        }
    }

    fn process_order_cancel(
        &mut self,
        results: &mut OrderProcessingResult,
        order_id: u64,
        side: OrderSide,
        user_id: u64,
    ) {
        let order_queue = match side {
            OrderSide::Bid => &mut self.bid_queue,
            OrderSide::Ask => &mut self.ask_queue,
        };

        if order_queue.cancel(order_id, user_id, false) {
            results.push(Ok(Success::Cancelled {
                id: order_id,
                ts: SystemTime::now(),
            }));
        } else {
            results.push(Err(Failed::OrderNotFound(order_id)));
        }
    }

    fn process_order_amend(
        &mut self,
        results: &mut OrderProcessingResult,
        order_id: u64,
        side: OrderSide,
        new_price: f64,
        new_expiration: u64,
        signature: Signature,
        user_id: u64,
        match_only: bool, // if true then only match the order and don't amend it
    ) {
        let order_queue = match side {
            OrderSide::Bid => &mut self.bid_queue,
            OrderSide::Ask => &mut self.ask_queue,
        };

        let qty_left: u64;
        let prev_price: f64;
        let prev_signature: Signature;
        let mut order_wrapper: OrderWrapper;
        if let Some(mut wrapper) = order_queue.remove_order(order_id, user_id, false) {
            prev_price = wrapper.order.get_price(side, None);
            prev_signature = wrapper.signature.clone();

            amend_inner(&mut wrapper, new_price, new_expiration, signature);

            let ts = SystemTime::now();

            results.push(Ok(Success::Amended {
                id: order_id,
                new_price,
                ts,
            }));

            let quote_qty = get_quote_qty(
                wrapper.qty_left,
                new_price,
                self.order_asset,
                self.price_asset,
                None,
            );

            order_wrapper = wrapper.clone();

            qty_left = self.process_order_internal(
                results,
                order_id,
                self.order_asset,
                self.price_asset,
                side,
                new_price,
                wrapper.qty_left,
                quote_qty,
                wrapper,
                ts,
                true,
                false,
                true,
            );
        } else {
            results.push(Err(Failed::OrderNotFound(order_id)));
            return;
        }

        if qty_left > 0 {
            order_wrapper.qty_left = qty_left;

            if match_only {
                amend_inner(
                    &mut order_wrapper,
                    prev_price,
                    new_expiration,
                    prev_signature,
                );
            }

            self.store_new_limit_order(
                results,
                order_id,
                side,
                if match_only { prev_price } else { new_price },
                order_wrapper,
                SystemTime::now(),
            )
        }
    }

    // =====================================
    pub fn get_order(&self, order_id: u64) -> Option<OrderWrapper> {
        if let Some(wrapper) = self.bid_queue.get_order(order_id) {
            Some(wrapper.clone())
        } else if let Some(wrapper) = self.ask_queue.get_order(order_id) {
            Some(wrapper.clone())
        } else {
            // println!("Order not found");

            None
        }
    }

    /* Helpers */
    fn store_new_limit_order(
        &mut self,
        results: &mut OrderProcessingResult,
        order_id: u64,
        side: OrderSide,
        price: f64,
        order: OrderWrapper,
        ts: SystemTime,
    ) {
        let order_queue = match side {
            OrderSide::Bid => &mut self.bid_queue,
            OrderSide::Ask => &mut self.ask_queue,
        };

        if !order_queue.insert(order_id, price, ts, order) {
            results.push(Err(Failed::DuplicateOrderID(order_id)))
        }
    }

    fn order_matching(
        &mut self,
        results: &mut OrderProcessingResult,
        opposite_order: OrderWrapper,
        order_id: u64,
        order: &OrderWrapper,
        order_type: OrderType,
        side: OrderSide,
        qty: u64,
    ) -> bool {
        // ? real processing time
        let deal_time = SystemTime::now();

        let mut order_clone = order.order.clone();
        if let Order::Spot(spot_order) = &mut order_clone {
            spot_order.order_id = order_id;
        } else if let Order::Perp(perp_order) = &mut order_clone {
            perp_order.order_id = order_id;
        }

        // ? match immediately
        if qty < opposite_order.qty_left {
            // fill new limit and modify opposite limit

            // let quote_qty = get_quote_qty(
            //     qty,
            //     opposite_order.order.get_price(opposite_order.order_side),
            //     self.order_asset,
            //     self.price_asset,
            //     if is_market { Some(side) } else { None },
            // );
            let quote_qty = 0;

            // ? report filled new order
            results.push(Ok(Success::Filled {
                order: order_clone,
                signature: order.signature.clone(),
                side,
                order_type,
                price: opposite_order
                    .order
                    .get_price(opposite_order.order_side, Some(side == OrderSide::Bid)),
                qty,
                quote_qty,
                partially_filled: false,
                ts: deal_time,
                user_id: order.user_id,
            }));

            // report partially filled opposite limit order
            results.push(Ok(Success::Filled {
                order: opposite_order.order.clone(),
                signature: opposite_order.signature.clone(),
                side: opposite_order.order_side,
                order_type: OrderType::Limit,
                price: opposite_order
                    .order
                    .get_price(opposite_order.order_side, Some(side == OrderSide::Bid)),
                qty,
                quote_qty,
                partially_filled: true,
                ts: deal_time,
                user_id: opposite_order.user_id,
            }));

            // modify unmatched part of the opposite limit order

            let opposite_queue: &mut OrderQueue;
            let same_queue: &mut OrderQueue;
            match side {
                OrderSide::Bid => {
                    same_queue = &mut self.bid_queue;
                    opposite_queue = &mut self.ask_queue;
                }
                OrderSide::Ask => {
                    same_queue = &mut self.ask_queue;
                    opposite_queue = &mut self.bid_queue;
                }
            };

            let modified_order = OrderWrapper {
                order_id: opposite_order.order_id,
                qty_left: opposite_order.qty_left - qty,
                order_side: opposite_order.order_side,
                order: opposite_order.order,
                signature: opposite_order.signature.clone(),
                user_id: opposite_order.user_id,
            };

            opposite_queue.modify_current_order(modified_order);

            // ? Store pending orders
            opposite_queue.store_pending_order(
                opposite_order.order_id,
                opposite_order.signature,
                opposite_order.order_side,
                opposite_order.qty_left,
                opposite_order.user_id,
            );
            same_queue.store_pending_order(
                order_id,
                order.signature.clone(),
                side,
                qty,
                order.user_id,
            );

            return true;
        } else if qty > opposite_order.qty_left {
            // partially fill new limit order, fill opposite limit and notify to process the rest

            // let quote_qty = get_quote_qty(
            //     opposite_order.qty_left,
            //     opposite_order.order.get_price(opposite_order.order_side),
            //     self.order_asset,
            //     self.price_asset,
            //     if is_market { Some(side) } else { None },
            // );
            let quote_qty = 0;

            // report new order partially filled
            results.push(Ok(Success::Filled {
                order: order_clone,
                signature: order.signature.clone(),
                side,
                order_type,
                price: opposite_order
                    .order
                    .get_price(opposite_order.order_side, Some(side == OrderSide::Bid)),
                qty: opposite_order.qty_left,
                quote_qty,
                partially_filled: true,
                ts: deal_time,
                user_id: order.user_id,
            }));

            // report filled opposite limit order
            results.push(Ok(Success::Filled {
                order: opposite_order.order.clone(),
                signature: opposite_order.signature.clone(),
                side: opposite_order.order_side,
                order_type: OrderType::Limit,
                price: opposite_order
                    .order
                    .get_price(opposite_order.order_side, Some(side == OrderSide::Bid)),
                qty: opposite_order.qty_left,
                quote_qty,
                partially_filled: false,
                ts: deal_time,
                user_id: opposite_order.user_id,
            }));

            // remove filled limit order from the queue
            // remove filled limit order from the queue
            let opposite_queue: &mut OrderQueue;
            let same_queue: &mut OrderQueue;
            match side {
                OrderSide::Bid => {
                    same_queue = &mut self.bid_queue;
                    opposite_queue = &mut self.ask_queue;
                }
                OrderSide::Ask => {
                    same_queue = &mut self.ask_queue;
                    opposite_queue = &mut self.bid_queue;
                }
            };

            opposite_queue.pop();

            // ? Store pending orders
            opposite_queue.store_pending_order(
                opposite_order.order_id,
                opposite_order.signature,
                opposite_order.order_side,
                opposite_order.qty_left,
                opposite_order.user_id,
            );
            same_queue.store_pending_order(
                order_id,
                order.signature.clone(),
                side,
                qty,
                order.user_id,
            );

            // matching incomplete
            return false;
        } else {
            // let quote_qty = get_quote_qty(
            //     qty,
            //     opposite_order.order.get_price(opposite_order.order_side),
            //     self.order_asset,
            //     self.price_asset,
            //     if is_market { Some(side) } else { None },
            // );
            let quote_qty = 0;

            // report filled new order
            results.push(Ok(Success::Filled {
                order: order_clone,
                signature: order.signature.clone(),
                side,
                order_type,
                price: opposite_order
                    .order
                    .get_price(opposite_order.order_side, Some(side == OrderSide::Bid)),
                qty,
                quote_qty,
                partially_filled: false,
                ts: deal_time,
                user_id: order.user_id,
            }));
            // report filled opposite limit order
            results.push(Ok(Success::Filled {
                order: opposite_order.order.clone(),
                signature: opposite_order.signature.clone(),
                side: opposite_order.order_side,
                order_type: OrderType::Limit,
                price: opposite_order
                    .order
                    .get_price(opposite_order.order_side, Some(side == OrderSide::Bid)),
                qty,
                quote_qty,
                partially_filled: false,
                ts: deal_time,
                user_id: opposite_order.user_id,
            }));

            // remove filled limit order from the queue
            let opposite_queue: &mut OrderQueue;
            let same_queue: &mut OrderQueue;
            match side {
                OrderSide::Bid => {
                    same_queue = &mut self.bid_queue;
                    opposite_queue = &mut self.ask_queue;
                }
                OrderSide::Ask => {
                    same_queue = &mut self.ask_queue;
                    opposite_queue = &mut self.bid_queue;
                }
            };

            opposite_queue.pop();

            // ? Store pending orders
            opposite_queue.store_pending_order(
                opposite_order.order_id,
                opposite_order.signature,
                opposite_order.order_side,
                opposite_order.qty_left,
                opposite_order.user_id,
            );
            same_queue.store_pending_order(order_id, order.signature.clone(), side, qty, order_id);
        }

        // complete matching
        true
    }

    fn order_matching_market_bid(
        &mut self,
        results: &mut OrderProcessingResult,
        opposite_order: OrderWrapper,
        order_id: u64,
        order: &OrderWrapper,
        order_type: OrderType,
        side: OrderSide,
        quote_qty: u64,
        opposite_quote_qty: u64,
    ) -> bool {
        // ? real processing time
        let deal_time = SystemTime::now();

        let mut order_clone = order.order.clone();
        if let Order::Spot(spot_order) = &mut order_clone {
            spot_order.order_id = order_id;
        } else if let Order::Perp(perp_order) = &mut order_clone {
            perp_order.order_id = order_id;
        }

        if opposite_quote_qty < quote_qty {
            // ? report filled new order
            results.push(Ok(Success::Filled {
                order: order_clone,
                signature: order.signature.clone(),
                side,
                order_type,
                price: opposite_order
                    .order
                    .get_price(opposite_order.order_side, Some(side == OrderSide::Bid)),
                qty: opposite_order.qty_left,
                quote_qty: opposite_quote_qty,
                partially_filled: true,
                ts: deal_time,
                user_id: order.user_id,
            }));

            // report partially filled opposite limit order
            results.push(Ok(Success::Filled {
                order: opposite_order.order.clone(),
                signature: opposite_order.signature.clone(),
                side: opposite_order.order_side,
                order_type: OrderType::Limit,
                price: opposite_order
                    .order
                    .get_price(opposite_order.order_side, Some(side == OrderSide::Bid)),
                qty: opposite_order.qty_left,
                quote_qty: opposite_quote_qty,
                partially_filled: false,
                ts: deal_time,
                user_id: opposite_order.user_id,
            }));

            // remove filled limit order from the queue
            let opposite_queue: &mut OrderQueue;
            match side {
                OrderSide::Bid => {
                    opposite_queue = &mut self.ask_queue;
                }
                OrderSide::Ask => {
                    opposite_queue = &mut self.bid_queue;
                }
            };

            opposite_queue.pop();

            // ? Store pending orders
            opposite_queue.store_pending_order(
                opposite_order.order_id,
                opposite_order.signature,
                opposite_order.order_side,
                opposite_order.qty_left,
                opposite_order.user_id,
            );

            // matching incomplete
            return false;
        } else if opposite_quote_qty > quote_qty {
            let qty = get_qty_from_quote(
                quote_qty,
                opposite_order
                    .order
                    .get_price(opposite_order.order_side, Some(true)),
                self.order_asset,
                self.price_asset,
            );

            // ? report filled new order
            results.push(Ok(Success::Filled {
                order: order_clone,
                signature: order.signature.clone(),
                side,
                order_type,
                price: opposite_order
                    .order
                    .get_price(opposite_order.order_side, Some(side == OrderSide::Bid)),
                qty,
                quote_qty,
                partially_filled: false,
                ts: deal_time,
                user_id: order.user_id,
            }));

            // report partially filled opposite limit order
            results.push(Ok(Success::Filled {
                order: opposite_order.order.clone(),
                signature: opposite_order.signature.clone(),
                side: opposite_order.order_side,
                order_type: OrderType::Limit,
                price: opposite_order
                    .order
                    .get_price(opposite_order.order_side, Some(side == OrderSide::Bid)),
                qty,
                quote_qty,
                partially_filled: true,
                ts: deal_time,
                user_id: opposite_order.user_id,
            }));

            // modify unmatched part of the opposite limit order

            let opposite_queue: &mut OrderQueue;
            match side {
                OrderSide::Bid => {
                    opposite_queue = &mut self.ask_queue;
                }
                OrderSide::Ask => {
                    opposite_queue = &mut self.bid_queue;
                }
            };

            let modified_order = OrderWrapper {
                order_id: opposite_order.order_id,
                qty_left: opposite_order.qty_left - qty,
                order_side: opposite_order.order_side,
                order: opposite_order.order,
                signature: opposite_order.signature.clone(),
                user_id: opposite_order.user_id,
            };

            opposite_queue.modify_current_order(modified_order);

            // ? Store pending orders
            opposite_queue.store_pending_order(
                opposite_order.order_id,
                opposite_order.signature,
                opposite_order.order_side,
                opposite_order.qty_left,
                opposite_order.user_id,
            );

            return true;
        } else {
            let qty = get_qty_from_quote(
                quote_qty,
                opposite_order
                    .order
                    .get_price(opposite_order.order_side, Some(side == OrderSide::Bid)),
                self.order_asset,
                self.price_asset,
            );

            // report filled new order
            results.push(Ok(Success::Filled {
                order: order_clone,
                signature: order.signature.clone(),
                side,
                order_type,
                price: opposite_order
                    .order
                    .get_price(opposite_order.order_side, Some(side == OrderSide::Bid)),
                qty,
                quote_qty,
                partially_filled: false,
                ts: deal_time,
                user_id: order.user_id,
            }));
            // report filled opposite limit order
            results.push(Ok(Success::Filled {
                order: opposite_order.order.clone(),
                signature: opposite_order.signature.clone(),
                side: opposite_order.order_side,
                order_type: OrderType::Limit,
                price: opposite_order
                    .order
                    .get_price(opposite_order.order_side, Some(side == OrderSide::Bid)),
                qty,
                quote_qty,
                partially_filled: false,
                ts: deal_time,
                user_id: opposite_order.user_id,
            }));

            // remove filled limit order from the queue
            let opposite_queue: &mut OrderQueue;
            match side {
                OrderSide::Bid => {
                    opposite_queue = &mut self.ask_queue;
                }
                OrderSide::Ask => {
                    opposite_queue = &mut self.bid_queue;
                }
            };

            opposite_queue.pop();

            // ? Store pending orders
            opposite_queue.store_pending_order(
                opposite_order.order_id,
                opposite_order.signature,
                opposite_order.order_side,
                opposite_order.qty_left,
                opposite_order.user_id,
            );
            return true;
        }
    }

    /// When a position is updated go through all the orders from the current user and update the position in the order
    /// so that the order doesen't fail when it is matched (otherweise the user would need to resubmit the order after every position update)
    pub fn update_order_positions(&mut self, user_id: u64, new_position: &Option<PerpPosition>) {
        self.bid_queue.update_order_position(user_id, new_position);

        self.ask_queue.update_order_position(user_id, new_position);
    }

    /// * Restore the orderbook (in case of server restarts)
    pub fn restore_spot_order_book(
        &mut self,
        spot_bid_orders: Vec<SpotOrderRestoreMessageInner>,
        spot_ask_orders: Vec<SpotOrderRestoreMessageInner>,
    ) {
        let mut max_order_id: u64 = 0;

        for order in spot_bid_orders {
            if order.order.is_none() {
                continue;
            }

            if order.amount
                <= order.order.as_ref().unwrap().amount_received
                    - DUST_AMOUNT_PER_ASSET
                        [&order.order.as_ref().unwrap().token_received.to_string()]
            {
                continue;
            };

            max_order_id = if order.order_id > max_order_id {
                order.order_id
            } else {
                max_order_id
            };

            self._restore_spot_inner(order, OrderSide::Bid);
        }

        for order in spot_ask_orders {
            if order.order.is_none() {
                continue;
            }

            if order.amount
                <= order.order.as_ref().unwrap().amount_spent
                    - DUST_AMOUNT_PER_ASSET[&order.order.as_ref().unwrap().token_spent.to_string()]
            {
                continue;
            };

            max_order_id = if order.order_id > max_order_id {
                order.order_id
            } else {
                max_order_id
            };

            self._restore_spot_inner(order, OrderSide::Ask);
        }

        let max_seq_id = max_order_id / 2_u64.pow(16);

        self.seq.set_id(max_seq_id + 1);
    }

    pub fn restore_perp_order_book(
        &mut self,
        perp_bid_orders: Vec<PerpOrderRestoreMessageInner>,
        perp_ask_orders: Vec<PerpOrderRestoreMessageInner>,
    ) {
        let mut max_order_id: u64 = 0;

        for order in perp_bid_orders {
            if order.amount
                <= order.order.as_ref().unwrap().synthetic_amount
                    - DUST_AMOUNT_PER_ASSET
                        [&order.order.as_ref().unwrap().synthetic_token.to_string()]
            {
                continue;
            };

            max_order_id = if order.order_id > max_order_id {
                order.order_id
            } else {
                max_order_id
            };

            self._restore_perp_inner(order, OrderSide::Bid);
        }

        for order in perp_ask_orders {
            if order.amount
                <= order.order.as_ref().unwrap().synthetic_amount
                    - DUST_AMOUNT_PER_ASSET
                        [&order.order.as_ref().unwrap().synthetic_token.to_string()]
            {
                continue;
            };

            max_order_id = if order.order_id > max_order_id {
                order.order_id
            } else {
                max_order_id
            };

            self._restore_perp_inner(order, OrderSide::Ask);
        }

        let max_seq_id = max_order_id / 2_u64.pow(16);

        self.seq.set_id(max_seq_id + 1);
    }

    pub fn _restore_spot_inner(
        &mut self,
        order: SpotOrderRestoreMessageInner,
        order_side: OrderSide,
    ) {
        let signature = Signature::try_from(
            order
                .order
                .as_ref()
                .unwrap()
                .signature
                .as_ref()
                .unwrap()
                .clone(),
        )
        .unwrap();
        let user_id = order.order.as_ref().unwrap().user_id;

        if let Ok(mut limit_order) = LimitOrder::try_from(order.order.unwrap()) {
            let order_id = order.order_id;
            let amount = order.amount;
            let price = order.price;
            let timestamp = SystemTime::UNIX_EPOCH + Duration::from_secs(order.timestamp);

            // ? If the order tab already exists as part of a different order link this order to that Mutex
            link_order_tab(&mut limit_order, &self.bid_queue, &self.ask_queue);

            let order = Order::Spot(limit_order);

            if order.has_expired() {
                return;
            }

            let wrapper = OrderWrapper {
                order_id,
                order,
                order_side,
                qty_left: amount,
                signature,
                user_id,
            };

            if order_side == OrderSide::Bid {
                self.bid_queue.insert(order_id, price, timestamp, wrapper);
            } else {
                self.ask_queue.insert(order_id, price, timestamp, wrapper);
            }
        }
    }

    pub fn _restore_perp_inner(
        &mut self,
        order: PerpOrderRestoreMessageInner,
        order_side: OrderSide,
    ) {
        let signature = Signature::try_from(
            order
                .order
                .as_ref()
                .unwrap()
                .signature
                .as_ref()
                .unwrap()
                .clone(),
        )
        .unwrap();
        let user_id = order.order.as_ref().unwrap().user_id;

        if let Ok(perp_order) = PerpOrder::try_from(order.order.unwrap()) {
            let order_id = order.order_id;
            let amount = order.amount;
            let price = order.price;
            let timestamp = SystemTime::UNIX_EPOCH + Duration::from_secs(order.timestamp);

            let wrapper = OrderWrapper {
                order_id,
                order: Order::Perp(perp_order),
                order_side,
                qty_left: amount,
                signature,
                user_id,
            };

            if order_side == OrderSide::Bid {
                self.bid_queue.insert(order_id, price, timestamp, wrapper);
            } else {
                self.ask_queue.insert(order_id, price, timestamp, wrapper);
            }
        };
    }

    /// * get impact bid/ask price
    pub fn get_impact_prices(&self, impact_notional: u64) -> Result<(u64, u64), String> {
        let bid_price_ = self.bid_queue.get_impact_price(impact_notional);
        let ask_price_ = self.ask_queue.get_impact_price(impact_notional);

        if bid_price_ == 0.0 || ask_price_ == 0.0 {
            // TODO: What if order book is empty how do we apply funding then
            return Err("No impact price".to_string());
        }

        let price_decimals: &u8 = PRICE_DECIMALS_PER_ASSET
            .get(self.order_asset.to_string().as_str())
            .unwrap();

        let impact_bid_price = bid_price_ * 10_f64.powi(*price_decimals as i32);
        let impact_ask_price = ask_price_ * 10_f64.powi(*price_decimals as i32);

        return Ok((impact_bid_price as u64, impact_ask_price as u64));
    }

    ///  * get market_price
    pub fn get_market_price(&mut self) -> Result<u64, String> {
        let top_bid_order = self.bid_queue.peek();
        if top_bid_order.is_none() {
            return Err("No market price".to_string());
        }
        let top_ask_order = self.ask_queue.peek();
        if top_ask_order.is_none() {
            return Err("No market price".to_string());
        }

        let bid_price_ = top_bid_order.unwrap().order.get_price(OrderSide::Bid, None);

        let ask_price_ = top_ask_order.unwrap().order.get_price(OrderSide::Ask, None);

        if bid_price_ == 0.0 || ask_price_ == 0.0 {
            return Err("No market price".to_string());
        }

        let market_price = (bid_price_ + ask_price_) / 2.0;
        let market_price = market_price
            * 10_f64.powi(
                *PRICE_DECIMALS_PER_ASSET
                    .get(self.order_asset.to_string().as_str())
                    .unwrap() as i32,
            );

        return Ok(market_price as u64);
    }

    /// * Clears all orders that have expired from both queues
    pub fn clear_expired_orders(&mut self) {
        self.ask_queue.remove_expired_orders();
        self.bid_queue.remove_expired_orders();
    }
}
