use std::str::FromStr;

use num_bigint::BigUint;
use num_traits::FromPrimitive;

use error_stack::Result;
use serde::Deserialize as DeserializeTrait;

use crate::perpetual::OrderSide;
use crate::perpetual::{
    COLLATERAL_TOKEN_DECIMALS, DECIMALS_PER_ASSET, LEVERAGE_DECIMALS, MIN_PARTIAL_LIQUIDATION_SIZE,
    PRICE_DECIMALS_PER_ASSET,
};
use crate::utils::errors::{send_perp_swap_error, PerpSwapExecutionError};

use crate::utils::crypto_utils::hash_many;

// position should have address or something
#[derive(Debug, Clone)]
pub struct PerpPosition {
    pub index: u32, // index of the position in the state (merkle tree)
    pub position_header: PositionHeader,
    // ? Mutable fields
    pub order_side: OrderSide, // Long or Short
    pub position_size: u64,    // size of synthetic tokens
    pub margin: u64,           // margin in collateral token

    pub entry_price: u64,       // average buy/sell price of the position
    pub liquidation_price: u64, // price at which position will be liquidated
    pub bankruptcy_price: u64,  // price at which the position has zero margin left

    pub last_funding_idx: u32, // last index when funding payment was updated in the state (in cairo)
    pub vlp_supply: u64,       // vlp tokens minted for the position
    //
    pub hash: BigUint,
}

impl PerpPosition {
    pub fn new(
        order_side: OrderSide,
        position_size: u64,
        synthetic_token: u32,
        _collateral_token: u32, // TODO: What to do with this?
        margin: u64,
        leverage: u64,
        allow_partial_liquidations: bool,
        position_address: BigUint,
        current_funding_idx: u32,
        index: u32,
        fee_taken: u64,
    ) -> PerpPosition {
        let entry_price = _get_entry_price(margin, leverage, position_size, synthetic_token);

        let margin = margin - fee_taken;

        let bankruptcy_price: u64 = _get_bankruptcy_price(
            entry_price,
            margin,
            position_size,
            &order_side,
            synthetic_token,
        );

        let liquidation_price: u64 = _get_liquidation_price(
            entry_price,
            margin,
            position_size,
            &order_side,
            synthetic_token,
            allow_partial_liquidations,
        );

        let position_header = PositionHeader::new(
            synthetic_token,
            allow_partial_liquidations,
            position_address,
            0,
            0,
        );

        let hash: BigUint = _hash_position(
            &position_header.hash,
            &order_side,
            position_size,
            entry_price,
            liquidation_price,
            current_funding_idx,
            0,
        );

        PerpPosition {
            index,
            position_header,
            order_side,
            position_size,
            margin,
            entry_price,
            liquidation_price,
            bankruptcy_price,
            last_funding_idx: current_funding_idx,
            vlp_supply: 0,
            hash,
        }
    }

    //
    /// *  This is called if an open order was filled partially before.
    /// *  It Adds new_margin amount of collateral at new_entry_price with new_leverage
    pub fn add_margin_to_position(
        &mut self,
        added_margin: u64,
        added_size: u64,
        added_leverage: u64,
        fee_taken: u64,
    ) {
        let synthetic_decimals: &u8 = DECIMALS_PER_ASSET
            .get(self.position_header.synthetic_token.to_string().as_str())
            .unwrap();

        let synthetic_price_decimals: &u8 = PRICE_DECIMALS_PER_ASSET
            .get(self.position_header.synthetic_token.to_string().as_str())
            .unwrap();

        let decimal_conversion = *synthetic_decimals + *synthetic_price_decimals
            - (COLLATERAL_TOKEN_DECIMALS + LEVERAGE_DECIMALS);
        let multiplier = 10_u128.pow(decimal_conversion as u32);

        let added_entry_price: u64 = ((added_margin as u128 * added_leverage as u128 * multiplier)
            / added_size as u128) as u64;

        let added_margin: u64 = added_margin - fee_taken;

        // & nominal usd value = size * price
        let prev_nominal_usd: u128 = self.position_size as u128 * self.entry_price as u128;
        let added_nominal_usd: u128 = added_size as u128 * added_entry_price as u128;

        // & average open = (amount*entry_price + new_amount*new_entry_price) / (amount+new_amount)
        let average_entry_price =
            (prev_nominal_usd + added_nominal_usd) / (self.position_size + added_size) as u128;

        // ? Make updates to the position
        self.position_size += added_size as u64;
        self.margin += added_margin as u64;
        self.entry_price = average_entry_price as u64;
        self.update_position_info();
    }

    //
    /// *  Adds added_size amount of synthetic tokens to the position
    /// *  This increases the size while worsening the liquidation price
    pub fn increase_position_size(
        &mut self,
        added_size: u64,
        added_price: u64,
        fee_taken: u64,
        funding_rates: Vec<i64>,
        prices: Vec<u64>,
        funding_idx: u32,
    ) {
        // &apply funding
        self.apply_funding(funding_rates, prices, funding_idx);

        let prev_nominal_usd = self.position_size as u128 * self.entry_price as u128;
        let added_nominal_usd = added_size as u128 * added_price as u128;

        // & average open = (amount*entry_price + added_amount*added_entry_price) / (amount+added_amount)
        let average_entry_price =
            (prev_nominal_usd + added_nominal_usd) / (self.position_size + added_size) as u128;

        // ? Make updates to the position
        self.position_size += added_size;
        self.margin -= fee_taken;
        self.entry_price = average_entry_price as u64;
        self.last_funding_idx = funding_idx;
        self.update_position_info();
    }

