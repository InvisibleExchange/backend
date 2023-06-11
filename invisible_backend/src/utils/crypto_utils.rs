use std::str::FromStr;

use num_bigint::{BigInt, BigUint};
use starknet::core::{
    crypto::{
        compute_hash_on_elements, ecdsa_verify, pedersen_hash, Signature as StarknetSignature,
    },
    types::FieldElement,
};

use starknet::curve::AffinePoint;

pub fn pedersen(a: &BigUint, b: &BigUint) -> BigUint {
    let left = FieldElement::from_dec_str(&a.to_string()).unwrap();
    let right = FieldElement::from_dec_str(&b.to_string()).unwrap();

    let res = pedersen_hash(&left, &right);

    let hash = BigUint::from_str(&res.to_string()).unwrap();

    return hash;
}

pub fn pedersen_on_vec(arr: &Vec<&BigUint>) -> BigUint {
    let input = arr
        .iter()
        .map(|el| FieldElement::from_dec_str(&el.to_string()).unwrap())
        .collect::<Vec<FieldElement>>();
    let input: &[FieldElement] = &input.as_slice();

    let res = compute_hash_on_elements(input);

    let hash = BigUint::from_str(&res.to_string()).unwrap();

    return hash;
}

pub fn verify(stark_key: &BigUint, msg_hash: &BigUint, signature: &Signature) -> bool {
    match ecdsa_verify(
        &FieldElement::from_dec_str(&stark_key.to_string()).unwrap(),
        &FieldElement::from_dec_str(&msg_hash.to_string()).unwrap(),
        &signature.to_starknet_signature(),
    ) {
        Ok(valid) => {
            return valid;
        }
        Err(_) => {
            return false;
        }
    }
}

// * STRUCTS ======================================================================================

use serde::ser::{Serialize, SerializeStruct, SerializeTuple, Serializer};

#[derive(Debug, Clone)]
pub struct Signature {
    pub r: String,
    pub s: String,
}

// * SERIALIZE * //
impl Serialize for Signature {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut sig = serializer.serialize_tuple(2)?;

        sig.serialize_element(&self.r)?;
        sig.serialize_element(&self.s)?;

        return sig.end();
    }
}

impl Signature {
    fn to_starknet_signature(&self) -> StarknetSignature {
        return StarknetSignature {
            r: FieldElement::from_dec_str(&self.r.to_string()).unwrap(),
            s: FieldElement::from_dec_str(&self.s.to_string()).unwrap(),
        };
    }
}

#[derive(Debug, Clone)]
pub struct EcPoint {
    pub x: BigInt,
    pub y: BigInt,
}

impl EcPoint {
    pub fn new(x: &BigUint, y: &BigUint) -> EcPoint {
        return EcPoint {
            x: BigInt::from_str(&x.to_string()).unwrap(),
            y: BigInt::from_str(&y.to_string()).unwrap(),
        };
    }
}

impl From<&EcPoint> for AffinePoint {
    fn from(p: &EcPoint) -> Self {
        if p.x == BigInt::from(0) {
            return AffinePoint::identity();
        }

        AffinePoint {
            x: FieldElement::from_dec_str(&p.x.to_string()).unwrap(),
            y: FieldElement::from_dec_str(&p.y.to_string()).unwrap(),
            infinity: false,
        }
    }
}

impl From<&AffinePoint> for EcPoint {
    fn from(p: &AffinePoint) -> Self {
        EcPoint {
            x: BigInt::from_str(&p.x.to_string()).unwrap(),
            y: BigInt::from_str(&p.y.to_string()).unwrap(),
        }
    }
}

impl Serialize for EcPoint {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut note = serializer.serialize_struct("EcPoint", 2)?;

        note.serialize_field("x", &self.x.to_string())?;
        note.serialize_field("y", &self.y.to_string())?;

        return note.end();
    }
}
