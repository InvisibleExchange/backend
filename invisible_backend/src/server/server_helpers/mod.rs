use std::cmp::min;
use std::{collections::HashMap, sync::Arc};

use futures::stream::SplitSink;
use futures::{SinkExt, StreamExt};
use num_traits::Pow;
use phf::phf_map;
use serde_json::{from_str, Value};
use tokio::net::TcpStream;
use tokio::sync::Mutex as TokioMutex;

use error_stack::{Report, Result};
use tokio_tungstenite::WebSocketStream;

use crate::matching_engine::get_quote_qty;
use crate::perpetual::perp_order::PerpOrder;
use crate::perpetual::perp_swap::PerpSwap;
use crate::perpetual::{COLLATERAL_TOKEN, COLLATERAL_TOKEN_DECIMALS, DECIMALS_PER_ASSET};
use crate::utils::crypto_utils::Signature;
use crate::{
    matching_engine::{
        domain::{Order, OrderSide as OBOrderSide},
        orderbook::{Failed, OrderBook, Success},
    },
    transactions::{limit_order::LimitOrder, swap::Swap},
    utils::errors::{send_matching_error, MatchingEngineError},
};

use tokio_tungstenite::tungstenite::{Message, Result as WsResult};

const BTC: u32 = 3592681469;
const ETH: u32 = 453755560;
const USDC: u32 = 2413654107;
const SOL: u32 = 277158171;

pub static SPOT_MARKET_IDS: phf::Map<&'static str, u16> = phf_map! {
    "3592681469" => 11, // BTC
 "453755560" => 12, // ETH
};

pub static PERP_MARKET_IDS: phf::Map<&'static str, u16> = phf_map! {
    "3592681469" => 21, // BTC
    "453755560" => 22, // ETH
    "277158171" => 23, // SOL
};

pub mod amend_order_execution;
pub mod engine_helpers;
pub mod periodic_updates;
pub mod perp_swap_execution;
pub mod swap_execution;

pub fn init_order_books() -> (
    HashMap<u16, Arc<TokioMutex<OrderBook>>>,
    HashMap<u16, Arc<TokioMutex<OrderBook>>>,
) {
    let mut spot_order_books: HashMap<u16, Arc<TokioMutex<OrderBook>>> = HashMap::new();
    let mut perp_order_books: HashMap<u16, Arc<TokioMutex<OrderBook>>> = HashMap::new();

    // & BTC-USDC orderbook
    let market_id = SPOT_MARKET_IDS.get(&BTC.to_string()).unwrap();
    let book = Arc::new(TokioMutex::new(OrderBook::new(BTC, USDC, *market_id)));
    spot_order_books.insert(*market_id, book);

    let market_id = PERP_MARKET_IDS.get(&BTC.to_string()).unwrap();
    let book = Arc::new(TokioMutex::new(OrderBook::new(BTC, USDC, *market_id)));
    perp_order_books.insert(*market_id, book);

    // & ETH-USDC orderbook
    let market_id = SPOT_MARKET_IDS.get(&ETH.to_string()).unwrap();
    let book = Arc::new(TokioMutex::new(OrderBook::new(ETH, USDC, *market_id)));
    spot_order_books.insert(*market_id, book);

    let market_id = PERP_MARKET_IDS.get(&ETH.to_string()).unwrap();
    let book = Arc::new(TokioMutex::new(OrderBook::new(ETH, USDC, *market_id)));
    perp_order_books.insert(*market_id, book);

    // & SOL-USDC orderbook
    let market_id = PERP_MARKET_IDS.get(&SOL.to_string()).unwrap();
    let book = Arc::new(TokioMutex::new(OrderBook::new(SOL, USDC, *market_id)));
    perp_order_books.insert(*market_id, book);

    return (spot_order_books, perp_order_books);
}

