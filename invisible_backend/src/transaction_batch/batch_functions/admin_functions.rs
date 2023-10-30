use parking_lot::Mutex;
use std::{collections::HashMap, sync::Arc};

use error_stack::Result;

use crate::transaction_batch::tx_batch_helpers::{
    _calculate_funding_rates, _per_minute_funding_update_inner,
};
use crate::trees::superficial_tree::SuperficialTree;
use crate::utils::storage::local_storage::MainStorage;

use crate::utils::errors::OracleUpdateError;

use crate::server::grpc::FundingUpdateMessage;

use crate::transaction_batch::{
    tx_batch_helpers::_init_empty_tokens_map, tx_batch_structs::OracleUpdate,
};

pub fn _init_inner(
    main_storage: &Arc<Mutex<MainStorage>>,
    funding_rates: &mut HashMap<u32, Vec<i64>>,
    funding_prices: &mut HashMap<u32, Vec<u64>>,
    min_funding_idxs: &mut Arc<Mutex<HashMap<u32, u32>>>,
    latest_index_price: &mut HashMap<u32, u64>,
    min_index_price_data: &mut HashMap<u32, (u64, OracleUpdate)>,
    max_index_price_data: &mut HashMap<u32, (u64, OracleUpdate)>,
    state_tree: &mut Arc<Mutex<SuperficialTree>>,
) {
    let storage = main_storage.lock();
    if !storage.funding_db.is_empty() {
        if let Ok((funding_rates_, funding_prices_, funding_idxs, min_funding_idxs_)) =
            storage.read_funding_info()
        {
            *funding_rates = funding_rates_;
            *funding_prices = funding_prices_;
            *min_funding_idxs = Arc::new(Mutex::new(min_funding_idxs_));
        } else {
            panic!("Error reading funding info from storage");
        }
    }

    if !storage.price_db.is_empty() {
        if let Some((latest_index_price_, min_index_price_data_, max_index_price_data_)) =
            storage.read_price_data()
        {
            *latest_index_price = latest_index_price_;
            *min_index_price_data = min_index_price_data_;
            *max_index_price_data = max_index_price_data_;
        }
    }

    let state_tree_ = match SuperficialTree::from_disk() {
        Ok(tree) => tree,
        Err(_) => SuperficialTree::new(32),
    };
    *state_tree = Arc::new(Mutex::new(state_tree_));
}

pub fn _per_minute_funding_updates(
    running_funding_tick_sums: &mut HashMap<u32, i64>,
    latest_index_price: &mut HashMap<u32, u64>,
    current_funding_count: &mut u16,
    funding_rates: &mut HashMap<u32, Vec<i64>>,
    funding_prices: &mut HashMap<u32, Vec<u64>>,
    min_funding_idxs: &Arc<Mutex<HashMap<u32, u32>>>,
    main_storage: &Arc<Mutex<MainStorage>>,
    funding_update: FundingUpdateMessage,
) {
    let mut running_sums: Vec<(u32, i64)> = Vec::new();
    for tup in running_funding_tick_sums.drain() {
        running_sums.push(tup);
    }

    for (token, sum) in running_sums {
        let index_price = latest_index_price.get(&token).unwrap().clone();

        if !funding_update.impact_prices.contains_key(&token) {
            continue;
        };
        let (impact_bid, impact_ask) = funding_update.impact_prices.get(&token).unwrap();
        let new_sum = _per_minute_funding_update_inner(*impact_bid, *impact_ask, sum, index_price);

        running_funding_tick_sums.insert(token, new_sum);
    }

    *current_funding_count += 1;

    if *current_funding_count == 480 {
        // Do we want 1 or 8 hours
        let fundings = _calculate_funding_rates(running_funding_tick_sums);

        for (token, funding) in fundings.iter() {
            funding_rates.get_mut(token).unwrap().push(*funding);
            let price = latest_index_price.get(token).unwrap().clone();
            funding_prices.get_mut(token).unwrap().push(price);
        }

        // Reinitialize the funding tick sums
        *current_funding_count = 0;
        _init_empty_tokens_map::<i64>(&mut *running_funding_tick_sums);

        let storage = main_storage.lock();
        storage.store_funding_info(&funding_rates, &funding_prices, &min_funding_idxs.lock());
        drop(storage);
    }
}

pub fn _update_index_prices_inner(
    latest_index_price: &mut HashMap<u32, u64>,
    min_index_price_data: &mut HashMap<u32, (u64, OracleUpdate)>,
    max_index_price_data: &mut HashMap<u32, (u64, OracleUpdate)>,
    running_index_price_count: &mut u16,
    main_storage: &Arc<Mutex<MainStorage>>,
    oracle_updates: Vec<OracleUpdate>,
) -> Result<(), OracleUpdateError> {
    // Oracle prices received from the oracle provider (e.g. Chainlink, Pontis, Stork)

    // Todo: check signatures only if the price is more/less then the max/min price this batch
    // Todo: Should also check signatures (at least a few) if the price deviates from the previous price by more than some threshold

    // TODO: VERIFY TIMESTAMP OF ORACLE UPDATE !!!!!!!!!!!!!!!!!!!!!!!!!!!

    for mut update in oracle_updates {
        let token = update.token;
        let mut median = update.median_price();

        if min_index_price_data.get(&update.token).unwrap().0 == 0 {
            update.verify_update()?;
            median = update.median_price();

            latest_index_price.insert(token, median);

            min_index_price_data.insert(update.token, (median, update.clone()));

            if max_index_price_data.get(&token).unwrap().0 == 0 {
                max_index_price_data.insert(token, (median, update));
            }
        } else if median < min_index_price_data.get(&update.token).unwrap().0 {
            // ? This disregards the invalid observations and just uses the valid ones to get the median
            update.verify_update()?;
            median = update.median_price();

            if median >= min_index_price_data.get(&update.token).unwrap().0 {
                latest_index_price.insert(token, median);
                continue;
            }

            min_index_price_data.insert(update.token, (median, update));

            //
        } else if median > max_index_price_data.get(&update.token).unwrap().0 {
            update.verify_update()?;
            median = update.median_price();

            if median <= max_index_price_data.get(&update.token).unwrap().0 {
                latest_index_price.insert(token, median);
                continue;
            }

            max_index_price_data.insert(update.token, (median, update));
        }

        latest_index_price.insert(token, median);
    }

    *running_index_price_count += 1;

    if *running_index_price_count == 10 {
        let main_storage = main_storage.lock();
        main_storage.store_price_data(
            &latest_index_price,
            &min_index_price_data,
            &max_index_price_data,
        );
        drop(main_storage);
    }

    Ok(())
}
