use error_stack::Report;
use std::error::Error;
use std::fmt;

// * DEPOSIT ERRORS ------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct DepositThreadExecutionError {
    pub err_msg: String,
}

impl fmt::Display for DepositThreadExecutionError {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt.write_str("Error executing a deposit")
    }
}

impl Error for DepositThreadExecutionError {}

pub fn send_deposit_error(
    err_msg: String,
    attachment: Option<String>,
) -> Report<DepositThreadExecutionError> {
    println!("ERROR in deposit: {:?} \n{:?}", err_msg, attachment);
    let report = Report::new(DepositThreadExecutionError {
        err_msg: err_msg.clone(),
    })
    .attach_printable(attachment.unwrap_or(err_msg));

    return report;
}

// * SWAP ERRORS --------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct SwapThreadExecutionError {
    pub err_msg: String,
    pub invalid_order: Option<u64>,
}

impl fmt::Display for SwapThreadExecutionError {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt.write_str("Error executing a spot swap")
    }
}

impl Error for SwapThreadExecutionError {}

pub fn send_swap_error(
    err_msg: String,
    invalid_order: Option<u64>,
    attachment: Option<String>,
) -> Report<SwapThreadExecutionError> {
    println!("ERROR: in spot swap {:?} \n{:?}", err_msg, attachment);
    let report = Report::new(SwapThreadExecutionError {
        err_msg: err_msg.clone(),
        invalid_order,
    })
    .attach_printable(attachment.unwrap_or(err_msg));

    return report;
}

// * WITHDRAWAL ERRORS ----------------------------------------------------------

#[derive(Debug, Clone)]
pub struct WithdrawalThreadExecutionError {
    pub err_msg: String,
}

impl fmt::Display for WithdrawalThreadExecutionError {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt.write_str("Error executing a withdrawal")
    }
}

impl Error for WithdrawalThreadExecutionError {}

pub fn send_withdrawal_error(
    err_msg: String,
    attachment: Option<String>,
) -> Report<WithdrawalThreadExecutionError> {
    println!("ERROR in withdrawal: {:?} \n{:?}", err_msg, attachment);
    let report = Report::new(WithdrawalThreadExecutionError {
        err_msg: err_msg.clone(),
    })
    .attach_printable(attachment.unwrap_or(err_msg));

    return report;
}

// * TRANSACTION ERRORS ---------------------------------------------------------

#[derive(Debug)]
pub enum TransactionExecutionError {
    Deposit(DepositThreadExecutionError),
    Swap(SwapThreadExecutionError),
    Withdrawal(WithdrawalThreadExecutionError),
}

impl fmt::Display for TransactionExecutionError {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt.write_str("Error executing a transaction")
    }
}

impl Error for TransactionExecutionError {}

// * PERPETUAL SWAP ERRORS ------------------------------------------------------

#[derive(Debug)]
pub struct PerpSwapExecutionError {
    pub err_msg: String,
    pub invalid_order: Option<u64>,
}

impl fmt::Display for PerpSwapExecutionError {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt.write_str("Error executing a perpetual swap")
    }
}

impl Error for PerpSwapExecutionError {}

pub fn send_perp_swap_error(
    err_msg: String,
    invalid_order: Option<u64>, // The id if the order is invalid and shouldnt be retruned to the orderbook
    attachment: Option<String>,
) -> Report<PerpSwapExecutionError> {
    println!("ERROR in perp_swap: {:?} \n{:?}", err_msg, attachment);

    let report = Report::new(PerpSwapExecutionError {
        err_msg: err_msg.clone(),
        invalid_order,
    })
    .attach_printable(attachment.unwrap_or(err_msg));

    return report;
}

// * GRPC MESSAGE ERRORS ---------------------------------------------------------

#[derive(Debug)]
pub struct GrpcMessageError {}

impl fmt::Display for GrpcMessageError {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt.write_str(
            "Error converting the Grpc message, verify the data sent is in the correct fromat",
        )
    }
}

impl Error for GrpcMessageError {}

// * BATCH FINALIZATION -------------------------------------------------------------------

#[derive(Debug)]
pub struct BatchFinalizationError {}

impl fmt::Display for BatchFinalizationError {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt.write_str("Error finalizing the transaction batch")
    }
}

impl Error for BatchFinalizationError {}

// * ORACLE UPDATE ERRORS -------------------------------------------------------------------

#[derive(Debug)]
pub struct OracleUpdateError {
    pub err_msg: String,
}

impl fmt::Display for OracleUpdateError {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt.write_str("Error updating the index price")
    }
}

impl Error for OracleUpdateError {}

pub fn send_oracle_update_error(err_msg: String) -> Report<OracleUpdateError> {
    let report = Report::new(OracleUpdateError {
        err_msg: err_msg.clone(),
    })
    .attach_printable(err_msg);

    return report;
}

// * MATCHING ENGINE ERRORS -------------------------------------------------------------------

#[derive(Debug)]
pub struct MatchingEngineError {
    pub err_msg: String,
}

impl fmt::Display for MatchingEngineError {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt.write_str("Error matching order")
    }
}

impl Error for MatchingEngineError {}

