use std::{collections::HashMap, sync::Arc};

use self::{
    admin::{finalize_batch_inner, restore_orderbook_inner, update_index_price_inner},
    note_position_helpers::{change_position_margin_inner, split_notes_inner},
    onchain_interaction::{execute_deposit_inner, execute_withdrawal_inner},
    onchain_order_tabs::{
        add_liquidity_mm_inner, onchain_register_mm_inner, remove_liquidity_mm_inner,
    },
    order_executions::{
        submit_limit_order_inner, submit_liquidation_order_inner, submit_perpetual_order_inner,
    },
    order_interactions::{amend_order_inner, cancel_order_inner},
    order_tabs::{close_order_tab_inner, open_order_tab_inner},
    queries::{
        get_funding_info_inner, get_index_prices_inner, get_liquidity_inner, get_orders_inner,
        get_state_info_inner,
    },
};

use super::grpc::engine_proto::{
    AddLiqOrderTabRes, AmendOrderRequest, AmendOrderResponse, CancelOrderMessage,
    CancelOrderResponse, CloseOrderTabReq, DepositMessage, DepositResponse, EmptyReq,
    FinalizeBatchResponse, FundingReq, FundingRes, IndexPriceRes, LimitOrderMessage,
    LiquidationOrderMessage, LiquidationOrderResponse, LiquidityReq, LiquidityRes, MarginChangeReq,
    MarginChangeRes, OnChainAddLiqTabReq, OnChainRegisterMmReq, OnChainRegisterMmRes,
    OnChainRemoveLiqTabReq, OpenOrderTabReq, OracleUpdateReq, OrderResponse, OrdersReq, OrdersRes,
    PerpOrderMessage, RemoveLiqOrderTabRes, RestoreOrderBookMessage, SplitNotesReq, SplitNotesRes,
    StateInfoReq, StateInfoRes, SuccessResponse, WithdrawalMessage,
};
use super::{
    grpc::engine_proto::{engine_server::Engine, CloseOrderTabRes, OpenOrderTabRes},
    server_helpers::WsConnectionsMap,
};
use crate::transaction_batch::TransactionBatch;
use crate::{
    matching_engine::orderbook::OrderBook,
    utils::errors::{send_deposit_error_reply, send_oracle_update_error_reply},
};

use tokio::sync::{Mutex as TokioMutex, Semaphore};
use tonic::{Request, Response, Status};

mod admin;
mod note_position_helpers;
mod onchain_interaction;
mod onchain_order_tabs;
mod order_executions;
mod order_interactions;
mod order_tabs;
mod queries;

// #[derive(Debug)]
pub struct EngineService {
    pub transaction_batch: Arc<TokioMutex<TransactionBatch>>,
    //
    pub order_books: HashMap<u16, Arc<TokioMutex<OrderBook>>>,
    pub perp_order_books: HashMap<u16, Arc<TokioMutex<OrderBook>>>,
    //
    pub ws_connections: Arc<TokioMutex<WsConnectionsMap>>,
    pub privileged_ws_connections: Arc<TokioMutex<Vec<u64>>>,
    //
    pub semaphore: Semaphore,
    pub is_paused: Arc<TokioMutex<bool>>,
}

// #[tokio::]
#[tonic::async_trait]
impl Engine for EngineService {
    async fn submit_limit_order(
        &self,
        request: Request<LimitOrderMessage>,
    ) -> Result<Response<OrderResponse>, Status> {
        return submit_limit_order_inner(
            &self.transaction_batch,
            &self.order_books,
            &self.ws_connections,
            &self.privileged_ws_connections,
            &self.semaphore,
            &self.is_paused,
            request,
        )
        .await;
    }

    //
    // * ===================================================================================================================================
    //

    async fn submit_perpetual_order(
        &self,
        request: Request<PerpOrderMessage>,
    ) -> Result<Response<OrderResponse>, Status> {
        return submit_perpetual_order_inner(
            &self.transaction_batch,
            &self.perp_order_books,
            &self.ws_connections,
            &self.privileged_ws_connections,
            &self.semaphore,
            &self.is_paused,
            request,
        )
        .await;
    }

