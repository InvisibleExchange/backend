use error_stack::Result;

use serde::Deserialize as DeserializeTrait;
use std::{collections::HashMap, str::FromStr};

use num_bigint::BigUint;
// * SERIALIZE * //
use serde::{
    ser::{SerializeStruct, Serializer},
    Serialize,
};
use serde_json::Value;

use crate::{
    perpetual::{
        perp_position::PerpPosition, DECIMALS_PER_ASSET, DUST_AMOUNT_PER_ASSET,
        LEVERAGE_BOUNDS_PER_ASSET, LEVERAGE_DECIMALS, MIN_PARTIAL_LIQUIDATION_SIZE,
        PRICE_DECIMALS_PER_ASSET, TOKENS, VALID_COLLATERAL_TOKENS,
    },
    utils::crypto_utils::verify,
    utils::errors::{send_oracle_update_error, OracleUpdateError},
};

use crate::utils::crypto_utils::Signature;

// * ORACLE PRICE UPDATES ================================================================================

// PrivKeys: 0x1, 0x2, 0x3, 0x4
pub static OBSERVERS: [&'static str; 4] = [
    "874739451078007766457464989774322083649278607533249481151382481072868806602",
    "3324833730090626974525872402899302150520188025637965566623476530814354734325",
    "1839793652349538280924927302501143912227271479439798783640887258675143576352",
    "296568192680735721663075531306405401515803196637037431012739700151231900092",
];

/// This is received from the oracle containing the new prices and signatures to update the index price
#[derive(Clone, Default, Debug)]
pub struct OracleUpdate {
    pub token: u32,                 // Token id
    pub timestamp: u32,             // Timestamp of the update
    pub observer_ids: Vec<u32>, // indexes of observers that signed the update (for verifying against pub keys)
    pub prices: Vec<u64>,       // price observations made by the observers
    pub signatures: Vec<Signature>, // signatures of the price observations made by the observers
}

impl OracleUpdate {
    /// Verify and clean the oracle update
    ///
    /// Verifies there are enough signatures for the given message and that the signatures are valid,
    /// discards invalid observations and updates the median accordingly
    pub fn verify_update(&mut self) -> Result<(), OracleUpdateError> {
        // Todo: Verify timestamp is valid

        if !TOKENS.contains(&self.token) {
            return Err(send_oracle_update_error("token is invalid".to_string()));
        }

        // ? check observer_ids are unique
        let mut observer_ids_ = self.observer_ids.clone();
        observer_ids_.sort();
        observer_ids_.dedup();
        if observer_ids_.len() != self.observer_ids.len() {
            return Err(send_oracle_update_error(
                "observer_ids are not unique".to_string(),
            ));
        }

        let mut valid_observations_count = 0;

        let mut invalid_idxs = vec![];

        // ? Verify signatures
        for (i, signature) in self.signatures.iter().enumerate() {
            let price = self.prices[i];
            let observer_id = self.observer_ids[i];

            if observer_id >= OBSERVERS.len() as u32 {
                return Err(send_oracle_update_error("invalid observer id".to_string()));
            }

            let observer = OBSERVERS[observer_id as usize];
            let observer = BigUint::from_str(observer)
                .or_else(|e| Err(send_oracle_update_error(e.to_string())))?;

            let msg = (BigUint::from(price) * BigUint::from(2u128).pow(64)
                + BigUint::from(self.token))
                * BigUint::from(2u128).pow(64)
                + BigUint::from(self.timestamp);

            let is_valid = verify(&observer, &msg, &signature);
            if is_valid {
                valid_observations_count += 1;
            } else {
                invalid_idxs.push(i);
            }
        }

        // TODO change to N later
        const THRESHOLD: usize = 1;
        // ? Check that there are enough valid observations
        if valid_observations_count < THRESHOLD {
            return Err(send_oracle_update_error(
                "not enough valid observations".to_string(),
            ));
        }

        for idx in invalid_idxs.iter().rev() {
            self.prices.remove(*idx);
            self.signatures.remove(*idx);
            self.observer_ids.remove(*idx);
        }

        Ok(())
    }

    /// Gets the median price of the observations
    pub fn median_price(&self) -> u64 {
        // Get the median of self.prices (ignoring invalid observations, only used when not verifyfing signatures)
        let mut prices = self.prices.clone();
        prices.sort();
        let median = prices[prices.len() / 2];
        median
    }
}

