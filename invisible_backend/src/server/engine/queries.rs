use std::{collections::HashMap, sync::Arc};

use super::super::grpc::engine_proto::{
    ActiveOrder, ActivePerpOrder, BookEntry, FundingInfo, FundingReq, FundingRes, GrpcNote,
    GrpcOrderTab, LiquidityReq, LiquidityRes, OrdersReq, OrdersRes, StateInfoReq, StateInfoRes,
};

use crate::server::grpc::engine_proto::{EmptyReq, IndexPriceRes};
use crate::transaction_batch::TransactionBatch;
use crate::{
    matching_engine::{
        domain::{Order, OrderSide as OBOrderSide},
        orderbook::OrderBook,
    },
    perpetual::PositionEffectType,
};

use crate::utils::{errors::send_liquidity_error_reply, notes::Note};

use tokio::sync::Mutex as TokioMutex;
use tonic::{Request, Response, Status};

pub async fn get_liquidity_inner(
    order_books: &HashMap<u16, Arc<TokioMutex<OrderBook>>>,
    perp_order_books: &HashMap<u16, Arc<TokioMutex<OrderBook>>>,

    //
    request: Request<LiquidityReq>,
) -> Result<Response<LiquidityRes>, Status> {
    tokio::task::yield_now().await;

    let req: LiquidityReq = request.into_inner();

    let order_book_m: &Arc<TokioMutex<OrderBook>>;

    if req.is_perp {
        if !perp_order_books.contains_key(&(req.market_id as u16)) {
            return send_liquidity_error_reply(
                "No market found for given base and quote token".to_string(),
            );
        }

        order_book_m = perp_order_books.get(&(req.market_id as u16)).unwrap();
    } else {
        if !order_books.contains_key(&(req.market_id as u16)) {
            return send_liquidity_error_reply(
                "No market found for given base and quote token".to_string(),
            );
        }

        // ? Get the relevant orderbook from the market_id
        order_book_m = order_books.get(&(req.market_id as u16)).unwrap();
    }

    let order_book = order_book_m.lock().await;

    let bid_queue = order_book
        .bid_queue
        .visualize()
        .into_iter()
        .map(|(p, qt, ts, _oid)| BookEntry {
            price: p,
            amount: qt,
            timestamp: ts,
        })
        .collect::<Vec<BookEntry>>();
    let ask_queue = order_book
        .ask_queue
        .visualize()
        .into_iter()
        .map(|(p, qt, ts, _oid)| BookEntry {
            price: p,
            amount: qt,
            timestamp: ts,
        })
        .collect::<Vec<BookEntry>>();

    drop(order_book);

    let reply = LiquidityRes {
        successful: true,
        ask_queue,
        bid_queue,
        error_message: "".to_string(),
    };

    return Ok(Response::new(reply));
}