    //
    /// * Reduces the position size by reduction_size amount of synthetic tokens at price
    /// * This reduces the size while  improving the liquidation price
    pub fn reduce_position_size(
        &mut self,
        reduction_size: u64,
        price: u64,
        fee_taken: u64,
        funding_rates: Vec<i64>,
        prices: Vec<u64>,
        funding_idx: u32,
    ) {
        // & apply funding
        self.apply_funding(funding_rates, prices, funding_idx);

        let synthetic_decimals: &u8 = DECIMALS_PER_ASSET
            .get(self.position_header.synthetic_token.to_string().as_str())
            .unwrap();

        let synthetic_price_decimals: &u8 = PRICE_DECIMALS_PER_ASSET
            .get(self.position_header.synthetic_token.to_string().as_str())
            .unwrap();

        let new_size = self.position_size - reduction_size;

        // & get the profit/loss to add/subtract from the margin
        let decimal_conversion =
            *synthetic_decimals + *synthetic_price_decimals - COLLATERAL_TOKEN_DECIMALS;
        let multiplier = 10_u128.pow(decimal_conversion as u32);

        let realized_pnl: i128;
        if self.order_side == OrderSide::Long {
            realized_pnl = reduction_size as i128
                * (price as i64 - self.entry_price as i64) as i128
                / multiplier as i128;
        } else {
            realized_pnl = reduction_size as i128
                * (self.entry_price as i64 - price as i64) as i128
                / multiplier as i128;
        }

        let updated_margin = (self.margin as i128 + realized_pnl - fee_taken as i128) as u64;

        // // & bankruptcy_price = entry_price +/- margin/(amount+new_amount)
        // // & liquidation_price = bankruptcy_price -/+ maintenance_margin/(amount+new_amount)
        // let new_bankruptcy_price: u64 = _get_bankruptcy_price(
        //     self.entry_price,
        //     updated_margin,
        //     new_size,
        //     &self.order_side,
        //     self.position_header.synthetic_token,
        // );

        // let new_liquidation_price: u64 = _get_liquidation_price(
        //     self.entry_price,
        //     updated_margin,
        //     new_size,
        //     &self.order_side,
        //     self.position_header.synthetic_token,
        //     self.position_header.allow_partial_liquidations,
        // );

        // let new_hash: BigUint = _hash_position(
        //     &self.position_header.hash,
        //     &self.order_side,
        //     new_size,
        //     self.entry_price,
        //     new_liquidation_price,
        //     funding_idx,
        //     self.vlp_supply,
        // );

        // ? Make updates to the position
        self.position_size = new_size;
        self.margin = updated_margin as u64;
        self.last_funding_idx = funding_idx;
        self.update_position_info();
    }

    //
    /// * Flip position side and update the position
    /// * If position is being reduced more than the size, then the position is opened in another direction
    pub fn flip_position_side(
        &mut self,
        reduction_size: u64,
        price: u64,
        fee_taken: u64,
        funding_rates: Vec<i64>,
        prices: Vec<u64>,
        funding_idx: u32,
    ) {
        // & apply funding
        self.apply_funding(funding_rates, prices, funding_idx);

        let synthetic_decimals: &u8 = DECIMALS_PER_ASSET
            .get(self.position_header.synthetic_token.to_string().as_str())
            .unwrap();

        let synthetic_price_decimals: &u8 = PRICE_DECIMALS_PER_ASSET
            .get(self.position_header.synthetic_token.to_string().as_str())
            .unwrap();

        let new_size = reduction_size - self.position_size;

        // & get the profit/loss to add/subtract from the margin
        let decimal_conversion =
            *synthetic_decimals + *synthetic_price_decimals - COLLATERAL_TOKEN_DECIMALS;
        let multiplier = 10_u128.pow(decimal_conversion as u32);

        let realized_pnl: i128;
        if self.order_side == OrderSide::Long {
            realized_pnl = self.position_size as i128
                * (price as i64 - self.entry_price as i64) as i128
                / multiplier as i128;
        } else {
            realized_pnl = self.position_size as i128
                * (self.entry_price as i64 - price as i64) as i128
                / multiplier as i128;
        }

        let updated_margin = (self.margin as i128 + realized_pnl - fee_taken as i128) as u64;

        let new_order_side = match self.order_side {
            OrderSide::Long => OrderSide::Short,
            OrderSide::Short => OrderSide::Long,
        };

        // let new_bankruptcy_price: u64 = _get_bankruptcy_price(
        //     price,
        //     updated_margin,
        //     new_size,
        //     &new_order_side,
        //     self.position_header.synthetic_token,
        // );
        // let new_liquidation_price: u64 = _get_liquidation_price(
        //     price,
        //     updated_margin,
        //     new_size,
        //     &new_order_side,
        //     self.position_header.synthetic_token,
        //     self.position_header.allow_partial_liquidations,
        // );

        // let new_hash: BigUint = _hash_position(
        //     &self.position_header.hash,
        //     &new_order_side,
        //     new_size,
        //     price,
        //     new_liquidation_price,
        //     funding_idx,
        //     self.vlp_supply,
        // );

        // ? Make updates to the position
        self.order_side = new_order_side;
        self.position_size = new_size;
        self.margin = updated_margin as u64;
        self.entry_price = price;
        self.update_position_info();
    }