pub fn get_market_id_and_order_side(
    token_spent: u32,
    token_received: u32,
) -> Option<(u16, OBOrderSide)> {
    let option1 = SPOT_MARKET_IDS.get(&token_spent.to_string());

    if let Some(m_id) = option1 {
        return Some((*m_id, OBOrderSide::Ask));
    }

    let option2 = SPOT_MARKET_IDS.get(&token_received.to_string());

    if let Some(m_id) = option2 {
        return Some((*m_id, OBOrderSide::Bid));
    }

    None
}

pub fn get_order_side(
    order_book: &OrderBook,
    token_spent: u32,
    token_received: u32,
) -> Option<OBOrderSide> {
    if order_book.order_asset == token_spent && order_book.price_asset == token_received {
        return Some(OBOrderSide::Ask);
    } else if order_book.order_asset == token_received && order_book.price_asset == token_spent {
        return Some(OBOrderSide::Bid);
    }

    None
}

// * ======================= ==================== ===================== =========================== ====================================

pub struct MatchingProcessedResult {
    pub swaps: Option<Vec<(Swap, u64, u64)>>, // An array of swaps that were processed by the order
    pub new_order_id: u64,                    // The order id of the order that was just processed
}

pub fn proccess_spot_matching_result(
    results_vec: &mut Vec<std::result::Result<Success, Failed>>,
) -> Result<MatchingProcessedResult, MatchingEngineError> {
    if results_vec.len() == 0 {
        return Err(send_matching_error(
            "Invalid or duplicate order".to_string(),
        ));
    } else if results_vec.len() == 1 {
        match &results_vec[0] {
            Ok(x) => match x {
                Success::Accepted {
                    id,
                    order_type: _,
                    ts: _,
                } => {
                    return Ok(MatchingProcessedResult {
                        swaps: None,
                        new_order_id: *id,
                    });
                }
                Success::Cancelled { id: _, ts: _ } => {
                    return Ok(MatchingProcessedResult {
                        swaps: None,
                        new_order_id: 0,
                    });
                }
                Success::Amended {
                    id: _,
                    new_price: _,
                    ts: _,
                } => {
                    return Ok(MatchingProcessedResult {
                        swaps: None,
                        new_order_id: 0,
                    });
                }
                _ => return Err(send_matching_error("Invalid matching response".to_string())),
            },
            Err(e) => Err(handle_error(e)),
        }
    } else if results_vec.len() % 2 == 0 {
        for res in results_vec {
            if let Err(e) = res {
                return Err(handle_error(e));
            }
        }

        return Err(send_matching_error(
            "Invalid matching response length".to_string(),
        ));
    } else {
        //

        let mut new_order_id: u64 = 0;
        if let Ok(x) = &results_vec[0] {
            if let Success::Accepted { id, .. } = x {
                new_order_id = *id;
            }
        } else if let Err(e) = &results_vec[0] {
            return Err(handle_error(e));
        }

        let mut prices: Vec<f64> = Vec::new();
        let mut a_orders: Vec<(LimitOrder, Signature, u64, u64, bool)> = Vec::new(); // Vec<(order, sig, spent_amount, user_id, take_fee?)>
        let mut b_orders: Vec<(LimitOrder, Signature, u64, u64, bool)> = Vec::new(); // Vec<(order, sig, spent_amount, user_id, take_fee?)>

        for (i, res) in results_vec.drain(1..).enumerate() {
            if let Ok(res) = res {
                match res {
                    // ? Because fills always happen in pairs you can always set the bid order to order_a and ask order to order_b
                    Success::Filled {
                        order,
                        signature,
                        side,
                        order_type: _,
                        price,
                        qty,
                        quote_qty,
                        partially_filled: _,
                        ts: _,
                        user_id,
                    } => {
                        if let Order::Spot(lim_order) = order {
                            if side == OBOrderSide::Ask {
                                // transactions are ordered as [(taker,maker), (taker,maker), ...]
                                let is_taker = i % 2 == 0;

                                // He is selling the base asset and buying the quote(price) asset
                                let spent_amount = qty;

                                if spent_amount > lim_order.amount_spent {
                                    println!(
                                        "ask spent_amount: {}, > lim_order.amount_spent: {}",
                                        spent_amount, lim_order.amount_spent
                                    )
                                }

                                let spent_amount = min(spent_amount, lim_order.amount_spent);

                                let b_order_tup =
                                    (lim_order, signature, spent_amount, user_id, is_taker);
                                b_orders.push(b_order_tup);
                                prices.push(price);
                            } else {
                                // transactions are ordered as [(taker,maker), (taker,maker), ...]
                                let is_taker = i % 2 == 0;

                                // He is buying the base asset and selling the quote(price) asset
                                let spent_amount = if quote_qty > 0 {
                                    quote_qty
                                } else {
                                    get_quote_qty(
                                        qty,
                                        price,
                                        lim_order.token_received,
                                        lim_order.token_spent,
                                        None,
                                    )
                                };

                                if spent_amount > lim_order.amount_spent {
                                    println!(
                                        "bid spent_amount: {}, > lim_order.amount_spent: {}",
                                        spent_amount, lim_order.amount_spent
                                    )
                                }
                                let spent_amount = min(spent_amount, lim_order.amount_spent);

                                let a_order_tup =
                                    (lim_order, signature, spent_amount, user_id, is_taker);
                                a_orders.push(a_order_tup);
                            }
                        } else {
                            return Err(send_matching_error(
                                "Invalid order type in Filled response".to_string(),
                            ));
                        }
                    }
                    _ => return Err(send_matching_error("SOMETHING WENT WRONG".to_string())),
                };
            } else if let Err(e) = res {
                return Err(handle_error(&e));
            }
        }

        let mut swaps: Vec<(Swap, u64, u64)> = Vec::new(); // Vec<(swap, user_id_a, user_id_b)>

        // ? Build swaps from a_orders and b_orders vecs
        for ((a, b), price) in a_orders.into_iter().zip(b_orders).zip(prices) {
            let (order_a, signature_a, spent_amount_a, user_id_a, take_fee_a) = a;
            let (order_b, signature_b, spent_amount_b, user_id_b, take_fee_b) = b;

            let quote_decimals = DECIMALS_PER_ASSET[&order_a.token_spent.to_string()];
            let base_decimals = DECIMALS_PER_ASSET[&order_a.token_received.to_string()];

            // a is bid - spent = quote
            let spent_b_: f64 =
                (spent_amount_a as f64 / price as f64 / 10_f64.pow(quote_decimals as i32)).ceil();
            let spent_amount_b = min(
                spent_amount_b,
                (spent_b_ * 10_f64.pow(base_decimals)) as u64,
            );

            // b is ask - spent = base
            let spent_a_: f64 =
                (spent_amount_b as f64 * price as f64 / 10_f64.pow(base_decimals as i32)).ceil();
            let spent_amount_a = min(
                spent_amount_a,
                (spent_a_ * 10_f64.pow(quote_decimals)) as u64,
            );

            let fee_taken_a = if take_fee_a {
                (spent_amount_b as f64 * 0.0005) as u64
            } else {
                0
            };
            let fee_taken_b = if take_fee_b {
                (spent_amount_a as f64 * 0.0005) as u64
            } else {
                0
            };

            let swap = Swap::new(
                order_a,
                order_b,
                signature_a,
                signature_b,
                spent_amount_a,
                spent_amount_b,
                fee_taken_a,
                fee_taken_b,
            );

            swaps.push((swap, user_id_a, user_id_b));
        }

        return Ok(MatchingProcessedResult {
            swaps: Some(swaps),
            new_order_id,
        });
    }
}