pub fn send_matching_error(err_msg: String) -> Report<MatchingEngineError> {
    let report = Report::new(MatchingEngineError {
        err_msg: err_msg.clone(),
    })
    .attach_printable(err_msg);

    return report;
}

use tonic::{Response, Status};

use crate::server::grpc::engine_proto::{
    AmendOrderResponse, CancelOrderResponse, CloseOrderTabRes, DepositResponse, FundingRes,
    LiquidationOrderResponse, LiquidityRes, MarginChangeRes, OnChainScmmRes, OpenOrderTabRes,
    OrderResponse, SplitNotesRes, SuccessResponse,
};

// * ERROR GRPC REPLIES

pub fn send_order_error_reply(err_msg: String) -> Result<Response<OrderResponse>, Status> {
    let reply = OrderResponse {
        successful: false,
        error_message: err_msg,
        order_id: 0,
    };

    return Ok(Response::new(reply));
}

pub fn send_liquidation_order_error_reply(
    err_msg: String,
) -> Result<Response<LiquidationOrderResponse>, Status> {
    let reply = LiquidationOrderResponse {
        successful: false,
        error_message: err_msg,
        new_position: None,
    };

    return Ok(Response::new(reply));
}

pub fn send_cancel_order_error_reply(
    err_msg: String,
) -> Result<Response<CancelOrderResponse>, Status> {
    let reply = CancelOrderResponse {
        successful: false,
        error_message: err_msg,
        pfr_note: None,
    };

    return Ok(Response::new(reply));
}

pub fn send_amend_order_error_reply(
    err_msg: String,
) -> Result<Response<AmendOrderResponse>, Status> {
    let reply = AmendOrderResponse {
        successful: false,
        error_message: err_msg,
    };

    return Ok(Response::new(reply));
}

pub fn send_deposit_error_reply(err_msg: String) -> Result<Response<DepositResponse>, Status> {
    let reply = DepositResponse {
        successful: false,
        error_message: err_msg,
        zero_idxs: vec![],
    };

    return Ok(Response::new(reply));
}

pub fn send_withdrawal_error_reply(err_msg: String) -> Result<Response<SuccessResponse>, Status> {
    let reply = SuccessResponse {
        successful: false,
        error_message: err_msg,
    };

    return Ok(Response::new(reply));
}

pub fn send_liquidity_error_reply(err_msg: String) -> Result<Response<LiquidityRes>, Status> {
    let reply = LiquidityRes {
        successful: false,
        bid_queue: vec![],
        ask_queue: vec![],
        error_message: err_msg,
    };

    return Ok(Response::new(reply));
}

pub fn send_split_notes_error_reply(err_msg: String) -> Result<Response<SplitNotesRes>, Status> {
    let reply = SplitNotesRes {
        successful: false,
        error_message: err_msg,
        zero_idxs: vec![],
    };

    return Ok(Response::new(reply));
}

pub fn send_open_tab_reply(err_msg: String) -> Result<Response<SuccessResponse>, Status> {
    let reply = SuccessResponse {
        successful: false,
        error_message: err_msg,
    };

    return Ok(Response::new(reply));
}

pub fn send_margin_change_error_reply(
    err_msg: String,
) -> Result<Response<MarginChangeRes>, Status> {
    let reply = MarginChangeRes {
        successful: false,
        error_message: err_msg,
        return_collateral_index: 0,
    };

    return Ok(Response::new(reply));
}

pub fn send_open_tab_error_reply(err_msg: String) -> Result<Response<OpenOrderTabRes>, Status> {
    let reply = OpenOrderTabRes {
        successful: false,
        error_message: err_msg,
        order_tab: None,
    };

    return Ok(Response::new(reply));
}

//

pub fn send_regster_mm_error_reply(err_msg: String) -> Result<Response<OnChainScmmRes>, Status> {
    let reply = OnChainScmmRes {
        successful: false,
        error_message: err_msg,
        position: None,
    };

    return Ok(Response::new(reply));
}

pub fn send_add_liq_tab_error_reply(err_msg: String) -> Result<Response<OnChainScmmRes>, Status> {
    let reply = OnChainScmmRes {
        successful: false,
        error_message: err_msg,
        position: None,
    };

    return Ok(Response::new(reply));
}

pub fn send_remove_liq_tab_error_reply(
    err_msg: String,
) -> Result<Response<OnChainScmmRes>, Status> {
    let reply = OnChainScmmRes {
        successful: false,
        error_message: err_msg,
        position: None,
    };

    return Ok(Response::new(reply));
}

pub fn send_close_tab_error_reply(err_msg: String) -> Result<Response<CloseOrderTabRes>, Status> {
    let reply = CloseOrderTabRes {
        successful: false,
        error_message: err_msg,
        base_return_note: None,
        quote_return_note: None,
    };

    return Ok(Response::new(reply));
}

pub fn send_oracle_update_error_reply(
    err_msg: String,
) -> Result<Response<SuccessResponse>, Status> {
    let reply = SuccessResponse {
        successful: false,
        error_message: err_msg,
    };

    return Ok(Response::new(reply));
}

pub fn send_funding_error_reply(err_msg: String) -> Result<Response<FundingRes>, Status> {
    let reply = FundingRes {
        successful: false,
        fundings: vec![],
        error_message: err_msg,
    };

    return Ok(Response::new(reply));
}
