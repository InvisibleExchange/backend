use serde_json::json;
use std::thread;
use std::time::Duration;
use std::{collections::HashMap, sync::Arc};
use tokio_tungstenite::tungstenite::Message;

use crate::matching_engine::orderbook::OrderBook;
use crate::perpetual::IMPACT_NOTIONAL_PER_ASSET;
use crate::server::grpc::FundingUpdateMessage;
use crate::server::server_helpers::broadcast_message;
use crate::transaction_batch::TransactionBatch;
use crate::utils::storage::firestore::{create_session, retry_failed_updates};

use tokio::sync::Mutex as TokioMutex;
use tokio::time;

use super::WsConnectionsMap;

pub async fn start_periodic_updates(
    tx_batch: &Arc<TokioMutex<TransactionBatch>>,
    order_books: &HashMap<u16, Arc<TokioMutex<OrderBook>>>,
    perp_order_books: &HashMap<u16, Arc<TokioMutex<OrderBook>>>,
    ws_connections: &Arc<TokioMutex<WsConnectionsMap>>,
    privileged_ws_connections: &Arc<TokioMutex<Vec<u64>>>,
) {
    let perp_order_books_ = perp_order_books.clone();

    let tx_batch_m = tx_batch.lock().await;
    let session = Arc::clone(&tx_batch_m.firebase_session);
    let backup_storage = Arc::clone(&tx_batch_m.backup_storage);
    let state_tree = Arc::clone(&tx_batch_m.state_tree);
    let storage_m = Arc::clone(&tx_batch_m.main_storage);
    drop(tx_batch_m);

    // * UPDATE FUNDING RATES EVERY 60 SECONDS
    let tx_batch_c = Arc::clone(&tx_batch);
    let mut interval = time::interval(time::Duration::from_secs(60));
    tokio::spawn(async move {
        // ? Skip the first tick
        interval.tick().await;

        'outer: loop {
            interval.tick().await;

            let mut impact_prices: HashMap<u32, (u64, u64)> = HashMap::new();
            for (_, b) in perp_order_books_.iter() {
                let book = b.lock().await;

                let impact_notional: u64 = *IMPACT_NOTIONAL_PER_ASSET
                    .get(book.order_asset.to_string().as_str())
                    .unwrap();

                let res = book.get_impact_prices(impact_notional);
                if let Err(_e) = res {
                    continue;
                }

                let (impact_bid_price, impact_ask_price) = res.unwrap();

                impact_prices.insert(book.order_asset, (impact_ask_price, impact_bid_price));
            }

            if impact_prices.is_empty() {
                continue 'outer;
            }

            let mut tx_batch_m = tx_batch_c.lock().await;
            let funding_update_msg = FundingUpdateMessage { impact_prices };
            tx_batch_m.per_minute_funding_updates(funding_update_msg);
            drop(tx_batch_m);
        }
    });

    //  *CHECK FOR FAILED DB UPDATES EVERY 2 MINUTES
    let mut interval = time::interval(time::Duration::from_secs(120));
    let session_ = session.clone();
    let backup_storage = backup_storage.clone();
    let state_tree = state_tree.clone();
    tokio::spawn(async move {
        loop {
            interval.tick().await;
            if let Err(_e) = retry_failed_updates(&state_tree, &session_, &backup_storage) {
                println!("Failed retrying failed database updates");
            };
        }
    });

    // * CLEAR EXPIRED ORDERS EVERY 3 SECONDS
    let order_books_ = order_books.clone();
    let perp_order_books_ = perp_order_books.clone();
    let session_ = session.clone();

    let mut interval2 = time::interval(time::Duration::from_secs(3));

    tokio::spawn(async move {
        loop {
            interval2.tick().await;

            for book in order_books_.values() {
                book.lock().await.clear_expired_orders();
            }

            for book in perp_order_books_.values() {
                book.lock().await.clear_expired_orders();
            }
        }
    });

    // * CREATE NEW FIREBASE SESSION EVERY 30 MINUTES
    std::thread::spawn(move || loop {
        thread::sleep(Duration::from_secs(1800));

        let new_session = create_session();
        let mut sess = session_.lock();
        *sess = new_session;

        drop(sess);
    });

    // * SEND LIQUIDITY UPDATE 300ms
    let order_books_ = order_books.clone();
    let perp_order_books_ = perp_order_books.clone();
    let ws_connections_ = ws_connections.clone();
    let privileged_ws_connections_ = privileged_ws_connections.clone();

    let mut interval3 = time::interval(time::Duration::from_millis(300));
    tokio::spawn(async move {
        loop {
            interval3.tick().await;

            let mut liquidity = Vec::new();

            for book in order_books_.values() {
                // ? Get the updated orderbook liquidity
                let order_book = book.lock().await;
                let market_id = order_book.market_id;
                let ask_queue = order_book.ask_queue.visualize();
                let bid_queue = order_book.bid_queue.visualize();
                drop(order_book);

                let update_msg = json!({
                    "type": "spot",
                    "market": market_id.to_string(),
                    "ask_liquidity": ask_queue,
                    "bid_liquidity": bid_queue
                });

                liquidity.push(update_msg)
            }

            for book in perp_order_books_.values() {
                // ? Get the updated orderbook liquidity
                let order_book = book.lock().await;
                let market_id = order_book.market_id;
                let ask_queue = order_book.ask_queue.visualize();
                let bid_queue = order_book.bid_queue.visualize();
                drop(order_book);

                let update_msg = json!({
                    "type": "perpetual",
                    "market": market_id.to_string(),
                    "ask_liquidity": ask_queue,
                    "bid_liquidity": bid_queue
                });

                liquidity.push(update_msg);
            }

            let json_msg = json!({
                "message_id": "LIQUIDITY_UPDATE",
                "liquidity": liquidity
            });
            let msg = Message::Text(json_msg.to_string());

            // ? Send the updated liquidity to anyone who's listening
            if let Err(_) =
                broadcast_message(&ws_connections_, &privileged_ws_connections_, msg).await
            {
                println!("Error sending liquidity update message")
            };
        }
    });

    // * STORE PENDING TXS EVERY 10 MINUTES
    let mut interval4 = time::interval(time::Duration::from_secs(600));
    tokio::spawn(async move {
        // ? Skip the first tick
        interval4.tick().await;

        loop {
            interval4.tick().await;

            let mut strg = storage_m.lock();
            let future = strg.process_pending_batch_updates(false);
            drop(strg);

            let _h = tokio::spawn(async move {
                match future {
                    None => {
                        return;
                    }
                    Some(future) => {
                        if let Err(e) = future.await {
                            println!("Error storing pending txs: {:?}", e);
                        }
                    }
                }
            });
        }
    });
}

//

//

//
