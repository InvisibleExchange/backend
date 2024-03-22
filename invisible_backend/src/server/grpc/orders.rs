use std::{str::FromStr, sync::Arc};

use error_stack::{Report, Result};
use num_bigint::BigUint;
use parking_lot::Mutex;

use crate::{
    order_tab::{OrderTab, TabHeader},
    perpetual::{
        liquidations::liquidation_order::LiquidationOrder,
        perp_order::{CloseOrderFields, OpenOrderFields, PerpOrder},
        perp_position::PerpPosition,
        OrderSide,
    },
    transactions::{
        deposit::Deposit,
        limit_order::{LimitOrder, SpotNotesInfo},
        withdrawal::Withdrawal,
    },
    utils::crypto_utils::{EcPoint, Signature},
    utils::{errors::GrpcMessageError, notes::Note},
};

// use super::engine_proto::{

// };

use super::engine_proto::{
    DepositMessage, GrpcCloseOrderFields, GrpcOpenOrderFields, GrpcOrderTab, GrpcTabHeader,
    LimitOrderMessage, LiquidationOrderMessage, PerpOrderMessage, SpotNotesInfoMessage,
    WithdrawalMessage,
};

// ------ DEPOSITS -------------------------------------------

impl TryFrom<DepositMessage> for Deposit {
    type Error = Report<GrpcMessageError>;

    fn try_from(req: DepositMessage) -> Result<Self, GrpcMessageError> {
        let mut notes: Vec<Note> = Vec::new();
        for n in req.notes.iter() {
            let note = Note::try_from(n.clone())?;

            notes.push(note);
        }

        let deposit = Deposit {
            transaction_type: "deposit".to_string(),
            deposit_id: req.deposit_id,
            deposit_token: req.deposit_token,
            deposit_amount: req.deposit_amount,
            notes,
            signature: Signature::try_from(req.signature.ok_or(GrpcMessageError {})?)?,
            stark_key: BigUint::from_str(&req.stark_key)
                .ok()
                .ok_or(GrpcMessageError {})?,
        };

        Ok(deposit)
    }
}

// ------ SPOT SWAPS -------------------------------------------

impl TryFrom<LimitOrderMessage> for LimitOrder {
    type Error = Report<GrpcMessageError>;

    fn try_from(req: LimitOrderMessage) -> Result<Self, GrpcMessageError> {
        let spot_note_info: Option<SpotNotesInfo> = if req.spot_note_info.is_some() {
            let notes_info =
                SpotNotesInfo::try_from(req.spot_note_info.ok_or(GrpcMessageError {})?)?;
            Some(notes_info)
        } else {
            None
        };

        let order_tab = if req.order_tab.is_some() {
            let tab = OrderTab::try_from(req.order_tab.ok_or(GrpcMessageError {})?)?;
            Some(Arc::new(Mutex::new(tab)))
        } else {
            None
        };

        let limit_order = LimitOrder::new(
            0,
            req.expiration_timestamp,
            req.token_spent,
            req.token_received,
            req.amount_spent,
            req.amount_received,
            req.fee_limit,
            spot_note_info,
            order_tab,
        );

        Ok(limit_order)
    }
}

impl TryFrom<SpotNotesInfoMessage> for SpotNotesInfo {
    type Error = Report<GrpcMessageError>;

    fn try_from(req: SpotNotesInfoMessage) -> Result<Self, GrpcMessageError> {
        let mut notes_in: Vec<Note> = Vec::new();
        for n in req.notes_in.iter() {
            let note = Note::try_from(n.clone())?;

            notes_in.push(note);
        }

        let refund_note: Option<Note>;
        if req.refund_note.is_some() {
            let n = Note::try_from(req.refund_note.ok_or(GrpcMessageError {})?)?;
            refund_note = Some(n);
        } else {
            refund_note = None
        }

        let spot_notes_info = SpotNotesInfo {
            dest_received_address: EcPoint::try_from(
                req.dest_received_address.ok_or(GrpcMessageError {})?,
            )?,
            dest_received_blinding: BigUint::from_str(req.dest_received_blinding.as_str())
                .ok()
                .ok_or(GrpcMessageError {})?,
            notes_in,
            refund_note,
        };

        Ok(spot_notes_info)
    }
}