    //
    /// * Partially fill a position close order
    pub fn close_position_partialy(
        &mut self,
        reduction_size: u64,
        close_price: u64,
        fee_taken: u64,
        funding_rates: Vec<i64>,
        prices: Vec<u64>,
        funding_idx: u32,
    ) -> Result<u64, PerpSwapExecutionError> {
        // & apply funding
        self.apply_funding(funding_rates, prices, funding_idx);

        // & closes part of a position while keeping the liquidation price the same
        // & returns part of the collateral and pnl

        let synthetic_decimals: &u8 = DECIMALS_PER_ASSET
            .get(self.position_header.synthetic_token.to_string().as_str())
            .unwrap();

        let synthetic_price_decimals: &u8 = PRICE_DECIMALS_PER_ASSET
            .get(self.position_header.synthetic_token.to_string().as_str())
            .unwrap();

        let updated_size = self.position_size - reduction_size;

        let reduction_margin =
            ((reduction_size as u128 * self.margin as u128) / self.position_size as u128) as u64;

        // & get the profit/loss to add/subtract from the margin
        let decimal_conversion =
            *synthetic_decimals + *synthetic_price_decimals - COLLATERAL_TOKEN_DECIMALS;
        let multiplier = 10_u128.pow(decimal_conversion as u32);

        let realized_pnl: i128;
        if self.order_side == OrderSide::Long {
            realized_pnl = reduction_size as i128
                * (close_price as i64 - self.entry_price as i64) as i128
                / multiplier as i128;
        } else {
            realized_pnl = reduction_size as i128
                * (self.entry_price as i64 - close_price as i64) as i128
                / multiplier as i128;
        }

        let return_collateral = (reduction_margin as i128 + realized_pnl) as i64 - fee_taken as i64;
        if return_collateral <= 0 {
            return Err(send_perp_swap_error(
                "Returned collateral cannot be negative".to_string(),
                None,
                None,
            ));
        }

        let margin = self.margin - reduction_margin;

        let new_hash: BigUint = _hash_position(
            &self.position_header.hash,
            &self.order_side,
            updated_size,
            self.entry_price,
            self.liquidation_price,
            funding_idx,
            self.vlp_supply,
        );

        // ? Make updates to the position
        self.position_size = updated_size;
        self.margin = margin;
        self.last_funding_idx = funding_idx;
        self.hash = new_hash;

        return Ok(return_collateral as u64);
    }

    //
    /// * Close a position and return the collateral +/- pnl
    pub fn close_position(
        &mut self,
        price: u64,
        fee_taken: u64,
        funding_rates: Vec<i64>,
        prices: Vec<u64>,
        funding_idx: u32,
    ) -> Result<u64, PerpSwapExecutionError> {
        self.apply_funding(funding_rates, prices, funding_idx);

        let margin: u64 = self.margin;

        let pnl = self.get_pnl(price);

        self.position_size = 0;
        self.margin = 0;

        let return_collateral = margin as i64 + pnl - fee_taken as i64;
        if return_collateral <= 0 {
            return Err(send_perp_swap_error(
                "Returned collateral cannot be negative".to_string(),
                None,
                None,
            ));
        }
        return Ok(return_collateral as u64);
    }

    //
    /// * Gets the amount of position to be liquidated
    /// * Returns: (is_fully_liquidation, liquidatable amount)
    pub fn is_position_liquidatable(&self, market_price: u64, index_price: u64) -> (bool, u64) {
        // & if market_price is greater than the bankruptcy price, the leftover collateral goes to the insurance fund
        if (self.order_side == OrderSide::Long && index_price > self.liquidation_price)
            || (self.order_side == OrderSide::Short && index_price < self.liquidation_price)
        {
            return (false, 0);
        }

        if self.position_header.allow_partial_liquidations
            && self.position_size
                > MIN_PARTIAL_LIQUIDATION_SIZE
                    [self.position_header.synthetic_token.to_string().as_str()]
        {
            let synthetic_price_decimals: &u8 = PRICE_DECIMALS_PER_ASSET
                .get(self.position_header.synthetic_token.to_string().as_str())
                .unwrap();

            let synthetic_decimals: &u8 = DECIMALS_PER_ASSET
                .get(self.position_header.synthetic_token.to_string().as_str())
                .unwrap();

            // & price_delta = entry_price - market_price for long and market_price - entry_price for short
            // & new_size = (margin - position.size * price_delta) / ((entry_price +/- price_delta) * (im_fraction + lf_rate))  ; - if long, + if short

            let decimal_conversion1 =
                *synthetic_price_decimals + *synthetic_decimals - COLLATERAL_TOKEN_DECIMALS;
            let multiplier1 = 10_u128.pow(decimal_conversion1 as u32);

            let price_delta = if self.order_side == OrderSide::Long {
                self.entry_price as u64 - market_price as u64
            } else {
                market_price as u64 - self.entry_price as u64
            };

            let im_rate = 67; // 6.7 %
            let liquidator_fee_rate = 5; // 0.5 %

            let s1 = self.margin as u128 * multiplier1;
            let s2 = self.position_size as u128 * price_delta as u128;

            let new_size =
                (s1 - s2) * 1000 / (market_price as u128 * (im_rate + liquidator_fee_rate) as u128);

            let liquidatable_size = self.position_size - new_size as u64;

            return (true, liquidatable_size);
        } else {
            let liquidatable_size = self.position_size;

            return (true, liquidatable_size);
        }
    }

