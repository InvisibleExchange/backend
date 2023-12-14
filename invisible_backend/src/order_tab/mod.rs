use std::str::FromStr;

use num_bigint::BigUint;

use crate::utils::crypto_utils::hash;
use crate::utils::crypto_utils::hash_many;

pub mod close_tab;
pub mod db_updates;
pub mod json_output;
pub mod open_tab;
pub mod state_updates;

#[derive(Debug, Clone)]
pub struct OrderTab {
    pub tab_idx: u32,
    //
    pub tab_header: TabHeader,
    pub base_amount: u64,
    pub quote_amount: u64,
    //
    pub hash: BigUint,
}

impl OrderTab {
    pub fn new(tab_header: TabHeader, base_amount: u64, quote_amount: u64) -> OrderTab {
        let hash = hash_tab(&tab_header, base_amount, quote_amount);

        OrderTab {
            tab_idx: 0,
            tab_header,
            base_amount,
            quote_amount,
            hash,
        }
    }

    pub fn update_hash(&mut self) {
        let new_hash = hash_tab(&self.tab_header, self.base_amount, self.quote_amount);

        self.hash = new_hash;
    }
}

fn hash_tab(tab_header: &TabHeader, base_amount: u64, quote_amount: u64) -> BigUint {
    let mut hash_inputs: Vec<&BigUint> = Vec::new();

    // & header_hash = H({ base_token, quote_token, pub_key})
    // & H({header_hash, base_commitment, quote_commitment})

    hash_inputs.push(&tab_header.hash);

    let base_commitment = hash(&BigUint::from(base_amount), &tab_header.base_blinding);
    hash_inputs.push(&base_commitment);

    let quote_commitment = hash(&BigUint::from(quote_amount), &tab_header.quote_blinding);
    hash_inputs.push(&quote_commitment);

    let tab_hash = hash_many(&hash_inputs);

    return tab_hash;
}

#[derive(Debug, Clone)]
pub struct TabHeader {
    pub base_token: u32,
    pub quote_token: u32,
    pub base_blinding: BigUint,
    pub quote_blinding: BigUint,
    pub pub_key: BigUint,
    //
    pub hash: BigUint,
}

impl TabHeader {
    pub fn new(
        base_token: u32,
        quote_token: u32,
        base_blinding: BigUint,
        quote_blinding: BigUint,
        pub_key: BigUint,
    ) -> TabHeader {
        let hash = hash_header(base_token, quote_token, &pub_key);

        TabHeader {
            base_token,
            quote_token,
            base_blinding,
            quote_blinding,
            pub_key,
            hash,
        }
    }

    pub fn update_hash(&mut self) {
        let new_hash = hash_header(self.base_token, self.quote_token, &self.pub_key);

        self.hash = new_hash;
    }
}

fn hash_header(base_token: u32, quote_token: u32, pub_key: &BigUint) -> BigUint {
    let mut hash_inputs: Vec<&BigUint> = Vec::new();

    // & header_hash = H({ base_token, quote_token, pub_key})
    let base_token = BigUint::from(base_token);
    hash_inputs.push(&base_token);
    let quote_token = BigUint::from(quote_token);
    hash_inputs.push(&quote_token);

    hash_inputs.push(&pub_key);

    let order_hash = hash_many(&hash_inputs);

    return order_hash;
}

// * EXECUTION LOGIC ======================================================================================================

// * SERIALIZE  ==========================================================================================

use serde::ser::{Serialize, SerializeStruct, Serializer};
impl Serialize for OrderTab {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut order_tab = serializer.serialize_struct("OrderTab", 5)?;

        order_tab.serialize_field("tab_idx", &self.tab_idx)?;
        order_tab.serialize_field("tab_header", &self.tab_header)?;
        order_tab.serialize_field("base_amount", &self.base_amount)?;
        order_tab.serialize_field("quote_amount", &self.quote_amount)?;
        order_tab.serialize_field("hash", &self.hash.to_string())?;

        return order_tab.end();
    }
}

impl Serialize for TabHeader {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut tab_header = serializer.serialize_struct("TabHeader", 8)?;

        tab_header.serialize_field("base_token", &self.base_token)?;
        tab_header.serialize_field("quote_token", &self.quote_token)?;
        tab_header.serialize_field("base_blinding", &self.base_blinding.to_string())?;
        tab_header.serialize_field("quote_blinding", &self.quote_blinding.to_string())?;
        tab_header.serialize_field("pub_key", &self.pub_key.to_string())?;
        tab_header.serialize_field("hash", &self.hash.to_string())?;

        return tab_header.end();
    }
}

// * DESERIALIZE * //
use serde::de::{Deserialize, Deserializer};
use serde::Deserialize as DeserializeTrait;

impl<'de> Deserialize<'de> for TabHeader {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(DeserializeTrait)]
        struct Helper {
            base_token: u32,
            quote_token: u32,
            base_blinding: String,
            quote_blinding: String,
            pub_key: String,
            hash: String,
        }

        let helper = Helper::deserialize(deserializer)?;
        Ok(TabHeader {
            base_token: helper.base_token,
            quote_token: helper.quote_token,
            base_blinding: BigUint::from_str(helper.base_blinding.as_str())
                .map_err(|err| serde::de::Error::custom(err.to_string()))?,
            quote_blinding: BigUint::from_str(helper.quote_blinding.as_str())
                .map_err(|err| serde::de::Error::custom(err.to_string()))?,
            pub_key: BigUint::from_str(helper.pub_key.as_str())
                .map_err(|err| serde::de::Error::custom(err.to_string()))?,
            hash: BigUint::from_str(helper.hash.as_str())
                .map_err(|err| serde::de::Error::custom(err.to_string()))?,
        })
    }
}

impl<'de> Deserialize<'de> for OrderTab {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(DeserializeTrait)]
        struct Helper {
            tab_idx: u32,
            tab_header: TabHeader,
            base_amount: u64,
            quote_amount: u64,
            hash: String,
        }

        let helper = Helper::deserialize(deserializer)?;
        Ok(OrderTab {
            tab_idx: helper.tab_idx,
            tab_header: helper.tab_header,
            base_amount: helper.base_amount,
            quote_amount: helper.quote_amount,
            hash: BigUint::from_str(helper.hash.as_str())
                .map_err(|err| serde::de::Error::custom(err.to_string()))?,
        })
    }
}