impl TryFrom<GrpcOrderTab> for OrderTab {
    type Error = Report<GrpcMessageError>;

    fn try_from(req: GrpcOrderTab) -> Result<Self, GrpcMessageError> {
        let tab_header = TabHeader::try_from(req.tab_header.ok_or(GrpcMessageError {})?)?;

        let mut order_tab = OrderTab::new(tab_header, req.base_amount, req.quote_amount);
        order_tab.tab_idx = req.tab_idx;

        Ok(order_tab)
    }
}

impl TryFrom<GrpcTabHeader> for TabHeader {
    type Error = Report<GrpcMessageError>;

    fn try_from(req: GrpcTabHeader) -> Result<Self, GrpcMessageError> {
        let header = TabHeader::new(
            req.base_token,
            req.quote_token,
            BigUint::from_str(&req.base_blinding).unwrap_or_default(),
            BigUint::from_str(&req.quote_blinding).unwrap_or_default(),
            BigUint::from_str(&req.pub_key).unwrap_or_default(),
        );

        Ok(header)
    }
}

impl From<OrderTab> for GrpcOrderTab {
    fn from(req: OrderTab) -> Self {
        let header = GrpcTabHeader {
            base_token: req.tab_header.base_token,
            quote_token: req.tab_header.quote_token,
            base_blinding: req.tab_header.base_blinding.to_string(),
            quote_blinding: req.tab_header.quote_blinding.to_string(),
            pub_key: req.tab_header.pub_key.to_string(),
        };

        let order_tab = GrpcOrderTab {
            tab_idx: req.tab_idx,
            tab_header: Some(header),
            base_amount: req.base_amount,
            quote_amount: req.quote_amount,
        };

        return order_tab;
    }
}

// ------ WITHDRAWALS -------------------------------------------

impl TryFrom<WithdrawalMessage> for Withdrawal {
    type Error = Report<GrpcMessageError>;

    fn try_from(req: WithdrawalMessage) -> Result<Self, GrpcMessageError> {
        let mut notes_in: Vec<Note> = Vec::new();
        for n in req.notes_in.iter() {
            let note = Note::try_from(n.clone())?;

            notes_in.push(note);
        }

        let refund_note: Option<Note>;
        if req.refund_note.is_some() {
            let n = Note::try_from(req.refund_note.ok_or(GrpcMessageError {})?)?;
            refund_note = Some(n);
        } else {
            refund_note = None
        }

        let withdrawal = Withdrawal {
            transaction_type: "withdrawal".to_string(),
            withdrawal_id: req.withdrawal_id,
            chain_id: req.chain_id,
            token: req.token,
            amount: req.amount,
            max_gas_fee: req.max_gas_fee,
            execution_gas_fee: req.execution_gas_fee,
            notes_in,
            refund_note,
            signature: Signature::try_from(req.signature.ok_or(GrpcMessageError {})?)?,
            recipient: BigUint::from_str(&req.recipient).or_else(|e| {
                return Err(Report::new(GrpcMessageError {}).attach_printable(e));
            })?,
        };

        Ok(withdrawal)
    }
}

// ------ PERPETUAL SWAPS -------------------------------------------

impl TryFrom<PerpOrderMessage> for PerpOrder {
    type Error = Report<GrpcMessageError>;