impl Serialize for OracleUpdate {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut oracle_update = serializer.serialize_struct("OracleUpdate", 4)?;

        oracle_update.serialize_field("token", &self.token)?;
        oracle_update.serialize_field("timestamp", &self.timestamp)?;
        oracle_update.serialize_field("prices", &self.prices)?;
        oracle_update.serialize_field("observer_idxs", &self.observer_ids)?;
        oracle_update.serialize_field("signatures", &self.signatures)?;

        return oracle_update.end();
    }
}

// * DESERIALIZE * //
use serde::de::{Deserialize, Deserializer};

impl<'de> Deserialize<'de> for OracleUpdate {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(DeserializeTrait, Debug)]
        struct Sig {
            r: String,
            s: String,
        }

        #[derive(DeserializeTrait, Debug)]
        struct Helper {
            token: u32,
            timestamp: u32,
            observer_idxs: Vec<u32>,
            prices: Vec<u64>,
            signatures: Vec<Sig>,
        }

        let helper = Helper::deserialize(deserializer)?;

        let sigs = helper
            .signatures
            .iter()
            .map(|sig| Signature {
                r: sig.r.clone(),
                s: sig.s.clone(),
            })
            .collect::<Vec<Signature>>();

        Ok(OracleUpdate {
            timestamp: helper.timestamp,
            token: helper.token,
            prices: helper.prices,
            observer_ids: helper.observer_idxs,
            signatures: sigs,
        })
    }
}

// * FUNDING ================================================================================

/// The information about the funding rates and prices for each token.\
/// This is constructed after each transaction batch finalization and
/// fed as input to the cairo program
#[derive(Debug, Clone, Serialize)]
pub struct FundingInfo {
    /// Funding_rates structure is as follows: \
    ///  \[0] = token id, \[1] = min_funding_idx, \[2] = token funding_rates len (n-3), \[3..n] = funding_rates \
    ///  \[n] = token id, \[n+1] = min_funding_idx,  \[n+2] = token funding_rates len (m-3), \[n+3..n+m] \
    ///  \[n+m] = token id, \[n+m+1] = min_funding_idx, \[n+m+2] = token funding_rates len (o), \[n+m+3..n+m+o] ...
    pub funding_rates: Vec<i64>, // funding rates for each token

    /// similar structure as funding_rates:
    ///
    /// \[0] = token id, \[1..n] = prices ...
    pub funding_prices: Vec<u64>, // funding prices for each token
}

impl FundingInfo {
    pub fn new(
        __funding_rates__: &HashMap<u32, Vec<i64>>,
        __funding_prices__: &HashMap<u32, Vec<u64>>,
        min_funding_idxs: &HashMap<u32, u32>,
    ) -> FundingInfo {
        let mut funding_rates: Vec<i64> = Vec::new();
        let mut funding_prices: Vec<u64> = Vec::new();

        for (token, rates) in __funding_rates__.iter() {
            // ? Get the relevant rates and prices for this batch from min_funding_idx forward
            let relevant_batch_frates = rates[min_funding_idxs[token] as usize..].to_vec();

            funding_rates.push(*token as i64);
            funding_rates.push(*min_funding_idxs.get(token).unwrap() as i64);
            funding_rates.push(relevant_batch_frates.len() as i64);
            for rate in relevant_batch_frates {
                funding_rates.push(rate);
            }

            let prices = __funding_prices__.get(token).unwrap();
            let relevant_batch_fprices = prices[min_funding_idxs[token] as usize..].to_vec();

            funding_prices.push(*token as u64);
            for price in relevant_batch_fprices {
                funding_prices.push(price);
            }
        }

        FundingInfo {
            funding_rates,
            funding_prices,
        }
    }
}

// ================= ====================== ================= ====================== =================

/// The information about the funding rates and prices relevant to the current perpetual swap being executed.
/// This is used to apply funding to a position in the swap.
#[derive(Clone)]
pub struct SwapFundingInfo {
    pub current_funding_idx: u32,      // current funding index
    pub swap_funding_rates: Vec<i64>,  // funding rates aplicable to positions in the swap
    pub swap_funding_prices: Vec<u64>, // funding prices aplicable to positions in the swap
    pub min_swap_funding_idx: u32, // min last_modified funding index of the positions for the swap
}