    //
    /// * Liquidate the position either partially or fully
    /// * Returns: (liquidated_size, liquidator_fee, leftover_collateral, is_partial_liquidation)
    pub fn liquidate_position(
        &mut self,
        market_price: u64,
        index_price: u64,
        funding_rates: Vec<i64>,
        prices: Vec<u64>,
        funding_idx: u32,
    ) -> Result<(u64, u64, i64, bool), PerpSwapExecutionError> {
        // & if market_price is greater than the bankruptcy price, the leftover collateral goes to the insurance fund
        if (self.order_side == OrderSide::Long && index_price > self.liquidation_price)
            || (self.order_side == OrderSide::Short && index_price < self.liquidation_price)
        {
            return Err(send_perp_swap_error(
                "Index price is not worse than the liquidation price".to_string(),
                None,
                None,
            ));
        }

        // & apply funding
        self.apply_funding(funding_rates, prices, funding_idx);

        if self.position_header.allow_partial_liquidations
            && self.position_size
                >= MIN_PARTIAL_LIQUIDATION_SIZE
                    [self.position_header.synthetic_token.to_string().as_str()]
        {
            let (liquidator_fee, liquidated_size) =
                self.partially_liquidate_position(market_price)?;

            return Ok((liquidated_size, liquidator_fee, 0, true));
        } else {
            let size = self.position_size;
            let (leftover_collateral, liquidator_fee) =
                self.fully_liquidate_position(market_price)?;

            return Ok((size, liquidator_fee, leftover_collateral, false));
        }
    }

    //
    // * Partially liquidate the position by closing enough of it to bring the margin back to the initial margin requirement
    fn partially_liquidate_position(
        &mut self,
        market_price: u64,
    ) -> Result<(u64, u64), PerpSwapExecutionError> {
        let synthetic_price_decimals: &u8 = PRICE_DECIMALS_PER_ASSET
            .get(self.position_header.synthetic_token.to_string().as_str())
            .unwrap();

        let synthetic_decimals: &u8 = DECIMALS_PER_ASSET
            .get(self.position_header.synthetic_token.to_string().as_str())
            .unwrap();

        // & price_delta = entry_price - market_price for long and market_price - entry_price for short
        // & new_size = (margin - position.size * price_delta) / ((entry_price +/- price_delta) * (im_fraction + lf_rate))  ; - if long, + if short

        let decimal_conversion1 =
            *synthetic_price_decimals + *synthetic_decimals - COLLATERAL_TOKEN_DECIMALS;
        let multiplier1 = 10_u128.pow(decimal_conversion1 as u32);

        let price_delta = if self.order_side == OrderSide::Long {
            self.entry_price as u64 - market_price as u64
        } else {
            market_price as u64 - self.entry_price as u64
        };

        let im_rate = 67; // 6.7 %
        let liquidator_fee_rate = 5; // 0.5 %

        let s1 = self.margin as u128 * multiplier1;
        let s2 = self.position_size as u128 * price_delta as u128;

        let numerator = s1 - s2;
        let denominator = market_price as u128 * (im_rate + liquidator_fee_rate) as u128 / 1000;

        let new_size = numerator / denominator;

        let liquidated_size = self.position_size - new_size as u64;

        //& Leftover value: (market_price - bankruptcy_price) * position_size   (denominated in collateral - USD)

        let liquidator_fee = (liquidated_size as u128 * market_price as u128 * liquidator_fee_rate
            / (multiplier1 as u128 * 1000)) as u64;

        // let new_bankruptcy_price: u64 = _get_bankruptcy_price(
        //     self.entry_price,
        //     self.margin - liquidator_fee,
        //     new_size as u64,
        //     &self.order_side,
        //     self.position_header.synthetic_token,
        // );

        // let new_liquidation_price: u64 = _get_liquidation_price(
        //     self.entry_price,
        //     self.margin - liquidator_fee,
        //     new_size as u64,
        //     &self.order_side,
        //     self.position_header.synthetic_token,
        //     self.position_header.allow_partial_liquidations,
        // );

        // let new_hash: BigUint = _hash_position(
        //     &self.position_header.hash,
        //     &self.order_side,
        //     new_size as u64,
        //     self.entry_price,
        //     new_liquidation_price,
        //     self.last_funding_idx,
        //     self.vlp_supply,
        // );

        self.position_size = new_size as u64;
        self.margin -= liquidator_fee;
        self.update_position_info();

        // if leftover_value > 0 add to insurance_fund else subtract
        return Ok((liquidator_fee as u64, liquidated_size));
    }