    fn try_from(req: PerpOrderMessage) -> Result<Self, GrpcMessageError> {
        let result: PerpOrder;
        match req.position_effect_type {
            0 => {
                let open_order_fields =
                    OpenOrderFields::try_from(req.open_order_fields.ok_or(GrpcMessageError {})?)?;

                result = PerpOrder::new_open_order(
                    0,
                    req.expiration_timestamp,
                    if req.order_side {
                        OrderSide::Long
                    } else {
                        OrderSide::Short
                    },
                    req.synthetic_token,
                    req.synthetic_amount,
                    req.collateral_amount,
                    req.fee_limit,
                    open_order_fields,
                );
            }
            1 => {
                result = PerpOrder::new_modify_order(
                    0,
                    req.expiration_timestamp,
                    PerpPosition::try_from(req.position.ok_or(GrpcMessageError {})?)?,
                    if req.order_side {
                        OrderSide::Long
                    } else {
                        OrderSide::Short
                    },
                    req.synthetic_token,
                    req.synthetic_amount,
                    req.collateral_amount,
                    req.fee_limit,
                );
            }
            2 => {
                let close_order_fields =
                    CloseOrderFields::try_from(req.close_order_fields.ok_or(GrpcMessageError {})?)?;

                result = PerpOrder::new_close_order(
                    0,
                    req.expiration_timestamp,
                    PerpPosition::try_from(req.position.ok_or(GrpcMessageError {})?)?,
                    if req.order_side {
                        OrderSide::Long
                    } else {
                        OrderSide::Short
                    },
                    req.synthetic_token,
                    req.synthetic_amount,
                    req.collateral_amount,
                    req.fee_limit,
                    close_order_fields,
                );

                if result.position.is_none() || result.position.as_ref().unwrap().vlp_supply > 0 {
                    return Err(Report::new(GrpcMessageError {})
                        .attach("Invalid position, can't close a position with vlp tokens"));
                }
            }
            _ => {
                return Err(Report::new(GrpcMessageError {}).attach("Invalid position effect type"))
            }
        }

        Ok(result)
    }
}

impl TryFrom<GrpcOpenOrderFields> for OpenOrderFields {
    type Error = Report<GrpcMessageError>;

    fn try_from(req: GrpcOpenOrderFields) -> Result<Self, GrpcMessageError> {
        let mut notes_in: Vec<Note> = Vec::new();
        for n in req.notes_in.iter() {
            let note = Note::try_from(n.clone())?;

            notes_in.push(note);
        }

        let refund_note: Option<Note>;
        if req.refund_note.is_some() {
            let n = Note::try_from(req.refund_note.ok_or(GrpcMessageError {})?)?;
            refund_note = Some(n);
        } else {
            refund_note = None
        }

        let fields = OpenOrderFields {
            initial_margin: req.initial_margin,
            collateral_token: req.collateral_token,
            notes_in,
            refund_note,
            position_address: BigUint::from_str(&req.position_address)
                .map_err(|_| GrpcMessageError {})?,
            allow_partial_liquidations: req.allow_partial_liquidations,
        };

        Ok(fields)
    }
}

impl TryFrom<GrpcCloseOrderFields> for CloseOrderFields {
    type Error = Report<GrpcMessageError>;

    fn try_from(req: GrpcCloseOrderFields) -> Result<Self, GrpcMessageError> {
        let fields = CloseOrderFields {
            dest_received_address: EcPoint::try_from(
                req.dest_received_address.ok_or(GrpcMessageError {})?,
            )?,
            dest_received_blinding: BigUint::from_str(&req.dest_received_blinding.as_str())
                .ok()
                .ok_or(GrpcMessageError {})?,
        };

        Ok(fields)
    }
}

// LIQUIDATION ORDER

impl TryFrom<LiquidationOrderMessage> for LiquidationOrder {
    type Error = Report<GrpcMessageError>;

    fn try_from(req: LiquidationOrderMessage) -> Result<Self, GrpcMessageError> {
        let open_order_fields =
            OpenOrderFields::try_from(req.open_order_fields.ok_or(GrpcMessageError {})?)?;
        let position = PerpPosition::try_from(req.position.ok_or(GrpcMessageError {})?)?;

        let result = LiquidationOrder::new(
            position,
            if req.order_side {
                OrderSide::Long
            } else {
                OrderSide::Short
            },
            req.synthetic_token,
            req.synthetic_amount,
            req.collateral_amount,
            open_order_fields,
        );

        return Ok(result);
    }
}