// ======================== ======================== =======================

pub struct PerpMatchingProcessedResult {
    pub perp_swaps: Option<Vec<(PerpSwap, u64, u64)>>, // An array of swaps that were processed by the order
    pub new_order_id: u64, // The order id of the order that was just processed
}

pub fn proccess_perp_matching_result(
    results_vec: &mut Vec<std::result::Result<Success, Failed>>,
) -> Result<PerpMatchingProcessedResult, MatchingEngineError> {
    if results_vec.len() == 0 {
        return Err(send_matching_error(
            "Invalid matching response length".to_string(),
        ));
    } else if results_vec.len() == 1 {
        match &results_vec[0] {
            Ok(x) => match x {
                Success::Accepted { id, .. } => {
                    return Ok(PerpMatchingProcessedResult {
                        perp_swaps: None,
                        new_order_id: *id,
                    });
                }
                Success::Cancelled { .. } => {
                    return Ok(PerpMatchingProcessedResult {
                        perp_swaps: None,
                        new_order_id: 0,
                    });
                }
                Success::Amended { .. } => {
                    return Ok(PerpMatchingProcessedResult {
                        perp_swaps: None,
                        new_order_id: 0,
                    });
                }
                _ => return Err(send_matching_error("Invalid matching response".to_string())),
            },
            Err(e) => Err(handle_error(e)),
        }
    } else if results_vec.len() % 2 == 0 {
        for res in results_vec {
            if let Err(e) = res {
                return Err(handle_error(e));
            }
        }

        return Err(send_matching_error(
            "Invalid matching response length".to_string(),
        ));
    } else {
        //

        let mut new_order_id: u64 = 0;
        if let Ok(x) = &results_vec[0] {
            if let Success::Accepted { id, .. } = x {
                new_order_id = *id;
            }
        } else if let Err(e) = &results_vec[0] {
            return Err(handle_error(e));
        }

        let mut prices: Vec<f64> = Vec::new();
        let mut a_orders: Vec<(PerpOrder, Signature, u64, u64, bool)> = Vec::new(); // Vec<(order, sig, spent_synthetic, user_id, take_fee?)>
        let mut b_orders: Vec<(PerpOrder, Signature, u64, u64, bool)> = Vec::new(); // Vec<(order, sig, spent_collateral, user_id, take_fee?)>

        for (i, res) in results_vec.drain(1..).enumerate() {
            if let Ok(res) = res {
                match res {
                    // ? Because fills always happen in pairs you can always set the bid order to order_a and ask order to order_b
                    Success::Filled {
                        order,
                        signature,
                        side,
                        order_type: _,
                        price,
                        qty,
                        quote_qty,
                        partially_filled: _,
                        ts: _,
                        user_id,
                    } => {
                        if let Order::Perp(perp_order) = order {
                            if side == OBOrderSide::Ask {
                                // The synthetic exchnaged in the swap
                                let spent_synthetic = qty;

                                if spent_synthetic > perp_order.synthetic_amount {
                                    println!(
                                        "spent_synthetic: {}, > perp_order.synthetic_amount: {}",
                                        spent_synthetic, perp_order.synthetic_amount
                                    )
                                }

                                let spent_synthetic =
                                    min(spent_synthetic, perp_order.synthetic_amount);

                                // transactions are ordered as [(taker,maker), (taker,maker), ...]
                                let take_fee = i % 2 == 0;

                                let b_order_tup =
                                    (perp_order, signature, spent_synthetic, user_id, take_fee);
                                b_orders.push(b_order_tup);
                                prices.push(price);
                            } else {
                                // The collateral exchnaged in the swap
                                let collateral_spent = if quote_qty > 0 {
                                    quote_qty
                                } else {
                                    get_quote_qty(
                                        qty,
                                        price,
                                        perp_order.synthetic_token,
                                        COLLATERAL_TOKEN,
                                        None,
                                    )
                                };

                                // if collateral_spent > perp_order.collateral_amount {
                                //     println!(
                                //         "collateral_spent: {}, < perp_order.collateral_amount: {}",
                                //         collateral_spent, perp_order.collateral_amount
                                //     )
                                // }
                                // let collateral_spent =
                                //     min(collateral_spent, perp_order.collateral_amount);

                                // transactions are ordered as [(taker,maker), (taker,maker), ...]
                                let take_fee = i % 2 == 0;

                                let a_order_tup =
                                    (perp_order, signature, collateral_spent, user_id, take_fee);
                                a_orders.push(a_order_tup);
                            }
                        } else {
                            return Err(send_matching_error(
                                "Invalid order type in Filled response".to_string(),
                            ));
                        }
                    }
                    _ => {
                        return Err(send_matching_error("SOMETHING WENT WRONG".to_string()));
                    }
                };
            } else if let Err(e) = res {
                return Err(handle_error(&e));
            }
        }

        let mut swaps: Vec<(PerpSwap, u64, u64)> = Vec::new(); // Vec<(swap, user_id_a, user_id_b)>

        // ? Build swaps from a_orders and b_orders vecs
        for ((a, b), price) in a_orders.into_iter().zip(b_orders).zip(prices) {
            let (order_a, signature_a, spent_collateral, user_id_a, take_fee_a) = a;
            let (order_b, signature_b, spent_synthetic, user_id_b, take_fee_b) = b;

            let synthetic_decimals: u8 = DECIMALS_PER_ASSET[&order_a.synthetic_token.to_string()];

            let synthetic_: f64 = (spent_collateral as f64
                / price as f64
                / 10_f64.pow(COLLATERAL_TOKEN_DECIMALS as i32))
            .ceil();
            let spent_synthetic = min(
                spent_synthetic,
                (synthetic_ * 10_f64.pow(synthetic_decimals)) as u64,
            );

            let collateral_ =
                spent_synthetic as f64 * price as f64 / 10_f64.pow(synthetic_decimals);
            let spent_collateral = min(
                spent_collateral,
                (collateral_ * 10_f64.pow(COLLATERAL_TOKEN_DECIMALS as i32)) as u64,
            );

            let fee_taken_a = if take_fee_a {
                (spent_collateral as f64 * 0.0005) as u64
            } else {
                0
            };
            let fee_taken_b = if take_fee_b {
                (spent_collateral as f64 * 0.0005) as u64
            } else {
                0
            };

            let swap = PerpSwap::new(
                order_a,
                order_b,
                Some(signature_a),
                Some(signature_b),
                spent_collateral,
                spent_synthetic,
                fee_taken_a,
                fee_taken_b,
            );

            swaps.push((swap, user_id_a, user_id_b));
        }

        return Ok(PerpMatchingProcessedResult {
            perp_swaps: Some(swaps),
            new_order_id,
        });
    }
}