    //
    // * Liquidate the position by closing it fully and add/remove the leftover margin to/from the insurance fund
    fn fully_liquidate_position(
        &mut self,
        market_price: u64,
    ) -> Result<(i64, u64), PerpSwapExecutionError> {
        let synthetic_price_decimals: &u8 = PRICE_DECIMALS_PER_ASSET
            .get(self.position_header.synthetic_token.to_string().as_str())
            .unwrap();

        let synthetic_decimals: &u8 = DECIMALS_PER_ASSET
            .get(self.position_header.synthetic_token.to_string().as_str())
            .unwrap();

        // & get the profit/loss to add/subtract from the margin
        let decimal_conversion =
            *synthetic_price_decimals + *synthetic_decimals - COLLATERAL_TOKEN_DECIMALS;
        let multiplier = 10_i128.pow(decimal_conversion as u32);

        let liquidator_fee_rate = 5; // 0.5 %
        let liquidator_fee =
            (self.position_size as u128 * market_price as u128 * liquidator_fee_rate as u128
                / (multiplier as u128 * 1000)) as u64;

        //& Leftover value: (market_price - bankruptcy_price) * position_size   (denominated in collateral - USD)
        let leftover_value: i128;
        if self.order_side == OrderSide::Long {
            leftover_value = (market_price as i64 - self.bankruptcy_price as i64) as i128
                * self.position_size as i128
                / multiplier as i128
                - liquidator_fee as i128;
        } else {
            leftover_value = (self.bankruptcy_price as i64 - market_price as i64) as i128
                * self.position_size as i128
                / multiplier as i128
                - liquidator_fee as i128;
        }

        self.position_size = 0;
        self.margin = 0;

        // if leftover_value > 0 add to insurance_fund else subtract
        return Ok((leftover_value as i64, liquidator_fee as u64));
    }

    // -----------------------------------------------------------------------

    pub fn modify_margin(&mut self, margin_change: i64) -> std::result::Result<(), String> {
        // ? Verify the margin_change is valid
        if margin_change == 0
            || self.margin as i64 + margin_change
                <= DUST_AMOUNT_PER_ASSET[&COLLATERAL_TOKEN.to_string()] as i64
        {
            return Err("Invalid margin change".to_string());
        }

        // Todo: Maybe have a constant fee here (like 5 cents or something)

        let margin = (self.margin as i64 + margin_change) as u64;

        // let new_bankruptcy_price: u64 = _get_bankruptcy_price(
        //     self.entry_price,
        //     margin,
        //     self.position_size,
        //     &self.order_side,
        //     self.position_header.synthetic_token,
        // );

        // let new_liquidation_price: u64 = _get_liquidation_price(
        //     self.entry_price,
        //     margin,
        //     self.position_size,
        //     &self.order_side,
        //     self.position_header.synthetic_token,
        //     self.position_header.allow_partial_liquidations,
        // );

        // let new_hash: BigUint = _hash_position(
        //     &self.position_header.hash,
        //     &self.order_side,
        //     self.position_size,
        //     self.entry_price,
        //     new_liquidation_price,
        //     self.last_funding_idx,
        //     self.vlp_supply,
        // );

        // ? Make updates to the position
        self.margin = margin;
        self.update_position_info();

        Ok(())
    }

    //  -----------------------------------------------------------------------

    pub fn get_pnl(&self, index_price: u64) -> i64 {
        let synthetic_decimals: &u8 = DECIMALS_PER_ASSET
            .get(self.position_header.synthetic_token.to_string().as_str())
            .unwrap();

        let synthetic_price_decimals: &u8 = PRICE_DECIMALS_PER_ASSET
            .get(self.position_header.synthetic_token.to_string().as_str())
            .unwrap();

        // & get the profit/loss to add/subtract from the margin
        let decimal_conversion =
            *synthetic_decimals + *synthetic_price_decimals - COLLATERAL_TOKEN_DECIMALS;
        let multiplier = 10_i128.pow(decimal_conversion as u32);

        let realized_pnl: i128;
        if self.order_side == OrderSide::Long {
            realized_pnl = self.position_size as i128
                * (index_price as i64 - self.entry_price as i64) as i128
                / multiplier;
        } else {
            realized_pnl = self.position_size as i128
                * (self.entry_price as i64 - index_price as i64) as i128
                / multiplier;
        }

        return realized_pnl as i64;
    }

    pub fn get_current_leverage(&self, index_price: u64) -> Result<u64, PerpSwapExecutionError> {
        // ? Make sure the index price is not 0
        if index_price == 0 {
            return Err(send_perp_swap_error(
                "Index price cannot be 0".to_string(),
                None,
                None,
            ));
        }

        let pnl: i64 = self.get_pnl(index_price);

        let synthetic_decimals: &u8 = DECIMALS_PER_ASSET
            .get(self.position_header.synthetic_token.to_string().as_str())
            .unwrap();

        let synthetic_price_decimals: &u8 = PRICE_DECIMALS_PER_ASSET
            .get(self.position_header.synthetic_token.to_string().as_str())
            .unwrap();

        let decimal_conversion = *synthetic_decimals + *synthetic_price_decimals
            - (COLLATERAL_TOKEN_DECIMALS + LEVERAGE_DECIMALS);
        let multiplier = 10_u128.pow(decimal_conversion as u32);

        if pnl < 0 && pnl.abs() as u64 > self.margin {
            return Err(send_perp_swap_error(
                "Position is liquidatable".to_string(),
                None,
                Some("position is liquidatable".to_string()),
            ));
        }

        let current_leverage: u64 = ((index_price as u128 * self.position_size as u128)
            / ((self.margin as i64 + pnl) as u128 * multiplier))
            as u64;

        return Ok(current_leverage);
    }

