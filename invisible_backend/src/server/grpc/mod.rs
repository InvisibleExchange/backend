pub mod engine_proto {
    tonic::include_proto!("engine");
}

use std::{collections::HashMap, thread::JoinHandle};

use error_stack::Result;
use serde::Serialize;

use crate::{
    order_tab::OrderTab,
    perpetual::{
        liquidations::liquidation_output::LiquidationResponse,
        perp_helpers::perp_swap_outptut::PerpSwapResponse, perp_order::CloseOrderFields,
        perp_position::PerpPosition,
    },
    smart_contract_mms::remove_liquidity::RemoveLiqRes,
    transactions::swap::SwapResponse,
    utils::crypto_utils::Signature,
    utils::{
        errors::{PerpSwapExecutionError, TransactionExecutionError},
        notes::Note,
    },
};

use self::engine_proto::{
    CloseOrderTabReq, OnChainAddLiqTabReq, OnChainRegisterMmReq, OnChainRemoveLiqTabReq,
    OpenOrderTabReq,
};

pub mod helpers;
pub mod orders;

#[derive(Debug, Default)]
pub struct GrpcTxResponse {
    pub tx_handle: Option<
        JoinHandle<Result<(Option<SwapResponse>, Option<Vec<u64>>), TransactionExecutionError>>,
    >,
    pub perp_tx_handle: Option<JoinHandle<Result<PerpSwapResponse, PerpSwapExecutionError>>>,
    pub liquidation_tx_handle:
        Option<JoinHandle<Result<LiquidationResponse, PerpSwapExecutionError>>>,
    pub margin_change_response: Option<(Option<MarginChangeResponse>, String)>, //
    pub order_tab_action_response: Option<JoinHandle<OrderTabActionResponse>>,
    pub new_idxs: Option<std::result::Result<Vec<u64>, String>>, // For deposit orders
    pub funding_info: Option<(HashMap<u32, Vec<i64>>, HashMap<u32, Vec<u64>>)>, // (funding_rates, funding_prices, latest_funding_idx)
    pub successful: bool,
}

impl GrpcTxResponse {
    pub fn new(successful: bool) -> GrpcTxResponse {
        GrpcTxResponse {
            successful,
            ..Default::default()
        }
    }
}

// * CONTROL ENGINE ======================================================================

#[derive(Debug)]
pub struct MarginChangeResponse {
    pub new_note_idx: u64,
    pub position: PerpPosition,
}

// * ===================================================================================

#[derive(Clone)]
pub struct FundingUpdateMessage {
    pub impact_prices: HashMap<u32, (u64, u64)>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ChangeMarginMessage {
    pub margin_change: i64,
    pub notes_in: Option<Vec<Note>>,
    pub refund_note: Option<Note>,
    pub close_order_fields: Option<CloseOrderFields>,
    pub position: PerpPosition,
    pub signature: Signature,
    pub user_id: u64,
}

pub struct OrderTabActionMessage {
    pub open_order_tab_req: Option<OpenOrderTabReq>,
    pub close_order_tab_req: Option<CloseOrderTabReq>,
    pub onchain_register_mm_req: Option<OnChainRegisterMmReq>,
    pub onchain_add_liq_req: Option<OnChainAddLiqTabReq>,
    pub onchain_remove_liq_req: Option<OnChainRemoveLiqTabReq>,
}

pub struct OrderTabActionResponse {
    pub open_tab_response: Option<std::result::Result<OrderTab, String>>,
    pub close_tab_response: Option<std::result::Result<(Note, Note), String>>,
    pub register_mm_response:
        Option<std::result::Result<(Option<OrderTab>, Option<PerpPosition>, Note), String>>,
    pub add_liq_response:
        Option<std::result::Result<(Option<OrderTab>, Option<PerpPosition>, Note), String>>,
    pub remove_liq_response: Option<std::result::Result<RemoveLiqRes, String>>,
}
