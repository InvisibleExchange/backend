use parking_lot::Mutex;
use serde_json::json;
use std::println;
use std::thread::{self};
use std::time::Duration;
use std::{collections::HashMap, sync::Arc};
use tokio_tungstenite::tungstenite::Message;

use crate::matching_engine::orderbook::OrderBook;
use crate::perpetual::IMPACT_NOTIONAL_PER_ASSET;
use crate::server::grpc::{FundingUpdateMessage, GrpcMessage, GrpcTxResponse, MessageType};
use crate::server::server_helpers::broadcast_message;
use crate::trees::superficial_tree::SuperficialTree;
use crate::utils::firestore::{create_session, retry_failed_updates};
use crate::utils::storage::BackupStorage;

use firestore_db_and_auth::ServiceSession;

use tokio::sync::{
    mpsc::Sender as MpscSender,
    oneshot::{self, Sender as OneshotSender},
    Mutex as TokioMutex,
};
use tokio::task::JoinHandle as TokioJoinHandle;
use tokio::time;

use super::WsConnectionsMap;

pub async fn start_periodic_updates(
    order_books: &HashMap<u16, Arc<TokioMutex<OrderBook>>>,
    perp_order_books: &HashMap<u16, Arc<TokioMutex<OrderBook>>>,
    mpsc_tx: &MpscSender<(GrpcMessage, OneshotSender<GrpcTxResponse>)>,
    session: &Arc<Mutex<ServiceSession>>,
    ws_connections: &Arc<TokioMutex<WsConnectionsMap>>,
    privileged_ws_connections: &Arc<TokioMutex<Vec<u64>>>,
    backup_storage: &Arc<Mutex<BackupStorage>>,
    state_tree: &Arc<Mutex<SuperficialTree>>,
    perp_state_tree: &Arc<Mutex<SuperficialTree>>,
) {
    let perp_order_books_ = perp_order_books.clone();
    let mpsc_tx = mpsc_tx.clone();

    // * UPDATE FUNDING RATES EVERY 60 SECONDS
    let mut interval = time::interval(time::Duration::from_secs(60));
    tokio::spawn(async move {
        'outer: loop {
            interval.tick().await;

            let mut impact_prices: HashMap<u64, (u64, u64)> = HashMap::new();
            for (_, b) in perp_order_books_.iter() {
                let book = b.lock().await;

                let impact_notional: u64 = *IMPACT_NOTIONAL_PER_ASSET
                    .get(book.order_asset.to_string().as_str())
                    .unwrap();

                let res = book.get_impact_prices(impact_notional);
                if let Err(_e) = res {
                    continue 'outer;
                }

                let (impact_bid_price, impact_ask_price) = res.unwrap();

                impact_prices.insert(book.order_asset, (impact_ask_price, impact_bid_price));
            }

            let transaction_mpsc_tx = mpsc_tx.clone();

            let handle: TokioJoinHandle<GrpcTxResponse> = tokio::spawn(async move {
                let (resp_tx, resp_rx) = oneshot::channel();

                let mut grpc_message = GrpcMessage::new();
                grpc_message.msg_type = MessageType::FundingUpdate;
                grpc_message.funding_update_message = Some(FundingUpdateMessage { impact_prices });

                transaction_mpsc_tx
                    .send((grpc_message, resp_tx))
                    .await
                    .ok()
                    .unwrap();

                return resp_rx.await.unwrap();
            });

            if let Ok(grpc_res) = handle.await {
                if !grpc_res.successful {
                    println!("Failed applying funding update\n");
                }
            } else {
                println!("Failed applying funding update\n");
            }
        }
    });

    //  *CHECK FOR FAILED DB UPDATES EVERY 5 MINUTES
    let mut interval = time::interval(time::Duration::from_secs(300));
    let session_ = session.clone();
    let backup_storage = backup_storage.clone();
    let state_tree = state_tree.clone();
    let perp_state_tree = perp_state_tree.clone();
    tokio::spawn(async move {
        loop {
            interval.tick().await;
            if let Err(_e) =
                retry_failed_updates(&state_tree, &perp_state_tree, &session_, &backup_storage)
            {
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

    // * CREATE NEW FIREBASE SESSION EVERY 45 MINUTES
    std::thread::spawn(move || loop {
        thread::sleep(Duration::from_secs(1800));

        let new_session = create_session();
        let mut sess = session_.lock();
        *sess = new_session;

        drop(sess);
    });

    // * SEND LIQUIDITY UPDATE EVERY SECOND
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
}

//

//

//

// fn update_and_compare(
//     prev_bid_queue: &mut HashMap<usize, (f64, u64)>,
//     prev_ask_queue: &mut HashMap<usize, (f64, u64)>,
//     bid_queue: Vec<(f64, u64, u64, u64)>,
//     ask_queue: Vec<(f64, u64, u64, u64)>,
// ) -> (Vec<(usize, (f64, u64, u64))>, Vec<(usize, (f64, u64, u64))>) {
//     let mut bid_diffs: Vec<(usize, (f64, u64, u64))> = Vec::new();

//     bid_queue
//         .iter()
//         .enumerate()
//         .for_each(|(i, bid)| match prev_bid_queue.get(&i) {
//             Some(prev_bid) if prev_bid.0 == bid.0 && prev_bid.1 == bid.1 => {}
//             _ => {
//                 bid_diffs.push((i, (bid.0, bid.1, bid.2)));
//                 prev_bid_queue.insert(i, (bid.0, bid.1));
//             }
//         });

//     let mut ask_diffs: Vec<(usize, (f64, u64, u64))> = Vec::new();
//     ask_queue
//         .iter()
//         .enumerate()
//         .for_each(|(i, ask)| match prev_ask_queue.get(&i) {
//             Some(prev_ask) if prev_ask.0 == ask.0 && prev_ask.1 == ask.1 => {}
//             _ => {
//                 ask_diffs.push((i, (ask.0, ask.1, ask.2)));
//                 prev_ask_queue.insert(i, (ask.0, ask.1));
//             }
//         });

//     (bid_diffs, ask_diffs)
// }