    fn apply_funding(&mut self, funding_rates: Vec<i64>, prices: Vec<u64>, funding_idx: u32) {
        // & Funding rate are the funding rate percentages that keep the market price close to the index price

        // Cairo input - array of funding indexes starting at the minimal one

        if funding_idx <= self.last_funding_idx {
            return;
        }

        let synthetic_decimals: &u8 = DECIMALS_PER_ASSET
            .get(self.position_header.synthetic_token.to_string().as_str())
            .unwrap();

        let synthetic_price_decimals: &u8 = PRICE_DECIMALS_PER_ASSET
            .get(self.position_header.synthetic_token.to_string().as_str())
            .unwrap();

        // & get the profit/loss to add/subtract from the margin
        let decimal_conversion =
            *synthetic_decimals + *synthetic_price_decimals - COLLATERAL_TOKEN_DECIMALS;
        let multiplier = 10_u128.pow(decimal_conversion as u32);

        let mut funding_sum: i128 = 0;
        for i in 0..funding_rates.len() {
            let funding_rate = funding_rates[i];
            let funding_price = prices[i];

            // funding rate has 5 decimal places
            let funding = self.position_size as i64 * funding_rate / 100_000;
            let funding_in_usd = funding as i128 * funding_price as i128 / multiplier as i128;

            funding_sum += funding_in_usd;
        }

        let margin_after_funding = if self.order_side == OrderSide::Long {
            (self.margin as i128 - funding_sum) as u128
        } else {
            (self.margin as i128 + funding_sum) as u128
        }; // Todo: check which is correct + or - depending on order_side

        // ? Make updates to the position
        self.margin = margin_after_funding as u64;
        self.last_funding_idx = funding_idx;
    }

    //  -----------------------------------------------------------------------

    /// This function recalculates the bankruptcy and liquidation price and hash
    pub fn update_position_info(&mut self) {
        let new_bankruptcy_price: u64 = _get_bankruptcy_price(
            self.entry_price,
            self.margin,
            self.position_size,
            &self.order_side,
            self.position_header.synthetic_token,
        );

        let new_liquidation_price: u64 = _get_liquidation_price(
            self.entry_price,
            self.margin,
            self.position_size,
            &self.order_side,
            self.position_header.synthetic_token,
            self.position_header.allow_partial_liquidations,
        );

        let new_hash: BigUint = _hash_position(
            &self.position_header.hash,
            &self.order_side,
            self.position_size,
            self.entry_price,
            new_liquidation_price,
            self.last_funding_idx,
            self.vlp_supply,
        );

        self.liquidation_price = new_liquidation_price;
        self.bankruptcy_price = new_bankruptcy_price;
        self.hash = new_hash;
    }

    pub fn hash_position(&self) -> BigUint {
        let position_hash: BigUint = _hash_position(
            &self.position_header.hash,
            &self.order_side,
            self.position_size,
            self.entry_price,
            self.liquidation_price,
            self.last_funding_idx,
            self.vlp_supply,
        );

        return position_hash;
    }

    //
}

#[derive(Debug, Clone)]
pub struct PositionHeader {
    pub synthetic_token: u32,             // type of asset being traded
    pub position_address: BigUint,        // address of the position (for signatures)
    pub allow_partial_liquidations: bool, // if true, allow partial liquidations
    pub vlp_token: u32,                   // token used for the vlp
    pub max_vlp_supply: u64,              // max supply of vlp tokens
    pub hash: BigUint,                    // hash of the position
}

impl PositionHeader {
    pub fn new(
        synthetic_token: u32,
        allow_partial_liquidations: bool,
        position_address: BigUint,
        vlp_token: u32,
        max_vlp_supply: u64,
    ) -> Self {
        let header_hash = _hash_position_header(
            synthetic_token,
            allow_partial_liquidations,
            &position_address,
            vlp_token,
            max_vlp_supply,
        );

        PositionHeader {
            allow_partial_liquidations,
            synthetic_token,
            position_address,
            hash: header_hash,
            vlp_token,
            max_vlp_supply,
        }
    }

    pub fn update_hash(&mut self) {
        let header_hash = _hash_position_header(
            self.synthetic_token,
            self.allow_partial_liquidations,
            &self.position_address,
            self.vlp_token,
            self.max_vlp_supply,
        );

        self.hash = header_hash;
    }
}

// * =============================================================================================================

use serde::ser::{Serialize, SerializeStruct, Serializer};

impl Serialize for PerpPosition {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut position = serializer.serialize_struct("PerpPosition", 12)?;