pub async fn get_orders_inner(
    tx_batch: &Arc<TokioMutex<TransactionBatch>>,
    order_books: &HashMap<u16, Arc<TokioMutex<OrderBook>>>,
    perp_order_books: &HashMap<u16, Arc<TokioMutex<OrderBook>>>,
    //
    request: Request<OrdersReq>,
) -> Result<Response<OrdersRes>, Status> {
    tokio::task::yield_now().await;

    let req: OrdersReq = request.into_inner();

    let tx_batch_m = tx_batch.lock().await;
    let partial_fill_tracker = Arc::clone(&tx_batch_m.partial_fill_tracker);
    let perpetual_partial_fill_tracker = Arc::clone(&tx_batch_m.perpetual_partial_fill_tracker);
    drop(tx_batch_m);

    let mut bad_order_ids: Vec<u64> = Vec::new();
    let mut active_orders: Vec<ActiveOrder> = Vec::new();
    let mut pfr_notes: Vec<Note> = Vec::new();
    for order_id in req.order_ids {
        let market_id = order_id as u16;

        if !order_books.contains_key(&market_id) {
            // ? order is non-existent or invalid
            bad_order_ids.push(order_id);

            continue;
        }

        let order_book = order_books.get(&market_id).unwrap().lock().await;
        let wrapper_ = order_book.get_order(order_id);

        if let Some(wrapper) = wrapper_ {
            let order_side = wrapper.order_side;
            let price = wrapper.order.get_price(order_side, None);
            let qty_left = wrapper.qty_left;
            if let Order::Spot(limit_order) = &wrapper.order {
                let base_asset: u32;
                let quote_asset: u32;
                if order_side == OBOrderSide::Bid {
                    base_asset = limit_order.token_received;
                    quote_asset = limit_order.token_spent;
                } else {
                    base_asset = limit_order.token_spent;
                    quote_asset = limit_order.token_received
                }

                let notes_in: Vec<GrpcNote>;
                let refund_note;
                if limit_order.spot_note_info.is_some() {
                    let notes_info = limit_order.spot_note_info.as_ref().unwrap();

                    notes_in = notes_info
                        .notes_in
                        .iter()
                        .map(|n| GrpcNote::from(n.clone()))
                        .collect();

                    refund_note = if notes_info.refund_note.is_some() {
                        Some(GrpcNote::from(
                            notes_info.refund_note.as_ref().unwrap().clone(),
                        ))
                    } else {
                        None
                    };
                } else {
                    notes_in = vec![];
                    refund_note = None;
                };

                let order_tab = if limit_order.order_tab.is_some() {
                    let lock = limit_order.order_tab.as_ref().unwrap().lock();
                    Some(GrpcOrderTab::from(lock.clone()))
                } else {
                    None
                };

                let active_order = ActiveOrder {
                    order_id: limit_order.order_id,
                    expiration_timestamp: limit_order.expiration_timestamp,
                    base_asset,
                    quote_asset,
                    order_side: order_side == OBOrderSide::Bid,
                    fee_limit: limit_order.fee_limit,
                    price,
                    qty_left,
                    notes_in,
                    refund_note,
                    order_tab,
                };

                active_orders.push(active_order);
            }
        } else {
            bad_order_ids.push(order_id);
        }
        drop(order_book);

        let partial_fill_tracker_m = partial_fill_tracker.lock();
        let pfr_info = partial_fill_tracker_m.get(&(order_id % 2_u64.pow(32)));
        if pfr_info.is_some() && pfr_info.unwrap().0.is_some() {
            pfr_notes.push(pfr_info.unwrap().0.as_ref().unwrap().clone());
        }
        drop(partial_fill_tracker_m);
    }

    let mut bad_perp_order_ids: Vec<u64> = Vec::new();
    let mut active_perp_orders: Vec<ActivePerpOrder> = Vec::new();

    for order_id in req.perp_order_ids {
        let market_id = order_id as u16;

        if !perp_order_books.contains_key(&market_id) {
            // ? order is non-existent or invalid
            bad_order_ids.push(order_id);

            continue;
        }

        let order_book = perp_order_books.get(&market_id).unwrap().lock().await;
        let wrapper = order_book.get_order(order_id);

        if let Some(wrapper) = wrapper {
            let order_side = wrapper.order_side;
            let price = wrapper.order.get_price(order_side, None);
            let qty_left = wrapper.qty_left;
            if let Order::Perp(perp_order) = &wrapper.order {
                let position_effect_type = match perp_order.position_effect_type {
                    PositionEffectType::Open => 0,
                    PositionEffectType::Modify => 1,
                    PositionEffectType::Close => 2,
                };

                let initial_margin: u64;
                let notes_in: Vec<GrpcNote>;
                let refund_note: Option<GrpcNote>;
                let position_address: String;
                if position_effect_type == 0 {
                    let open_order_fields = perp_order.open_order_fields.clone().unwrap();
                    initial_margin = open_order_fields.initial_margin;
                    notes_in = open_order_fields
                        .notes_in
                        .into_iter()
                        .map(|n| GrpcNote::from(n))
                        .collect();
                    refund_note = if open_order_fields.refund_note.is_some() {
                        Some(GrpcNote::from(open_order_fields.refund_note.unwrap()))
                    } else {
                        None
                    };
                    position_address = "".to_string();
                } else {
                    initial_margin = 0;
                    notes_in = vec![];
                    refund_note = None;
                    position_address = perp_order
                        .position
                        .clone()
                        .unwrap()
                        .position_header
                        .position_address
                        .to_string();
                }

                let active_order = ActivePerpOrder {
                    order_id: perp_order.order_id,
                    expiration_timestamp: perp_order.expiration_timestamp,
                    synthetic_token: perp_order.synthetic_token,
                    position_effect_type,
                    order_side: order_side == OBOrderSide::Bid,
                    fee_limit: perp_order.fee_limit,
                    price,
                    qty_left,
                    initial_margin,
                    notes_in,
                    refund_note,
                    position_address,
                };

                active_perp_orders.push(active_order)
            }
        } else {
            bad_perp_order_ids.push(order_id);
        }

        let perpetual_partial_fill_tracker_m = perpetual_partial_fill_tracker.lock();
        let pfr_info = perpetual_partial_fill_tracker_m.get(&order_id);
        if let Some(pfr_info) = pfr_info {
            if let Some(pfr_note) = &pfr_info.0 {
                pfr_notes.push(pfr_note.clone());
            }
        }
        drop(perpetual_partial_fill_tracker_m);
    }

    let reply = OrdersRes {
        bad_order_ids,
        orders: active_orders,
        bad_perp_order_ids,
        perp_orders: active_perp_orders,
        pfr_notes: pfr_notes.into_iter().map(|n| GrpcNote::from(n)).collect(),
    };

    return Ok(Response::new(reply));
}

