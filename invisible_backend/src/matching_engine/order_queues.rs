use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};
use std::sync::Arc;
use std::time::{self, SystemTime};

use num_bigint::BigUint;
use parking_lot::Mutex;

use crate::order_tab::OrderTab;
use crate::perpetual::perp_position::PerpPosition;
use crate::utils::crypto_utils::Signature;

use super::domain::{Order, OrderSide, OrderWrapper};
use super::orders::amend_inner;

#[derive(Clone, Debug)]
struct OrderIndex {
    id: u64,
    price: f64,
    timestamp: time::SystemTime,
    order_side: OrderSide,
}

// Arrange at first by price and after that by time
impl Ord for OrderIndex {
    fn cmp(&self, other: &Self) -> Ordering {
        if self.price < other.price {
            match self.order_side {
                OrderSide::Bid => Ordering::Less,
                OrderSide::Ask => Ordering::Greater,
            }
        } else if self.price > other.price {
            match self.order_side {
                OrderSide::Bid => Ordering::Greater,
                OrderSide::Ask => Ordering::Less,
            }
        } else {
            // FIFO
            other.timestamp.cmp(&self.timestamp)
        }
    }
}

impl PartialOrd for OrderIndex {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for OrderIndex {
    fn eq(&self, other: &Self) -> bool {
        if self.price > other.price || self.price < other.price {
            false
        } else {
            self.timestamp == other.timestamp
        }
    }
}

impl Eq for OrderIndex {}

#[derive(Debug)]
/// Public methods
pub struct OrderQueue {
    // use Option in order to replace heap in mutable borrow
    idx_queue: Option<BinaryHeap<OrderIndex>>,
    orders: HashMap<u64, OrderWrapper>,
    op_counter: u64,
    max_stalled: u64,
    queue_side: OrderSide,
    pending_orders: HashMap<u64, (Signature, OrderSide, u64, u64)>, // order_id => (signature, order_side, qty_left, user_id)
}

impl OrderQueue {
    /// Create new order queue
    ///
    /// Queue is universal and could be used for both asks and bids
    pub fn new(side: OrderSide, max_stalled: u64, capacity: usize) -> Self {
        OrderQueue {
            idx_queue: Some(BinaryHeap::with_capacity(capacity)),
            orders: HashMap::with_capacity(capacity),
            op_counter: 0,
            max_stalled,
            queue_side: side,
            pending_orders: HashMap::with_capacity(capacity),
        }
    }

    pub fn peek(&mut self) -> Option<&OrderWrapper> {
        // get best order ID
        let order_id = self.get_current_order_id()?;

        // obtain order info
        if self.orders.contains_key(&order_id) {
            self.orders.get(&order_id)
        } else {
            self.idx_queue.as_mut().unwrap().pop()?;
            self.peek()
        }
    }

    pub fn pop(&mut self) -> Option<(OrderWrapper, SystemTime)> {
        // remove order index from queue in any case
        let index_item = self.idx_queue.as_mut()?.pop()?;

        let order_id = index_item.id;
        let ts = index_item.timestamp;

        if self.orders.contains_key(&order_id) {
            if let Some(ord) = self.orders.remove(&order_id) {
                Some((ord, ts))
            } else {
                None
            }
        } else {
            self.pop()
        }
    }

    // Add new limit order to the queue
    pub fn insert(
        &mut self,
        id: u64,
        price: f64,
        ts: time::SystemTime,
        mut order: OrderWrapper,
    ) -> bool {
        if self.orders.contains_key(&id) {
            let mut is_retry = false;
            for x in self.idx_queue.as_mut().unwrap().iter() {
                if x.id == id {
                    is_retry = true;
                    break;
                }
            }

            // do not update existing order
            if is_retry {
                return false;
            }
        }

        // store new order
        self.idx_queue.as_mut().unwrap().push(OrderIndex {
            id,
            price,
            timestamp: ts,
            order_side: self.queue_side,
        });

        order.order.set_id(id);
        order.order_id = id;

        self.orders.insert(id, order);

        true
    }

    // use it when price was changed
    pub fn amend(
        &mut self,
        id: u64,
        user_id: u64,
        price: f64,
        new_expiration: u64,
        signature: Signature,
        ts: time::SystemTime,
    ) -> bool {
        if self.orders.contains_key(&id) {
            let mut wrapper = self.orders.get_mut(&id).unwrap();

            if wrapper.user_id != user_id {
                return false;
            };

            amend_inner(&mut wrapper, price, new_expiration, signature);

            // store new order data
            self.rebuild_idx(id, price, ts);

            true
        } else {
            false
        }
    }