    //
    // * ===================================================================================================================================
    //

    async fn submit_liquidation_order(
        &self,
        request: Request<LiquidationOrderMessage>,
    ) -> Result<Response<LiquidationOrderResponse>, Status> {
        return submit_liquidation_order_inner(
            &self.transaction_batch,
            &self.perp_order_books,
            &self.semaphore,
            &self.is_paused,
            request,
        )
        .await;
    }

    //
    // * ===================================================================================================================================
    //

    async fn cancel_order(
        &self,
        request: Request<CancelOrderMessage>,
    ) -> Result<Response<CancelOrderResponse>, Status> {
        return cancel_order_inner(
            &self.transaction_batch,
            &self.order_books,
            &self.perp_order_books,
            request,
        )
        .await;
    }

    //
    // * ===================================================================================================================================
    //

    async fn amend_order(
        &self,
        request: Request<AmendOrderRequest>,
    ) -> Result<Response<AmendOrderResponse>, Status> {
        return amend_order_inner(
            &self.transaction_batch,
            &self.order_books,
            &self.perp_order_books,
            &self.ws_connections,
            &self.privileged_ws_connections,
            request,
        )
        .await;
    }

    //
    // * ===================================================================================================================================
    //

    async fn execute_deposit(
        &self,
        request: Request<DepositMessage>,
    ) -> Result<Response<DepositResponse>, Status> {
        // ? Only call the server from the same network (onyl as fallback)
        if request.remote_addr().unwrap().ip()
            != std::net::IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1))
        {
            return send_deposit_error_reply(
                "execute deposit can only be called from the same network".to_string(),
            );
        }

        return execute_deposit_inner(
            &self.transaction_batch,
            &self.semaphore,
            &self.is_paused,
            request,
        )
        .await;
    }

    //
    // * ===================================================================================================================================
    //

    async fn execute_withdrawal(
        &self,
        request: Request<WithdrawalMessage>,
    ) -> Result<Response<SuccessResponse>, Status> {
        return execute_withdrawal_inner(
            &self.transaction_batch,
            &self.semaphore,
            &self.is_paused,
            request,
        )
        .await;
    }

    //
    // * ===================================================================================================================================
    //

    async fn split_notes(
        &self,
        req: Request<SplitNotesReq>,
    ) -> Result<Response<SplitNotesRes>, Status> {
        return split_notes_inner(
            &self.transaction_batch,
            &self.semaphore,
            &self.is_paused,
            req,
        )
        .await;
    }

    //
    // * ===================================================================================================================================
    //

    async fn change_position_margin(
        &self,
        req: Request<MarginChangeReq>,
    ) -> Result<Response<MarginChangeRes>, Status> {
        return change_position_margin_inner(
            &self.transaction_batch,
            &self.perp_order_books,
            &self.ws_connections,
            &self.semaphore,
            &self.is_paused,
            req,
        )
        .await;
    }

    //
    // * ===================================================================================================================================
    //

    async fn open_order_tab(
        &self,
        req: Request<OpenOrderTabReq>,
    ) -> Result<Response<OpenOrderTabRes>, Status> {
        return open_order_tab_inner(
            &self.transaction_batch,
            &self.order_books,
            &self.semaphore,
            &self.is_paused,
            req,
        )
        .await;
    }

    //
    // * ===================================================================================================================================
    //

    async fn close_order_tab(
        &self,
        req: Request<CloseOrderTabReq>,
    ) -> Result<Response<CloseOrderTabRes>, Status> {
        return close_order_tab_inner(
            &self.transaction_batch,
            &self.semaphore,
            &self.is_paused,
            req,
        )
        .await;
    }

    //
    // * ===================================================================================================================================
    //

    async fn onchain_register_mm(
        &self,
        //
        req: Request<OnChainRegisterMmReq>,
    ) -> Result<Response<OnChainRegisterMmRes>, Status> {
        return onchain_register_mm_inner(
            &self.transaction_batch,
            &self.order_books,
            &self.perp_order_books,
            &self.semaphore,
            &self.is_paused,
            req,
        )
        .await;
    }

    //
    // * ===================================================================================================================================
    //

    async fn add_liquidity_mm(
        &self,
        req: Request<OnChainAddLiqTabReq>,
    ) -> Result<Response<AddLiqOrderTabRes>, Status> {
        return add_liquidity_mm_inner(
            &self.transaction_batch,
            &self.order_books,
            &self.perp_order_books,
            &self.semaphore,
            &self.is_paused,
            req,
        )
        .await;
    }

    //
    // * ===================================================================================================================================
    //

    async fn remove_liquidity_mm(
        &self,
        req: Request<OnChainRemoveLiqTabReq>,
    ) -> Result<Response<RemoveLiqOrderTabRes>, Status> {
        return remove_liquidity_mm_inner(
            &self.transaction_batch,
            &self.order_books,
            &self.perp_order_books,
            &self.semaphore,
            &self.is_paused,
            req,
        )
        .await;
    }

    //
    // * ===================================================================================================================================
    //

    async fn finalize_batch(
        &self,
        request: Request<EmptyReq>,
    ) -> Result<Response<FinalizeBatchResponse>, Status> {
        // ? Only call the server from the same network (onyl as fallback)
        if request.remote_addr().unwrap().ip()
            != std::net::IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1))
        {
            return Ok(Response::new(FinalizeBatchResponse {}));
        }

        return finalize_batch_inner(
            &self.transaction_batch,
            &self.semaphore,
            &self.is_paused,
            request,
        )
        .await;
    }

    //
    // * ===================================================================================================================================
    //

    async fn update_index_price(
        &self,
        request: Request<OracleUpdateReq>,
    ) -> Result<Response<SuccessResponse>, Status> {
        // ? Only call the server from the same network (onyl as fallback)
        if request.remote_addr().unwrap().ip()
            != std::net::IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1))
        {
            return send_oracle_update_error_reply(format!(
                "update_index_price can only be called from the same network"
            ));
        }

        return update_index_price_inner(&self.transaction_batch, request).await;
    }

    //
    // * ===================================================================================================================================
    //

    async fn restore_orderbook(
        &self,
        request: Request<RestoreOrderBookMessage>,
    ) -> Result<Response<SuccessResponse>, Status> {
        // ? Only call the server from the same network (onyl as fallback)
        if request.remote_addr().unwrap().ip()
            != std::net::IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1))
        {
            let reply = SuccessResponse {
                successful: false,
                error_message: "restore_orderbook can only be called from the same network"
                    .to_string(),
            };

            return Ok(Response::new(reply));
        }

        return restore_orderbook_inner(&self.order_books, &self.perp_order_books, request).await;
    }

    //
    // * ===================================================================================================================================
    //

    async fn get_liquidity(
        &self,
        request: Request<LiquidityReq>,
    ) -> Result<Response<LiquidityRes>, Status> {
        return get_liquidity_inner(&self.order_books, &self.perp_order_books, request).await;
    }

    async fn get_orders(&self, request: Request<OrdersReq>) -> Result<Response<OrdersRes>, Status> {
        return get_orders_inner(
            &self.transaction_batch,
            &self.order_books,
            &self.perp_order_books,
            request,
        )
        .await;
    }

    // rpc get_index_prices (EmptyReq) returns (IndexPriceRes);
    async fn get_index_prices(
        &self,
        req: Request<EmptyReq>,
    ) -> Result<Response<IndexPriceRes>, Status> {
        return get_index_prices_inner(&self.transaction_batch, req).await;
    }

    async fn get_state_info(
        &self,
        req: Request<StateInfoReq>,
    ) -> Result<Response<StateInfoRes>, Status> {
        return get_state_info_inner(&self.transaction_batch, req).await;
    }

    async fn get_funding_info(
        &self,
        req: Request<FundingReq>,
    ) -> Result<Response<FundingRes>, Status> {
        return get_funding_info_inner(&self.transaction_batch, req).await;
    }

    //
    // * ===================================================================================================================================
    //
}
