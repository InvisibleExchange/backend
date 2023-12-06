use std::str::FromStr;

use error_stack::{Report, Result};
use num_bigint::{BigInt, BigUint};

use crate::{
    perpetual::{
        perp_order::CloseOrderFields,
        perp_position::{PerpPosition, PositionHeader, _hash_position},
        OrderSide, COLLATERAL_TOKEN,
    },
    smart_contract_mms::vlp_note::VlpNote,
    transaction_batch::tx_batch_structs::OracleUpdate,
    utils::crypto_utils::{EcPoint, Signature},
    utils::{errors::GrpcMessageError, notes::Note},
};

use super::{
    engine_proto::{
        Address, GrcpPositionHeader, GrpcNote, GrpcOracleUpdate, GrpcPerpPosition, GrpcVlpNote,
        MarginChangeReq, Signature as GrpcSignature,
    },
    ChangeMarginMessage,
};

// POSITIONS
impl From<PerpPosition> for GrpcPerpPosition {
    fn from(req: PerpPosition) -> Self {
        let pos_header = GrcpPositionHeader {
            synthetic_token: req.position_header.synthetic_token,
            allow_partial_liquidations: req.position_header.allow_partial_liquidations,
            position_address: req.position_header.position_address.to_string(),
            max_vlp_supply: req.position_header.max_vlp_supply,
            vlp_token: req.position_header.vlp_token,
        };

        GrpcPerpPosition {
            order_side: if req.order_side == OrderSide::Long {
                1
            } else {
                0
            },
            position_size: req.position_size,
            position_header: Some(pos_header),
            margin: req.margin,
            entry_price: req.entry_price,
            liquidation_price: req.liquidation_price,
            bankruptcy_price: req.bankruptcy_price,
            last_funding_idx: req.last_funding_idx,
            index: req.index,
            vlp_supply: req.vlp_supply,
            hash: req.hash.to_string(),
        }
    }
}

impl TryFrom<GrpcPerpPosition> for PerpPosition {
    type Error = Report<GrpcMessageError>;

    fn try_from(req: GrpcPerpPosition) -> Result<Self, GrpcMessageError> {
        let order_side = if req.order_side == 1 {
            OrderSide::Long
        } else {
            OrderSide::Short
        };

        let pos_header = req.position_header.ok_or(GrpcMessageError {})?;

        let position_address =
            BigUint::from_str(&pos_header.position_address).map_err(|_| GrpcMessageError {})?;

        let position_header = PositionHeader::new(
            pos_header.synthetic_token,
            pos_header.allow_partial_liquidations,
            position_address,
            pos_header.vlp_token,
            pos_header.max_vlp_supply,
        );

        let position_hash = _hash_position(
            &position_header.hash,
            &order_side,
            req.position_size,
            req.entry_price,
            req.liquidation_price,
            req.last_funding_idx,
            req.vlp_supply,
        );

        let position = PerpPosition {
            position_header,
            order_side,
            position_size: req.position_size,
            margin: req.margin,
            entry_price: req.entry_price,
            liquidation_price: req.liquidation_price,
            bankruptcy_price: req.bankruptcy_price,
            last_funding_idx: req.last_funding_idx,
            index: req.index,
            vlp_supply: req.vlp_supply,
            hash: position_hash,
        };

        Ok(position)
    }
}

// ————————————————————————————————————————————————————————————————————————————————————————————————————————

// ActiveOrders

// ————————————————————————————————————————————————————————————————————————————————————————————————————————

// ------ UTILS -------------------------------------------

impl TryFrom<Address> for EcPoint {
    type Error = Report<GrpcMessageError>;

    fn try_from(req: Address) -> Result<Self, GrpcMessageError> {
        let point = EcPoint {
            x: BigInt::from_str(req.x.as_str())
                .ok()
                .ok_or(GrpcMessageError {})?,
            y: BigInt::from_str(req.y.as_str())
                .ok()
                .ok_or(GrpcMessageError {})?,
        };

        Ok(point)
    }
}

impl TryFrom<GrpcNote> for Note {
    type Error = Report<GrpcMessageError>;

    fn try_from(req: GrpcNote) -> Result<Self, GrpcMessageError> {
        let note = Note::new(
            req.index,
            EcPoint::try_from(req.address.ok_or(GrpcMessageError {})?)?,
            req.token,
            req.amount,
            BigUint::from_str(req.blinding.as_str())
                .ok()
                .ok_or(GrpcMessageError {})?,
        );

        Ok(note)
    }
}

impl TryFrom<GrpcVlpNote> for VlpNote {
    type Error = Report<GrpcMessageError>;