        position.serialize_field("position_header", &self.position_header)?;
        position.serialize_field("order_side", &self.order_side)?;
        position.serialize_field("position_size", &self.position_size)?;
        position.serialize_field("margin", &self.margin)?;
        position.serialize_field("entry_price", &self.entry_price)?;
        position.serialize_field("liquidation_price", &self.liquidation_price)?;
        position.serialize_field("bankruptcy_price", &self.bankruptcy_price)?;
        position.serialize_field("last_funding_idx", &self.last_funding_idx)?;
        position.serialize_field("vlp_supply", &self.vlp_supply)?;
        position.serialize_field("hash", &self.hash.to_string())?;
        position.serialize_field("index", &self.index)?;

        return position.end();
    }
}

// * ---------------------------------------------

impl Serialize for PositionHeader {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut header = serializer.serialize_struct("PositionHeader", 7)?;

        header.serialize_field("synthetic_token", &self.synthetic_token)?;
        // header.serialize_field("collateral_token", &self.collateral_token)?;
        header.serialize_field(
            "allow_partial_liquidations",
            &self.allow_partial_liquidations,
        )?;
        header.serialize_field("position_address", &self.position_address.to_string())?;
        header.serialize_field("vlp_token", &self.vlp_token)?;
        header.serialize_field("max_vlp_supply", &self.max_vlp_supply)?;
        header.serialize_field("hash", &self.hash.to_string())?;

        return header.end();
    }
}

// ---------------------------------------------

use serde::de::{Deserialize, Deserializer};

use super::{COLLATERAL_TOKEN, DUST_AMOUNT_PER_ASSET};

impl<'de> Deserialize<'de> for PerpPosition {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(DeserializeTrait)]
        struct Helper {
            position_header: PositionHeader,
            order_side: String,
            position_size: u64,
            margin: u64,
            entry_price: u64,
            liquidation_price: u64,
            bankruptcy_price: u64,
            last_funding_idx: u32,
            vlp_supply: u64,
            hash: String,
            index: u32,
        }

        let helper = Helper::deserialize(deserializer)?;

        Ok(PerpPosition {
            position_header: helper.position_header,
            order_side: if helper.order_side == "Long" {
                OrderSide::Long
            } else {
                OrderSide::Short
            },
            position_size: helper.position_size,
            margin: helper.margin,
            entry_price: helper.entry_price,
            liquidation_price: helper.liquidation_price,
            bankruptcy_price: helper.bankruptcy_price,
            last_funding_idx: helper.last_funding_idx,
            vlp_supply: helper.vlp_supply,
            hash: BigUint::from_str(&helper.hash).unwrap(),
            index: helper.index,
        })
    }
}

impl<'de> Deserialize<'de> for PositionHeader {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(DeserializeTrait)]
        struct Helper {
            synthetic_token: u32,
            // collateral_token: u32,
            position_address: String,
            allow_partial_liquidations: bool,
            vlp_token: u32,
            max_vlp_supply: u64,
            hash: String,
        }

        let helper = Helper::deserialize(deserializer)?;

        Ok(PositionHeader {
            synthetic_token: helper.synthetic_token,
            // collateral_token: helper.collateral_token,
            position_address: BigUint::from_str(&helper.position_address).unwrap(),
            allow_partial_liquidations: helper.allow_partial_liquidations,
            vlp_token: helper.vlp_token,
            max_vlp_supply: helper.max_vlp_supply,
            hash: BigUint::from_str(&helper.hash).unwrap(),
        })
    }
}

// * ---------------------------------------------

pub fn _hash_position(
    header_hash: &BigUint,
    order_side: &OrderSide,
    position_size: u64,
    entry_price: u64,
    liquidation_price: u64,
    current_funding_idx: u32,
    vlp_supply: u64,
) -> BigUint {
    let mut hash_inputs: Vec<&BigUint> = Vec::new();

    // & hash = H({header_hash, order_side, position_size, entry_price, liquidation_price, current_funding_idx, vlp_supply})
    hash_inputs.push(&header_hash);

    let order_side = BigUint::from_u8(if *order_side == OrderSide::Long { 1 } else { 0 }).unwrap();
    hash_inputs.push(&order_side);

    let position_size = BigUint::from_u64(position_size).unwrap();
    hash_inputs.push(&position_size);
    let entry_price = BigUint::from_u64(entry_price).unwrap();
    hash_inputs.push(&entry_price);
    let liquidation_price = BigUint::from_u64(liquidation_price).unwrap();
    hash_inputs.push(&liquidation_price);

    let current_funding_idx = BigUint::from_u32(current_funding_idx).unwrap();
    hash_inputs.push(&current_funding_idx);

    let vlp_supply = BigUint::from_u64(vlp_supply).unwrap();
    hash_inputs.push(&vlp_supply);

    let position_hash = hash_many(&hash_inputs);

    return position_hash;
}