    /// This cancels an order with order_id. \
    /// If force is true, then order will be cancelled even if it's not owned by user_id
    pub fn cancel(&mut self, order_id: u64, user_id: u64, force: bool) -> bool {
        match self.orders.remove(&order_id) {
            Some(wrapper) => {
                // Only a user can cancel
                if wrapper.user_id != user_id && !force {
                    return false;
                }
                self.clean_check();
                true
            }
            None => false,
        }
    }

    pub fn remove_order(
        &mut self,
        order_id: u64,
        user_id: u64,
        force: bool,
    ) -> Option<OrderWrapper> {
        let wrapper = self.orders.get(&order_id)?;

        if (wrapper.user_id != user_id) && !force {
            return None;
        }

        let wrapper = self.orders.remove(&order_id)?;

        self.remove_stalled();

        Some(wrapper)
    }

    /// Returns order by id
    pub fn get_order(&self, id: u64) -> Option<&OrderWrapper> {
        self.orders.get(&id)
    }

    // *-----------------------------------------------------------------------------
    pub fn get_tab_mutex(&self, tab_hash: &BigUint) -> Option<Arc<Mutex<OrderTab>>> {
        for (_, ord_) in self.orders.iter() {
            if let Order::Spot(ord) = &ord_.order {
                if let Some(tab) = &ord.order_tab {
                    let tab_lock = tab.lock();
                    if tab_lock.hash == *tab_hash {
                        return Some(tab.clone());
                    }
                }
            }
        }

        None
    }

    // *-----------------------------------------------------------------------------

    /// Increases the quantity of the order with id by increase_qty
    fn increase_qty(&mut self, id: u64, increase_qty: u64) {
        if !self.orders.contains_key(&id) {
            return;
        }

        let order = self.orders.get_mut(&id).unwrap();

        order.qty_left += increase_qty;
    }

    /* Internal methods */

    /// Used internally when current order is partially matched.
    ///
    /// Note: do not modify price or time, because index doesn't change!
    pub fn modify_current_order(&mut self, new_order: OrderWrapper) -> bool {
        if let Some(order_id) = self.get_current_order_id() {
            if self.orders.contains_key(&order_id) {
                self.orders.insert(order_id, new_order);
                return true;
            }
        }
        false
    }

    /// Verify if queue should be cleaned
    fn clean_check(&mut self) {
        if self.op_counter > self.max_stalled {
            self.op_counter = 0;
            self.remove_stalled()
        } else {
            self.op_counter += 1;
        }
    }

    /// Remove dangling indices without orders from queue
    fn remove_stalled(&mut self) {
        if let Some(idx_queue) = self.idx_queue.take() {
            let mut active_orders = idx_queue.into_vec();
            active_orders.retain(|order_ptr| self.orders.contains_key(&order_ptr.id));
            self.idx_queue = Some(BinaryHeap::from(active_orders));
        }
    }

    /// Recreate order-index queue with changed index info
    fn rebuild_idx(&mut self, id: u64, price: f64, ts: time::SystemTime) {
        if let Some(idx_queue) = self.idx_queue.take() {
            // deconstruct queue
            let mut active_orders = idx_queue.into_vec();
            // remove old idx value
            active_orders.retain(|order_ptr| order_ptr.id != id);
            // insert new one
            active_orders.push(OrderIndex {
                id,
                price,
                timestamp: ts,
                order_side: self.queue_side,
            });
            // construct new queue
            let amended_queue = BinaryHeap::from(active_orders);
            self.idx_queue = Some(amended_queue);
        }
    }

    /// Return ID of current order in queue
    fn get_current_order_id(&self) -> Option<u64> {
        let order_id = self.idx_queue.as_ref()?.peek()?;
        Some(order_id.id)
    }

    /// Remove expired orders from the queue
    pub fn remove_expired_orders(&mut self) {
        let mut expired_order_ids = vec![];

        for (id, order) in self.orders.iter_mut() {
            if order.order.has_expired() {
                expired_order_ids.push(*id);
            }
        }

        for id in expired_order_ids {
            self.orders.remove(&id);
            self.op_counter += 1;
        }

        self.clean_check();
    }

    /// Returns the liquidity of the queue
    /// ### Returns:
    /// * liquidity: Vec<(f64, u64)> - vector of tuples (price, liquidity)
    pub fn visualize(&self) -> Vec<(f64, u64, u64, u64)> {
        // sort the idx queue
        let mut idx_queue = self.idx_queue.as_ref().unwrap().clone().into_vec();
        idx_queue.sort_by(|a, b| a.partial_cmp(b).unwrap());

        let book = idx_queue
            .iter()
            .rev()
            .filter_map(|idx| {
                let ord = self.orders.get(&idx.id);

                // Get the order time in seconds since UNIX_EPOCH
                let ts = idx.timestamp;
                let timestamp = ts
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .expect("Time went backwards")
                    .as_secs();

                if let Some(ord) = ord {
                    if ord.qty_left <= 0 {
                        return None;
                    }

                    return Some((idx.price, ord.qty_left, timestamp, ord.order_id));
                } else {
                    return None;
                }
            })
            .collect::<Vec<(f64, u64, u64, u64)>>();

        return book;
    }

