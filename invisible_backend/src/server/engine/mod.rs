use std::{collections::HashMap, str::FromStr, sync::Arc};

use self::{
    admin::{finalize_batch_inner, restore_orderbook_inner, update_index_price_inner},
    note_position_helpers::{change_position_margin_inner, split_notes_inner},
    onchain_interaction::{execute_deposit_inner, execute_escape_inner, execute_withdrawal_inner},
    onchain_mms::{
        add_liquidity_mm_inner, close_onchain_mm_inner, register_onchain_mm_inner,
        remove_liquidity_mm_inner,
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
    AmendOrderRequest, AmendOrderResponse, CancelOrderMessage, CancelOrderResponse,
    CloseOrderTabReq, DepositMessage, DepositResponse, EmptyReq, EscapeMessage,
    FinalizeBatchResponse, FundingReq, FundingRes, IndexPriceRes, LimitOrderMessage,
    LiquidationOrderMessage, LiquidationOrderResponse, LiquidityReq, LiquidityRes, MarginChangeReq,
    MarginChangeRes, OnChainAddLiqReq, OnChainCloseMmReq, OnChainRegisterMmReq,
    OnChainRemoveLiqReq, OnChainScmmRes, OpenOrderTabReq, OracleUpdateReq, OrderResponse,
    OrdersReq, OrdersRes, PerpOrderMessage, RegisterOnchainActionRequest, RestoreOrderBookMessage,
    SplitNotesReq, SplitNotesRes, StateInfoReq, StateInfoRes, SuccessResponse, UpdateDbIndexesReq,
    WithdrawalMessage,
};
use super::{
    grpc::engine_proto::{engine_server::Engine, CloseOrderTabRes, OpenOrderTabRes},
    server_helpers::WsConnectionsMap,
};
use crate::{
    matching_engine::orderbook::OrderBook,
    utils::{
        errors::send_deposit_error_reply,
        storage::{local_storage::OnchainActionType, update_invalid::update_invalid_state},
    },
};
use crate::{transaction_batch::TransactionBatch, utils::errors::send_oracle_update_error_reply};

use num_bigint::BigUint;
use tokio::sync::{Mutex as TokioMutex, Semaphore};
use tonic::{Request, Response, Status};

mod admin;
mod note_position_helpers;
mod onchain_interaction;
mod onchain_mms;
mod order_executions;
mod order_interactions;
mod order_tabs;
mod queries;

const SERVER_URL: [u8; 4] = [54, 212, 28, 196];

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
        let request: PerpOrderMessage = request.into_inner();

        return submit_perpetual_order_inner(
            &self.transaction_batch,
            &self.perp_order_books,
            &self.ws_connections,
            &self.privileged_ws_connections,
            None,
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
        if !is_local_address(&request) {
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

    async fn execute_escape(
        &self,
        request: Request<EscapeMessage>,
    ) -> Result<Response<SuccessResponse>, Status> {
        return execute_escape_inner(
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
        let req: CloseOrderTabReq = req.into_inner();

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

    async fn register_onchain_mm(
        &self,
        //
        req: Request<OnChainRegisterMmReq>,
    ) -> Result<Response<OnChainScmmRes>, Status> {
        let req = req.into_inner();

        return register_onchain_mm_inner(
            &self.transaction_batch,
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
        req: Request<OnChainAddLiqReq>,
    ) -> Result<Response<OnChainScmmRes>, Status> {
        let req = req.into_inner();

        return add_liquidity_mm_inner(
            &self.transaction_batch,
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
        req: Request<OnChainRemoveLiqReq>,
    ) -> Result<Response<OnChainScmmRes>, Status> {
        let req = req.into_inner();

        return remove_liquidity_mm_inner(
            &self.transaction_batch,
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

    async fn close_onchain_mm(
        &self,
        req: Request<OnChainCloseMmReq>,
    ) -> Result<Response<OnChainScmmRes>, Status> {
        let req = req.into_inner();

        return close_onchain_mm_inner(
            &self.transaction_batch,
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
        if !is_local_address(&request) {
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
        if !is_local_address(&request) {
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
        if !is_local_address(&request) {
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

    async fn register_onchain_action(
        &self,
        request: Request<RegisterOnchainActionRequest>,
    ) -> Result<Response<SuccessResponse>, Status> {
        // ? Only call the server from the same network (onyl as fallback)
        if !is_local_address(&request) {
            let reply = SuccessResponse {
                successful: false,
                error_message: "register_onchain_action can only be called from the same network"
                    .to_string(),
            };

            return Ok(Response::new(reply));
        }

        let request = request.into_inner();

        let action_type = OnchainActionType::from(request.action_type());
        let data_commitment = BigUint::from_str(&request.data_commitment);
        if let Err(_) = data_commitment {
            return Ok(Response::new(SuccessResponse {
                successful: false,
                error_message: "data_commitment is not a valid BigUint".to_string(),
            }));
        }
        let tx_batch = self.transaction_batch.lock().await;
        let main_storage = tx_batch.main_storage.lock();
        println!(
            "Registered onchain action: {} - {:?} - {:?}",
            request.data_id, action_type, data_commitment
        );
        main_storage.register_onchain_action(
            action_type,
            request.data_id,
            data_commitment.unwrap(),
        );
        drop(main_storage);
        drop(tx_batch);

        return Ok(Response::new(SuccessResponse {
            successful: true,
            error_message: "".to_string(),
        }));
    }

    async fn update_invalid_state_indexes(
        &self,
        request: Request<UpdateDbIndexesReq>,
    ) -> Result<Response<SuccessResponse>, Status> {
        // ? Only call the server from the same network (onyl as fallback)
        if !is_local_address(&request) {
            let reply = SuccessResponse {
                successful: false,
                error_message: "register_onchain_action can only be called from the same network"
                    .to_string(),
            };

            return Ok(Response::new(reply));
        }

        let indexes = request.into_inner().invalid_indexes;

        let tx_batch = self.transaction_batch.lock().await;

        update_invalid_state(
            &tx_batch.state_tree,
            &tx_batch.firebase_session,
            &tx_batch.backup_storage,
            indexes,
        );

        return Ok(Response::new(SuccessResponse {
            successful: true,
            error_message: "".to_string(),
        }));
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

fn is_local_address<T>(request: &Request<T>) -> bool {
    let [a, b, c, d] = SERVER_URL;

    return request.remote_addr().unwrap().ip()
        == std::net::IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1))
        || request.remote_addr().unwrap().ip()
            == std::net::IpAddr::V4(std::net::Ipv4Addr::new(a, b, c, d));
}
