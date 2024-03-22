use phf::phf_map;
use serde::{Deserialize, Serialize};

pub mod liquidations;
pub mod order_execution;
pub mod perp_helpers;
pub mod perp_order;
pub mod perp_position;
pub mod perp_swap;

#[derive(PartialEq, Debug, Clone, Deserialize, Serialize)]
pub enum OrderSide {
    Long,
    Short,
}
#[derive(PartialEq, Debug, Clone, Deserialize, Serialize)]
pub enum OrderType {
    Limit,
    Market,
}
#[derive(PartialEq, Debug, Clone, Deserialize, Serialize)]
pub enum PositionEffectType {
    Open,
    Close,
    Modify,
}

pub static LEVERAGE_BOUNDS_PER_ASSET: phf::Map<&'static str, [f32; 2]> = phf_map! {
"3592681469" => [1.5, 30.0], // BTC
"453755560" => [15.0, 150.0], // ETH
"277158171" => [1000.0, 10_000.0], // SOL
};
pub const MAX_LEVERAGE: f64 = 15.0;

// BTC - 3592681469
// ETH - 453755560
// USDC - 2413654107
// SOL - 277158171
pub static ASSETS: [u32; 4] = [3592681469, 453755560, 2413654107, 277158171];
pub static SYNTHETIC_ASSETS: [u32; 3] = [3592681469, 453755560, 277158171];
pub const COLLATERAL_TOKEN: u32 = 2413654107;

pub static DECIMALS_PER_ASSET: phf::Map<&'static str, u8> = phf_map! {
"3592681469" => 8, // BTC
"453755560" => 8, // ETH
"2413654107" => 6, // USDC
"277158171" => 8, // SOL
};
// Minimum amount that is worth acknowledging
pub static DUST_AMOUNT_PER_ASSET: phf::Map<&'static str, u64> = phf_map! {
"3592681469" => 250, // BTC ~ 5c
"453755560" => 2500, // ETH ~ 5c
"2413654107" => 50_000, // USDC ~ 5c
"277158171" => 250_000, // SOL ~ 5c
};

// ? ------------------  SYNTHETIC_ASSETS ------------------ //

pub static PRICE_DECIMALS_PER_ASSET: phf::Map<&'static str, u8> = phf_map! {
"3592681469" => 6, // BTC
"453755560" => 6, // ETH
"277158171" => 6, // SOL
};

pub static IMPACT_NOTIONAL_PER_ASSET: phf::Map<&'static str, u64> = phf_map! {
"3592681469" => 20_000_000, // BTC
"453755560" => 200_000_000, // ETH
"277158171" => 7_500_000_000, // SOL

};

// Only allow partial liquidations on positions that are at least this size
pub static MIN_PARTIAL_LIQUIDATION_SIZE: phf::Map<&'static str, u64> = phf_map! {
"3592681469" => 5_000_000, // 0.05 BTC
"453755560" => 50_000_000, // 0.5 ETH
"277158171" => 350_000_000, // 3.5 SOL
};

pub const LEVERAGE_DECIMALS: u8 = 4; // 6 decimals for leverage
pub const COLLATERAL_TOKEN_DECIMALS: u8 = 6; // 6 decimals for USDC/USDT...

// impact Notional Amount = 500 USDC / Initial Margin Fraction

// notional_size0 => 2 BTC
// 3 BTC > 20X leverage > init_margin = 5%
// 6 BTC > 10X leverage > init_margin = 10%
// 9 BTC > 5X leverage > init_margin = 20%
// 12 BTC > 4X leverage > init_margin = 25%
// 16 BTC > 3X leverage > init_margin = 33.3%
// 20 BTC > 2X leverage > init_margin = 50%
// 25 BTC > 1.5X leverage > init_margin = 66.6%

// 10 BTC min init_margin = 3BTC*5% + 6BTC*10% + 1BTC*20% = 0.95BTC
// max leverage = 10BTC/0.95BTC = 10.5X