impl SwapFundingInfo {
    pub fn new(
        funding_rates: &HashMap<u32, Vec<i64>>,
        funding_prices: &HashMap<u32, Vec<u64>>,
        current_funding_idx: u32,
        funding_idx_shift: &HashMap<u32, u32>,
        synthetic_token: u32,
        position_a: &Option<PerpPosition>,
        position_b: &Option<PerpPosition>,
    ) -> SwapFundingInfo {
        let mut prev_funding_idx_a: Option<u32> = None;
        if let Some(position) = position_a.as_ref() {
            prev_funding_idx_a = Some(position.last_funding_idx);
        }

        let mut prev_funding_idx_b: Option<u32> = None;
        if let Some(position) = position_b.as_ref() {
            prev_funding_idx_b = Some(position.last_funding_idx);
        }

        let swap_funding_rates: Vec<i64>;
        let swap_funding_prices: Vec<u64>;
        let min_swap_funding_idx: u32;
        if prev_funding_idx_a.is_none() && prev_funding_idx_b.is_none() {
            min_swap_funding_idx = 0;

            swap_funding_rates = Vec::new();
            swap_funding_prices = Vec::new();
        } else {
            min_swap_funding_idx = std::cmp::min(
                prev_funding_idx_a.unwrap_or(u32::MAX),
                prev_funding_idx_b.unwrap_or(u32::MAX),
            );

            let shift: u32 = *funding_idx_shift.get(&synthetic_token).unwrap_or(&0);
            let arr_start_idx = min_swap_funding_idx - shift;

            if arr_start_idx == 0 {
                swap_funding_rates = funding_rates
                    .get(&synthetic_token)
                    .unwrap_or(&vec![])
                    .clone();
                swap_funding_prices = funding_prices
                    .get(&synthetic_token)
                    .unwrap_or(&vec![])
                    .clone();
            } else {
                swap_funding_rates = funding_rates.get(&synthetic_token).unwrap()
                    [arr_start_idx as usize - 1..]
                    .to_vec()
                    .clone();

                swap_funding_prices = funding_prices.get(&synthetic_token).unwrap()
                    [arr_start_idx as usize - 1..]
                    .to_vec()
                    .clone();
            }
        };

        let swap_funding_info = SwapFundingInfo {
            current_funding_idx,
            swap_funding_rates,
            swap_funding_prices,
            min_swap_funding_idx,
        };

        return swap_funding_info;
    }
}

// * PRICING ================================================================================

/// The information about the min and max prices for each token this transaction batch. \
/// This is constructed after each transaction batch finalization and used in the cairo program.
pub struct PriceInfo<'a> {
    pub token: u32,
    /// Price data for the min price this batch for each token
    pub min_index_price_data: &'a OracleUpdate,
    /// Price data for the max price this batch for each token
    pub max_index_price_data: &'a OracleUpdate,
}

/// Constructs the price info for the current batch
pub fn get_price_info(
    min_index_price_data: &HashMap<u32, (u64, OracleUpdate)>,
    max_index_price_data: &HashMap<u32, (u64, OracleUpdate)>,
) -> Value {
    let mut price_info: Vec<PriceInfo> = Vec::new();

    for (token, (_, min_index_oracle_update)) in min_index_price_data.iter() {
        let max_index_oracle_update = &max_index_price_data.get(token).unwrap().1;

        let token_price_info = PriceInfo {
            token: *token,
            min_index_price_data: min_index_oracle_update,
            max_index_price_data: max_index_oracle_update,
        };

        price_info.push(token_price_info);
    }

    return serde_json::to_value(price_info).unwrap();
}

impl Serialize for PriceInfo<'_> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut price_info = serializer.serialize_struct("OracleUpdate", 4)?;

        price_info.serialize_field("token", &self.token)?;
        price_info.serialize_field("min", &self.min_index_price_data)?;
        price_info.serialize_field("max", &self.max_index_price_data)?;

        return price_info.end();
    }
}

// * GLOBAL DEX STATE ================================================================================

/// This holds the global state of the dex at the end of the batch.\
/// It is all the relevant information needed for the cairo program.
#[derive(Debug, Clone, Serialize)]
pub struct GlobalDexState {
    pub config_code: u64,
    pub init_state_root: String,
    pub final_state_root: String,
    pub init_perp_state_root: String,
    pub final_perp_state_root: String,
    pub state_tree_depth: u32,
    pub perp_tree_depth: u32,
    pub global_expiration_timestamp: u32,
    pub n_output_notes: u32,
    pub n_zero_notes: u32,
    pub n_output_positions: u32,
    pub n_empty_positions: u32,
    pub n_deposits: u32,
    pub n_withdrawals: u32,
    pub chain_ids: Vec<u32>,
}