fn handle_error(e: &Failed) -> Report<MatchingEngineError> {
    match e {
        Failed::ValidationFailed(e) => {
            return send_matching_error(format!("ValidationFailed: {:#?}", e))
        }
        Failed::DuplicateOrderID(e) => {
            return send_matching_error(format!("DuplicateOrderID: {:#?}", e))
        }
        Failed::NoMatch(e) => return send_matching_error(format!("NoMatch: {:#?}", e)),
        Failed::OrderNotFound(e) => return send_matching_error(format!("OrderNotFound: {:#?}", e)),
        Failed::TooMuchSlippage(e) => {
            return send_matching_error(format!("TooMuchSlippage: {:#?}", e))
        }
    }
}

// * ======================= ==================== ===================== =========================== ====================================

pub type WsConnectionsMap = HashMap<u64, SplitSink<WebSocketStream<TcpStream>, Message>>;

const RELAY_SERVER_ID: u64 = 43147634234;
const CONFIG_CODE: u64 = 1234567890;

pub async fn handle_connection(
    raw_stream: TcpStream,
    ws_connections: Arc<TokioMutex<WsConnectionsMap>>,
    privileged_ws_connections: Arc<TokioMutex<Vec<u64>>>,
) -> WsResult<()> {
    let ws_stream = tokio_tungstenite::accept_async(raw_stream).await?;

    let (ws_sender, mut ws_receiver) = ws_stream.split();

    let msg = ws_receiver.next().await;

    let mut user_id: u64 = 0;
    let mut config_code: u64 = 0;

    match msg {
        Some(msg) => {
            let msg: Message = msg?;
            if let Message::Text(m) = msg {
                let json: std::result::Result<Value, _> = from_str(&m);

                if let Ok(json) = json {
                    // Extract the desired fields
                    user_id = u64::from_str_radix(json["user_id"].as_str().unwrap_or("0"), 10)
                        .unwrap_or_default();
                    config_code =
                        u64::from_str_radix(json["config_code"].as_str().unwrap_or("0"), 10)
                            .unwrap_or_default();

                    if user_id > 0 {
                        // ? SUBSCRIBE TO THE LIQUIDITY UPDATES
                        let mut ws_connections__ = ws_connections.lock().await;
                        ws_connections__.insert(user_id, ws_sender);
                        drop(ws_connections__);

                        if config_code == CONFIG_CODE {
                            // ? SUBSCRIBE TO THE TRADE UPDATES
                            let mut privileged_ws_connections__ =
                                privileged_ws_connections.lock().await;
                            privileged_ws_connections__.push(user_id);
                            drop(privileged_ws_connections__);
                        }
                    }
                }
            }
        }
        None => {
            // println!("Failed to establish connection");
        }
    }

    loop {
        let msg = ws_receiver.next().await;
        match msg {
            Some(_msg) => {
                // let msg: Message = msg?;
            }
            None => break,
        }
    }

    let mut ws_connections__ = ws_connections.lock().await;
    ws_connections__.remove(&user_id);
    drop(ws_connections__);

    if config_code == CONFIG_CODE {
        let mut privileged_ws_connections__ = privileged_ws_connections.lock().await;
        let index = privileged_ws_connections__
            .iter()
            .position(|&uid| uid == user_id)
            .unwrap_or_default();
        privileged_ws_connections__.remove(index);

        drop(privileged_ws_connections__);
    }

    Ok(())
}