// * Price functions * // ====================================================================
pub fn get_price(synthetic_token: u32, collateral_amount: u64, synthetic_amount: u64) -> u64 {
    let synthetic_decimals: &u8 = DECIMALS_PER_ASSET
        .get(synthetic_token.to_string().as_str())
        .unwrap();

    let synthetic_price_decimals: &u8 = PRICE_DECIMALS_PER_ASSET
        .get(synthetic_token.to_string().as_str())
        .unwrap();

    let decimal_conversion: i8 = *synthetic_decimals as i8 + *synthetic_price_decimals as i8
        - COLLATERAL_TOKEN_DECIMALS as i8;
    let multiplier = 10_u128.pow(decimal_conversion as u32);

    let price = ((collateral_amount as u128 * multiplier) / synthetic_amount as u128) as u64;

    return price;
}

pub fn get_cross_price(
    base_token: u32,
    quote_token: u32,
    base_amount: u64,
    quote_amount: u64,
    _round: Option<bool>,
) -> f64 {
    // Price of two tokens in terms of each other (possible to get ETH/BTC price)

    if COLLATERAL_TOKEN == quote_token {
        let base_decimals = DECIMALS_PER_ASSET[&base_token.to_string()];
        let quote_decimals = DECIMALS_PER_ASSET[&quote_token.to_string()];

        let price = (quote_amount as f64 / 10_f64.powi(quote_decimals as i32))
            / (base_amount as f64 / 10_f64.powi(base_decimals as i32));

        return price;

        // return round_price(price, round);
    } else {
        return 0.0;
    }

    // TODO: What is the quote token is not a valid collateral token?

    // let base_decimals: &u8 = DECIMALS_PER_ASSET
    //     .get(base_token.to_string().as_str())
    //     .unwrap();
    // let base_price_decimals: &u8 = PRICE_DECIMALS_PER_ASSET
    //     .get(base_token.to_string().as_str())
    //     .unwrap();

    // let quote_decimals: &u8 = DECIMALS_PER_ASSET
    //     .get(quote_token.to_string().as_str())
    //     .unwrap();

    // let decimal_conversion = *base_decimals + *base_price_decimals - quote_decimals;
    // let multiplier = 10_u128.pow(decimal_conversion as u32);

    // let price = (quote_amount as u128 * multiplier) as u64 / base_amount;
    // return price as f64 / 10_f64.powi(*base_price_decimals as i32);
}

// * Price functions * // ====================================================================
pub fn get_collateral_amount(synthetic_token: u32, synthetic_amount: u64, price: u64) -> u64 {
    let synthetic_decimals: &u8 = DECIMALS_PER_ASSET
        .get(synthetic_token.to_string().as_str())
        .unwrap();

    let synthetic_price_decimals: &u8 = PRICE_DECIMALS_PER_ASSET
        .get(synthetic_token.to_string().as_str())
        .unwrap();

    let decimal_conversion: i8 = *synthetic_decimals as i8 + *synthetic_price_decimals as i8
        - COLLATERAL_TOKEN_DECIMALS as i8;
    let multiplier = 10_u128.pow(decimal_conversion as u32);

    let collateral_amount = ((synthetic_amount as u128 * price as u128) / multiplier) as u64;

    return collateral_amount;
}

pub fn round_price(price: f64, round: Option<bool>) -> f64 {
    if let Some(r) = round {
        if r {
            return (price * 100.0).ceil() / 100.0;
        } else {
            return (price * 100.0).floor() / 100.0;
        }
    }

    return (price * 100.0).floor() / 100.0;
}

pub fn scale_up_price(price: f64, token: u32) -> u64 {
    let price_decimals: &u8 = PRICE_DECIMALS_PER_ASSET
        .get(token.to_string().as_str())
        .unwrap();

    let price = price * 10_f64.powi(*price_decimals as i32);

    return price as u64;
}

pub fn scale_down_price(price: u64, token: u32) -> f64 {
    let price_decimals: &u8 = PRICE_DECIMALS_PER_ASSET
        .get(token.to_string().as_str())
        .unwrap();

    let price = price as f64 / 10_f64.powi(*price_decimals as i32);

    return price;
}