fn _hash_position_header(
    synthetic_token: u32,
    allow_partial_liquidations: bool,
    position_address: &BigUint,
    vlp_token: u32,
    max_vlp_supply: u64,
) -> BigUint {
    let mut hash_inputs: Vec<&BigUint> = Vec::new();

    // & hash = H({allow_partial_liquidations, synthetic_token, position_address, vlp_token, max_vlp_supply})
    let allow_partial_liquidations =
        BigUint::from_u8(if allow_partial_liquidations { 1 } else { 0 }).unwrap();
    hash_inputs.push(&allow_partial_liquidations);

    let synthetic_token = BigUint::from_u32(synthetic_token).unwrap();
    hash_inputs.push(&synthetic_token);

    hash_inputs.push(position_address);

    let vlp_token = BigUint::from_u32(vlp_token).unwrap();
    hash_inputs.push(&vlp_token);

    let max_vlp_supply = BigUint::from_u64(max_vlp_supply).unwrap();
    hash_inputs.push(&max_vlp_supply);

    let position_hash = hash_many(&hash_inputs);

    return position_hash;
}

fn _get_entry_price(initial_margin: u64, leverage: u64, size: u64, synthetic_token: u32) -> u64 {
    // ? Assuming the collateral token is USD pegged and has 4 decimal places

    let synthetic_decimals: &u8 = DECIMALS_PER_ASSET
        .get(synthetic_token.to_string().as_str())
        .unwrap_or(&9);

    let synthetic_price_decimals: &u8 = PRICE_DECIMALS_PER_ASSET
        .get(synthetic_token.to_string().as_str())
        .unwrap_or(&6);

    // ! Stable coins have decimal places hardcoded to 6 for now
    // ! Leverage too

    // = synthetic_decimals + synthetic_price_decimals - (collateral_decimals + leverage_decimals)
    let decimal_conversion: u8 = *synthetic_decimals as u8 + *synthetic_price_decimals as u8
        - (COLLATERAL_TOKEN_DECIMALS + LEVERAGE_DECIMALS);
    let multiplier = 10_u128.pow(decimal_conversion as u32);

    let price: u64 =
        ((initial_margin as u128 * leverage as u128 * multiplier) / size as u128) as u64;

    return price;
}

fn _get_liquidation_price(
    entry_price: u64,
    margin: u64,
    position_size: u64,
    order_side: &OrderSide,
    synthetic_token: u32,
    is_partial_liquidation: bool,
) -> u64 {
    // maintenance margin
    let mm_fraction = if is_partial_liquidation
        && position_size > MIN_PARTIAL_LIQUIDATION_SIZE[synthetic_token.to_string().as_str()]
    {
        4 //%
    } else {
        3 //%
    };

    let synthetic_decimals: &u8 = DECIMALS_PER_ASSET
        .get(synthetic_token.to_string().as_str())
        .unwrap();

    let synthetic_price_decimals: &u8 = PRICE_DECIMALS_PER_ASSET
        .get(synthetic_token.to_string().as_str())
        .unwrap();

    let dec_conversion1: i8 = *synthetic_decimals as i8 + *synthetic_price_decimals as i8
        - COLLATERAL_TOKEN_DECIMALS as i8;
    let multiplier1 = 10_u128.pow(dec_conversion1 as u32);

    // & price_delta = (margin - mm_fraction * entry_price * size) / ((1 -/+ mm_fraction)*size) ; - for long, + for short

    let d1 = margin as u128 * multiplier1 as u128;
    let d2 = mm_fraction as u128 * entry_price as u128 * position_size as u128 / 100;

    if *order_side == OrderSide::Long {
        if position_size == 0 {
            return 0;
        }

        let price_delta =
            ((d1 - d2) * 100) / ((100_u128 - mm_fraction as u128) * position_size as u128);

        let liquidation_price = entry_price.checked_sub(price_delta as u64);

        return liquidation_price.unwrap_or(0);
    } else {
        if position_size == 0 {
            return 1_000_000_000 * 10_u64.pow(*synthetic_price_decimals as u32);
        }

        let price_delta =
            ((d1 - d2) * 100) / ((100_u128 + mm_fraction as u128) * position_size as u128);

        let liquidation_price = entry_price + price_delta as u64;

        return liquidation_price;
    }
}

fn _get_bankruptcy_price(
    entry_price: u64,
    margin: u64,
    size: u64,
    order_side: &OrderSide,
    synthetic_token: u32,
) -> u64 {
    let synthetic_decimals: &u8 = DECIMALS_PER_ASSET
        .get(synthetic_token.to_string().as_str())
        .unwrap();

    let synthetic_price_decimals: &u8 = PRICE_DECIMALS_PER_ASSET
        .get(synthetic_token.to_string().as_str())
        .unwrap();

    // ! Stable coins have decimal places hardcoded to 6 for now
    let dec_conversion1: i8 = *synthetic_price_decimals as i8 - COLLATERAL_TOKEN_DECIMALS as i8
        + *synthetic_decimals as i8;
    let multiplier1 = 10_u128.pow(dec_conversion1 as u32);

    if *order_side == OrderSide::Long {
        if size == 0 {
            return 0;
        }

        return entry_price
            .checked_sub((margin as u128 * multiplier1 / size as u128) as u64)
            .unwrap_or(0);
    } else {
        if size == 0 {
            return 1_000_000_000 * 10_u64.pow(*synthetic_price_decimals as u32);
        }

        let bp = entry_price + (margin as u128 * multiplier1 / size as u128) as u64;
        return bp;
    }
}
