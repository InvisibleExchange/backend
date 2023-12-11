use std::str::FromStr;

use num_bigint::{BigInt, BigUint};
use num_traits::FromPrimitive;
use serde::Deserialize as DeserializeTrait;

use crate::utils::crypto_utils::{hash, hash_many, EcPoint};

#[derive(Debug, Clone)]
pub struct Note {
    pub index: u64,
    pub address: EcPoint,
    pub token: u32,
    pub amount: u64,
    pub blinding: BigUint,
    pub hash: BigUint,
}

impl Note {
    pub fn new(
        index: u64,
        address: EcPoint, //address_pk
        token: u32,
        amount: u64,
        blinding: BigUint,
    ) -> Note {
        let note_hash = hash_note(amount, &blinding, token, &address);

        Note {
            index,
            address, //address_pk
            token,
            amount,
            blinding,
            hash: note_hash,
        }
    }
}

fn hash_note(amount: u64, blinding: &BigUint, token: u32, address: &EcPoint) -> BigUint {
    if amount == 0 {
        return BigUint::from_i8(0).unwrap();
    }

    let commitment = hash(&BigUint::from_u64(amount).unwrap(), blinding);

    let address_x = address.x.to_biguint().unwrap();
    let token = BigUint::from_u32(token).unwrap();
    let hash_input = vec![&address_x, &token, &commitment];

    let note_hash = hash_many(&hash_input);

    return note_hash;
}

// * SERIALIZE * //
use serde::ser::{Serialize, SerializeStruct, Serializer};

impl Serialize for Note {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut note = serializer.serialize_struct("Note", 6)?;

        note.serialize_field("index", &self.index)?;
        note.serialize_field("address", &self.address)?;
        note.serialize_field("token", &self.token)?;
        note.serialize_field("amount", &self.amount)?;
        note.serialize_field("blinding", &self.blinding.to_string())?;
        note.serialize_field("hash", &self.hash.to_string())?;

        return note.end();
    }
}

// * DESERIALIZE * //
use serde::de::{Deserialize, Deserializer};

impl<'de> Deserialize<'de> for Note {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(DeserializeTrait)]
        struct Addr {
            x: String,
            y: String,
        }

        #[derive(DeserializeTrait)]
        struct Helper {
            index: u64,
            address: Addr,
            token: u32,
            amount: u64,
            blinding: String,
            hash: String,
        }

        let helper = Helper::deserialize(deserializer)?;

        let x = BigInt::from_str(&helper.address.x).unwrap();
        let y = BigInt::from_str(&helper.address.y).unwrap();
        Ok(Note {
            index: helper.index,
            address: EcPoint { x, y },
            token: helper.token,
            amount: helper.amount,
            blinding: BigUint::from_str(&helper.blinding).unwrap(),
            hash: BigUint::from_str(&helper.hash).unwrap(),
        })
    }
}

pub fn biguint_to_32vec(a: &BigUint) -> [u8; 32] {
    let mut a_bytes = a.to_bytes_le();

    a_bytes.append(&mut vec![0; 32 - a_bytes.len()]);

    let a_vec: [u8; 32] = a_bytes.try_into().unwrap();

    return a_vec;
}