    fn try_from(req: GrpcVlpNote) -> Result<Self, GrpcMessageError> {
        let note = VlpNote::new(
            req.index,
            EcPoint::try_from(req.address.ok_or(GrpcMessageError {})?)?,
            req.token,
            req.amount,
            req.initial_value,
            BigUint::from_str(req.blinding.as_str())
                .ok()
                .ok_or(GrpcMessageError {})?,
        );

        Ok(note)
    }
}

impl TryFrom<GrpcSignature> for Signature {
    type Error = Report<GrpcMessageError>;

    fn try_from(req: GrpcSignature) -> Result<Self, GrpcMessageError> {
        let sig = Signature {
            r: req.r.to_string(),
            s: req.s.to_string(),
        };

        Ok(sig)
    }
}

// —————————————————————————————————————

impl From<Note> for GrpcNote {
    fn from(req: Note) -> Self {
        GrpcNote {
            index: req.index,
            address: Some(Address {
                x: req.address.x.to_str_radix(10),
                y: req.address.y.to_str_radix(10),
            }),
            token: req.token,
            amount: req.amount,
            blinding: req.blinding.to_str_radix(10),
        }
    }
}

impl From<VlpNote> for GrpcVlpNote {
    fn from(req: VlpNote) -> Self {
        GrpcVlpNote {
            index: req.index,
            address: Some(Address {
                x: req.address.x.to_str_radix(10),
                y: req.address.y.to_str_radix(10),
            }),
            token: req.token,
            amount: req.amount,
            initial_value: req.initial_value,
            blinding: req.blinding.to_str_radix(10),
        }
    }
}

impl From<EcPoint> for Address {
    fn from(req: EcPoint) -> Self {
        Address {
            x: req.x.to_string(),
            y: req.y.to_string(),
        }
    }
}

// ...........................

impl TryFrom<MarginChangeReq> for ChangeMarginMessage {
    type Error = Report<GrpcMessageError>;

    fn try_from(req: MarginChangeReq) -> Result<Self, GrpcMessageError> {
        // position
        if req.position.is_none() {
            return Err(Report::new(GrpcMessageError {}));
        }
        let position = PerpPosition::try_from(req.position.ok_or(GrpcMessageError {})?)?;

        // signature
        if req.signature.is_none() {
            return Err(Report::new(GrpcMessageError {}));
        }
        let sig = Signature::try_from(req.signature.ok_or(GrpcMessageError {})?)?;

        // notes and close order fields
        let notes_in: Option<Vec<Note>>;
        let refund_note: Option<Note>;
        let close_order_fields: Option<CloseOrderFields>;
        if req.margin_change >= 0 {
            let mut notes_in_: Vec<Note> = Vec::new();
            for n in req.notes_in.iter() {
                let note = Note::try_from(n.clone())?;

                if COLLATERAL_TOKEN != note.token {
                    return Err(Report::new(GrpcMessageError {}));
                }

                notes_in_.push(note);
            }
            if req.refund_note.is_none() {
                refund_note = None;
            } else {
                let ref_note = Note::try_from(req.refund_note.ok_or(GrpcMessageError {})?)?;

                if COLLATERAL_TOKEN != ref_note.token {
                    return Err(Report::new(GrpcMessageError {}));
                }

                refund_note = Some(ref_note);
            }

            notes_in = Some(notes_in_);
            close_order_fields = None;
        } else {
            if req.close_order_fields.is_none() {
                return Err(Report::new(GrpcMessageError {}));
            } else {
                let close_order_fields_ =
                    CloseOrderFields::try_from(req.close_order_fields.ok_or(GrpcMessageError {})?)?;
                close_order_fields = Some(close_order_fields_);
            }

            notes_in = None;
            refund_note = None;
        }

        Ok(ChangeMarginMessage {
            margin_change: req.margin_change,
            notes_in,
            refund_note,
            close_order_fields,
            position,
            signature: sig,
            user_id: req.user_id,
        })
    }
}

// ...........................

impl TryFrom<GrpcOracleUpdate> for OracleUpdate {
    type Error = Report<GrpcMessageError>;

    fn try_from(req: GrpcOracleUpdate) -> Result<Self, GrpcMessageError> {
        let mut signatures: Vec<Signature> = Vec::new();
        for s in req.signatures.iter() {
            let sig = Signature::try_from(s.clone())?;

            signatures.push(sig);
        }

        let point = OracleUpdate {
            token: req.token,
            timestamp: req.timestamp,
            observer_ids: req.observer_ids,
            prices: req.prices,
            signatures,
        };

        Ok(point)
    }
}