pub async fn get_state_info_inner(
    tx_batch: &Arc<TokioMutex<TransactionBatch>>,
    _: Request<StateInfoReq>,
) -> Result<Response<StateInfoRes>, Status> {
    tokio::task::yield_now().await;

    let tx_batch_m = tx_batch.lock().await;
    let state_tree = Arc::clone(&tx_batch_m.state_tree);
    drop(tx_batch_m);

    let state_tree = state_tree.lock();
    let state_tree_leaves = state_tree
        .leaf_nodes
        .iter()
        .map(|x| x.to_string())
        .collect();
    drop(state_tree);

    let reply = StateInfoRes {
        state_tree: state_tree_leaves,
    };

    return Ok(Response::new(reply));
}

pub async fn get_index_prices_inner(
    tx_batch: &Arc<TokioMutex<TransactionBatch>>,
    _: Request<EmptyReq>,
) -> Result<Response<IndexPriceRes>, Status> {
    tokio::task::yield_now().await;

    let mut tokens = vec![];
    let mut index_prices = vec![];
    let tx_batch_m = tx_batch.lock().await;
    tx_batch_m
        .latest_index_price
        .iter()
        .for_each(|(token, price)| {
            tokens.push(*token);
            index_prices.push(*price);
        });
    drop(tx_batch_m);

    let reply = IndexPriceRes {
        tokens,
        index_prices,
    };

    return Ok(Response::new(reply));
}

pub async fn get_funding_info_inner(
    tx_batch: &Arc<TokioMutex<TransactionBatch>>,
    _: Request<FundingReq>,
) -> Result<Response<FundingRes>, Status> {
    tokio::task::yield_now().await;

    let tx_batch_m = tx_batch.lock().await;
    let funding_rates = tx_batch_m.funding_rates.clone();
    let funding_prices = tx_batch_m.funding_prices.clone();
    drop(tx_batch_m);

    let mut fundings = Vec::new();
    for token in funding_rates.keys() {
        let rates = funding_rates.get(token).unwrap();
        let prices = funding_prices.get(token).unwrap();

        let funding_info = FundingInfo {
            token: *token,
            funding_rates: rates.clone(),
            funding_prices: prices.clone(),
        };

        fundings.push(funding_info);
    }

    let reply = FundingRes {
        successful: true,
        fundings,
        error_message: "".to_string(),
    };

    return Ok(Response::new(reply));
}