pub async fn broadcast_message(
    ws_connections: &Arc<TokioMutex<WsConnectionsMap>>,
    privileged_ws_connections: &Arc<TokioMutex<Vec<u64>>>,
    msg: Message,
) -> WsResult<()> {
    for user_id in privileged_ws_connections.lock().await.iter() {
        let mut ws_connections__ = ws_connections.lock().await;
        let ws_sender = ws_connections__.get_mut(&user_id);

        if let None = ws_sender {
            continue;
        }

        ws_sender.unwrap().send(msg.clone()).await?;
    }

    Ok(())
}

pub async fn send_to_relay_server(
    ws_connections: &Arc<TokioMutex<WsConnectionsMap>>,
    msg: Message,
) -> WsResult<()> {
    let mut ws_connections__ = ws_connections.lock().await;
    let ws_sender = ws_connections__.get_mut(&RELAY_SERVER_ID);

    if let None = ws_sender {
        return Ok(());
    }

    ws_sender.unwrap().send(msg.clone()).await?;

    Ok(())
}

pub async fn send_direct_message(
    ws_connections: &Arc<TokioMutex<WsConnectionsMap>>,
    user_id: u64,
    msg: Message,
) -> WsResult<()> {
    let mut ws_connections__ = ws_connections.lock().await;

    let ws_sender = ws_connections__.get_mut(&user_id);

    if let None = ws_sender {
        return Ok(());
    }

    ws_sender.unwrap().send(msg.clone()).await?;

    drop(ws_connections__);

    Ok(())
}

// * ======================= ==================== ===================== =========================== ====================================