impl GlobalDexState {
    pub fn new(
        config_code: u64,
        init_state_root: &BigUint,
        final_state_root: &BigUint,
        init_perp_state_root: &BigUint,
        final_perp_state_root: &BigUint,
        state_tree_depth: u32,
        perp_tree_depth: u32,
        global_expiration_timestamp: u32,
        n_output_notes: u32,
        n_zero_notes: u32,
        n_output_positions: u32,
        n_empty_positions: u32,
        n_deposits: u32,
        n_withdrawals: u32,
        chain_ids: Vec<u32>,
    ) -> GlobalDexState {
        let init_state_root = init_state_root.to_string();
        let final_state_root = final_state_root.to_string();
        let init_perp_state_root = init_perp_state_root.to_string();
        let final_perp_state_root = final_perp_state_root.to_string();

        GlobalDexState {
            config_code,
            init_state_root,
            final_state_root,
            init_perp_state_root,
            final_perp_state_root,
            state_tree_depth,
            perp_tree_depth,
            global_expiration_timestamp,
            n_output_notes,
            n_zero_notes,
            n_output_positions,
            n_empty_positions,
            n_deposits,
            n_withdrawals,
            chain_ids,
        }
    }
}

// * Global Config

// Structures:
// - assets: [token1, token2, ...]
// - observers : [observer1, observer2, ...]
// - everything else: [token1, value1, token2, value2, ...]

#[derive(Debug, Clone, Serialize)]
pub struct GlobalConfig {
    pub assets: Vec<u32>,
    pub collateral_token: u32,
    pub decimals_per_asset: Vec<u64>,
    pub price_decimals_per_asset: Vec<u64>,
    pub leverage_decimals: u8,
    pub leverage_bounds_per_asset: Vec<f64>,
    pub dust_amount_per_asset: Vec<u64>,
    pub observers: Vec<String>,
    pub min_partial_liquidation_sizes: Vec<u64>,
}

impl GlobalConfig {
    pub fn new() -> GlobalConfig {
        let assets = TOKENS.to_vec();
        let collateral_token = VALID_COLLATERAL_TOKENS[0];
        let decimals_per_asset = flatten_map(&DECIMALS_PER_ASSET);
        let price_decimals_per_asset = flatten_map(&PRICE_DECIMALS_PER_ASSET);
        let leverage_decimals = LEVERAGE_DECIMALS;
        let leverage_bounds_per_asset = flatten_leverage_bounds(&LEVERAGE_BOUNDS_PER_ASSET);
        let dust_amount_per_asset = flatten_map(&DUST_AMOUNT_PER_ASSET);
        let min_partial_liquidation_sizes = flatten_map(&MIN_PARTIAL_LIQUIDATION_SIZE);

        let observers = OBSERVERS.iter().map(|x| x.to_string()).collect();

        GlobalConfig {
            assets,
            collateral_token,
            decimals_per_asset,
            price_decimals_per_asset,
            leverage_decimals,
            leverage_bounds_per_asset,
            dust_amount_per_asset,
            observers,
            min_partial_liquidation_sizes,
        }
    }
}

fn flatten_map<T>(x: &phf::Map<&'static str, T>) -> Vec<u64>
where
    T: Into<u64> + Copy,
{
    let mut sorted_keys = x.keys().collect::<Vec<&&str>>();
    sorted_keys.sort();

    let mut v: Vec<u64> = Vec::new();
    for k in sorted_keys {
        let val = x.get(k).unwrap();
        v.push(u64::from_str(k).unwrap());
        v.push((*val).into());
    }
    return v;
}

fn flatten_leverage_bounds(x: &phf::Map<&'static str, [f32; 2]>) -> Vec<f64> {
    let mut sorted_keys = x.keys().collect::<Vec<&&str>>();
    sorted_keys.sort();

    let mut v: Vec<f64> = Vec::new();

    for k in sorted_keys {
        let val = x.get(k).unwrap();
        v.push(f64::from_str(k).unwrap());
        v.push(val[0] as f64);
        v.push(val[1] as f64);
    }
    return v;
}