    // *-----------------------------------------------------------------------------

    /// Update the order position
    pub fn update_order_position(&mut self, user_id: u64, new_position: &Option<PerpPosition>) {
        if new_position.is_some() {
            self.orders.iter_mut().for_each(|(_, wrapper)| {
                if wrapper.user_id == user_id {
                    if let Order::Perp(ord) = &mut wrapper.order {
                        if ord.position.is_some()
                            && ord
                                .position
                                .as_ref()
                                .unwrap()
                                .position_header
                                .position_address
                                == new_position
                                    .as_ref()
                                    .unwrap()
                                    .position_header
                                    .position_address
                        {
                            ord.position = new_position.clone();
                        }
                    }
                }
            });
        }

        return;
    }

    /// Gets the impact price from the impact notional value
    pub fn get_impact_price(&self, impact_notional: u64) -> f64 {
        let mut sum = 0;

        let idx_queue = self.idx_queue.as_ref().unwrap().clone();
        // into_sorted_vec();

        let mut price: f64 = 0.0;
        for i in idx_queue.into_sorted_vec() {
            let order_ = self.orders.get(&i.id);
            if order_.is_none() {
                continue;
            }

            let order = order_.unwrap();
            sum += order.qty_left;
            price = i.price;
            if sum >= impact_notional {
                break;
            }
        }

        price
    }

    // *-----------------------------------------------------------------------------

    /// Stores a pending order!
    ///
    /// Stores the signature and order_side for order_id in case that order fails and needs to
    /// be reinserted into the orderbook.
    pub fn store_pending_order(
        &mut self,
        order_id: u64,
        sig: Signature,
        side: OrderSide,
        qty: u64,
        user_id: u64,
    ) {
        if !self.pending_orders.contains_key(&order_id) {
            self.pending_orders
                .insert(order_id, (sig, side, qty, user_id));
        }
    }

    /// Removes a pending order!
    ///
    /// Removes reduce_qty amount from the pending order with order_id from
    /// when that order is filled.
    /// In case of cancellations or rejections set force to true to remove the order
    pub fn reduce_pending_order(&mut self, order_id: u64, reduce_qty: u64, force: bool) {
        if !self.pending_orders.contains_key(&order_id) {
            return;
        }
        let (sig, side, qty, user_id) = self.pending_orders.remove(&order_id).unwrap();

        if qty <= reduce_qty || force {
            self.pending_orders.remove(&order_id);
        } else {
            let new_qty = qty - reduce_qty;

            self.pending_orders
                .insert(order_id, (sig, side, new_qty, user_id));
        }
    }

    /// Restores a pending order!
    ///
    /// Reinsert the order back into the order book with the signature, side, and qty from the pending order,
    /// or increase the qty of that order if it has already been reinserted, by another partial fill
    pub fn restore_pending_order(&mut self, order: Order, order_qty: u64) {
        match order {
            Order::Spot(limit_order) => {
                // ? If pending order was already restored increase the qty of that order in the book
                if self.orders.contains_key(&limit_order.order_id) {
                    self.increase_qty(limit_order.order_id, order_qty);
                }

                if !self.pending_orders.contains_key(&limit_order.order_id) {
                    return;
                }
                let (sig, side, _, user_id) =
                    self.pending_orders.remove(&limit_order.order_id).unwrap();

                let order_id = limit_order.order_id;
                let order__ = Order::Spot(limit_order);
                let price = order__.get_price(side, None);
                let ts = time::SystemTime::now();

                let wrapper = OrderWrapper {
                    order_id,
                    order_side: side,
                    qty_left: order_qty,
                    signature: sig,
                    order: order__,
                    user_id,
                };

                self.insert(order_id, price, ts, wrapper);
            }
            Order::Perp(perp_order) => {
                // ? If pending order was already restored increase the qty of that order in the book
                if self.orders.contains_key(&perp_order.order_id) {
                    self.increase_qty(perp_order.order_id, order_qty);
                }

                if !self.pending_orders.contains_key(&perp_order.order_id) {
                    return;
                }
                let (sig, side, _, user_id) =
                    self.pending_orders.remove(&perp_order.order_id).unwrap();

                let order_id = perp_order.order_id;
                let order__ = Order::Perp(perp_order);
                let price = order__.get_price(side, None);
                let ts = time::SystemTime::now();

                let wrapper = OrderWrapper {
                    order_id,
                    order_side: side,
                    qty_left: order_qty,
                    signature: sig,
                    order: order__,
                    user_id,
                };

                self.insert(order_id, price, ts, wrapper);
            }
        }
    }
}
