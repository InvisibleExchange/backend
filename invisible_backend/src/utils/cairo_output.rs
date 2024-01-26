use num_bigint::{BigInt, BigUint, Sign};
use num_integer::Integer;
use num_traits::{FromPrimitive, ToPrimitive, Zero};

use crate::{
    perpetual::{perp_position::get_liquidation_price, OrderSide},
    transaction_batch::{
        tx_batch_structs::{GlobalConfig, GlobalDexState, ProgramInputCounts},
        CHAIN_IDS,
    },
};

use super::{crypto_utils::hash_many, storage::firestore::upload_file_to_storage};

use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgramOutput {
    pub dex_state: GlobalDexState,
    pub global_config: GlobalConfig,
    pub accumulated_hashes: Vec<AccumulatedHashesOutput>,
    pub deposit_outputs: Vec<DepositOutput>,
    pub withdrawal_outputs: Vec<WithdrawalOutput>,
    pub mm_onchain_actions: Vec<OnChainMMActionOutput>,
    pub escape_outputs: Vec<EscapeOutput>,
    pub position_escape_outputs: Vec<PositionEscapeOutput>,
    pub note_outputs: Vec<NoteOutput>,
    pub position_outputs: Vec<PerpPositionOutput>,
    pub tab_outputs: Vec<OrderTabOutput>,
    pub zero_note_idxs: Vec<u64>,
}

pub fn parse_cairo_output(raw_program_output: Vec<&str>) -> ProgramOutput {
    // & cairo_output structure:
    // 0: dex_state + global_config
    // 1: accumulated_hashes
    // 1.1: deposits
    // 1.2: withdrawals
    // 1.3: MM registrations
    // 1.4: escapes
    // 1.5: position escapes
    // 2: notes
    // 3: positions
    // 4: order_tabs
    // 5: zero indexes

    let cairo_output = preprocess_cairo_output(raw_program_output);

    // ? Parse dex state
    let (dex_state, cairo_output) = parse_dex_state(&cairo_output);

    let (global_config, cairo_output) = parse_global_config(cairo_output);

    // ? Parse accumulated hashes
    let (accumulated_hashes, cairo_output) =
        parse_accumulated_hashes_outputs(&cairo_output, CHAIN_IDS.len());

    // ? Parse deposits
    let (deposit_outputs, cairo_output) =
        parse_deposit_outputs(cairo_output, dex_state.program_input_counts.n_deposits);

    // ? Parse withdrawals
    let (withdrawal_outputs, cairo_output) =
        parse_withdrawal_outputs(&cairo_output, dex_state.program_input_counts.n_withdrawals);

    // ? Parse MM registrations
    let (mm_onchain_actions, cairo_output) = parse_onchain_mm_actions(
        &cairo_output,
        dex_state.program_input_counts.n_onchain_mm_actions,
    );

    // ? Parse escapes
    let (escape_outputs, cairo_output) = parse_escape_outputs(
        &cairo_output,
        dex_state.program_input_counts.n_note_escapes
            + dex_state.program_input_counts.n_tab_escapes,
    );

    // ? Parse position escapes
    let (position_escape_outputs, cairo_output) = parse_position_escape_outputs(
        &cairo_output,
        dex_state.program_input_counts.n_position_escapes,
    );

    // ? Parse notes
    let (note_outputs, cairo_output) =
        parse_note_outputs(cairo_output, dex_state.program_input_counts.n_output_notes);

    // ? Parse positions
    let (position_outputs, cairo_output) = parse_position_outputs(
        cairo_output,
        dex_state.program_input_counts.n_output_positions,
    );

    // ? Parse order tabs
    let (tab_outputs, cairo_output) =
        parse_order_tab_outputs(cairo_output, dex_state.program_input_counts.n_output_tabs);

    // ? Parse zero notes
    let zero_note_idxs =
        parse_zero_indexes(cairo_output, dex_state.program_input_counts.n_zero_indexes);

    let program_output = ProgramOutput {
        dex_state,
        global_config,
        accumulated_hashes,
        deposit_outputs,
        withdrawal_outputs,
        mm_onchain_actions,
        escape_outputs,
        position_escape_outputs,
        note_outputs,
        position_outputs,
        tab_outputs,
        zero_note_idxs,
    };

    return program_output;
}

// * =====================================================================================

fn parse_dex_state(output: &[BigUint]) -> (GlobalDexState, &[BigUint]) {
    // & assert config_output_ptr[0] = dex_state.init_state_root;
    // & assert config_output_ptr[1] = dex_state.final_state_root;

    let init_state_root = &output[0];
    let final_state_root = &output[1];

    // & 1: | state_tree_depth (8 bits) | global_expiration_timestamp (32 bits) | tx_batch_id (32 bits) |
    let batched_output_info = &output[2];
    let res_vec = split_by_bytes(batched_output_info, vec![8, 32, 32]);
    let state_tree_depth = res_vec[0].to_u32().unwrap();
    let global_expiration_timestamp = res_vec[1].to_u32().unwrap();
    let config_code = res_vec[2].to_u32().unwrap();

    // & n_output_notes (32 bits) | n_output_positions (16 bits) | n_output_tabs (16 bits) | n_zero_indexes (32 bits) | n_deposits (16 bits) | n_withdrawals (16 bits) |
    // & n_onchain_mm_actions (16 bits) | n_note_escapes (16 bits) | n_position_escapes (16 bits) | n_tab_escapes (16 bits) |
    let output_counts1 = &output[3];
    let res_vec = split_by_bytes(output_counts1, vec![32, 16, 16, 32, 16, 16, 16, 16, 16, 16]);
    let n_output_notes = res_vec[0].to_u32().unwrap();
    let n_output_positions = res_vec[1].to_u16().unwrap();
    let n_output_tabs = res_vec[2].to_u16().unwrap();
    let n_zero_indexes = res_vec[3].to_u32().unwrap();
    let n_deposits = res_vec[4].to_u16().unwrap();
    let n_withdrawals = res_vec[5].to_u16().unwrap();
    let n_onchain_mm_actions = res_vec[6].to_u16().unwrap();
    let n_note_escapes = res_vec[7].to_u16().unwrap();
    let n_position_escapes = res_vec[8].to_u16().unwrap();
    let n_tab_escapes = res_vec[9].to_u16().unwrap();

    let shifted_output = &output[4..];

    let program_input_counts = ProgramInputCounts {
        n_output_notes,
        n_output_positions,
        n_output_tabs,
        n_zero_indexes,
        n_deposits,
        n_withdrawals,
        n_onchain_mm_actions,
        n_note_escapes,
        n_position_escapes,
        n_tab_escapes,
    };

    return (
        GlobalDexState::new(
            config_code,
            &init_state_root,
            &final_state_root,
            state_tree_depth,
            global_expiration_timestamp,
            program_input_counts,
        ),
        shifted_output,
    );
}

// * =====================================================================================

fn parse_global_config(output: &[BigUint]) -> (GlobalConfig, &[BigUint]) {
    // & 1: | collateral_token (32 bits) | leverage_decimals (8 bits) | assets_len (32 bits) | synthetic_assets_len (32 bits) | observers_len (32 bits) | chain_ids_len (32 bits) |
    let batched_info = &output[0];
    let res_vec = split_by_bytes(batched_info, vec![32, 8, 32, 32, 32, 32]);
    let collateral_token = res_vec[0].to_u32().unwrap();
    let leverage_decimals = res_vec[1].to_u8().unwrap();
    let assets_len = res_vec[2].to_u32().unwrap();
    let synthetic_assets_len = res_vec[3].to_u32().unwrap();
    let observers_len = res_vec[4].to_u32().unwrap();
    let chain_ids_len = res_vec[5].to_u32().unwrap();

    // ? 1 + 3*assets_len + 5*synthetic_assets_len + observers_len + chain_ids_len

    // ? assets
    let mut i = 1;
    let i_next = i + assets_len as usize;
    let assets = output[i..i_next]
        .iter()
        .map(|v| v.to_u32().unwrap())
        .collect();
    i = i_next;

    // ? synthetic_assets
    let i_next = i + synthetic_assets_len as usize;
    let synthetic_assets = output[i..i_next]
        .iter()
        .map(|v| v.to_u32().unwrap())
        .collect();
    i = i_next;
    //* */
    // ? decimals_per_asset
    let i_next = i + assets_len as usize;
    let decimals_per_asset = output[i..i_next]
        .into_iter()
        .map(|o| o.to_u64().unwrap())
        .collect::<Vec<u64>>();
    i = i_next;
    // ? dust_amount_per_asset
    let i_next = i + assets_len as usize;
    let dust_amount_per_asset = output[i..i_next]
        .into_iter()
        .map(|o| o.to_u64().unwrap())
        .collect::<Vec<u64>>();
    i = i_next;

    // *
    // ? price_decimals_per_asset
    let i_next = i + synthetic_assets_len as usize;
    let price_decimals_per_asset = output[i..i_next]
        .into_iter()
        .map(|o| o.to_u64().unwrap())
        .collect::<Vec<u64>>();
    i = i_next;
    // ? min_partial_liquidation_size
    let i_next = i + synthetic_assets_len as usize;
    let min_partial_liquidation_sizes = output[i..i_next]
        .into_iter()
        .map(|o| o.to_u64().unwrap())
        .collect::<Vec<u64>>();
    i = i_next;
    // ? leverage_bounds_per_asset
    let i_next = i + 2 * synthetic_assets_len as usize;
    let leverage_bounds_per_asset = output[i..i_next]
        .into_iter()
        .map(|o| (o.to_u64().unwrap() / 100_000) as f64)
        .collect::<Vec<f64>>();
    i = i_next;
    //*

    // ? Chain IDs
    let i_next = i + chain_ids_len as usize;
    let chain_ids = output[i..i_next]
        .iter()
        .map(|v| v.to_u32().unwrap())
        .collect();
    i = i_next;
    // ? observers
    let i_next = i + observers_len as usize;
    let observers = output[i..i_next]
        .into_iter()
        .map(|o| o.to_string())
        .collect::<Vec<String>>();
    i = i_next;

    let shifted_output = &output[i..];

    return (
        GlobalConfig {
            assets,
            synthetic_assets,
            collateral_token,

            chain_ids,
            leverage_decimals,

            decimals_per_asset,
            dust_amount_per_asset,

            price_decimals_per_asset,
            leverage_bounds_per_asset,
            min_partial_liquidation_sizes,

            observers,
        },
        shifted_output,
    );
}

// * =====================================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccumulatedHashesOutput {
    pub chain_id: u32,
    pub deposit_hash: String,
    pub withdrawal_hash: String,
}

fn parse_accumulated_hashes_outputs(
    output: &[BigUint],
    num_chain_ids: usize,
) -> (Vec<AccumulatedHashesOutput>, &[BigUint]) {
    let mut hashes: Vec<AccumulatedHashesOutput> = Vec::new();

    for i in 0..num_chain_ids {
        let chain_id = output[(i * 3) as usize].clone();
        let deposit_hash = output[(i * 3 + 1) as usize].to_string();
        let withdrawal_hash = output[(i * 3 + 2) as usize].to_string();

        let hash = AccumulatedHashesOutput {
            chain_id: chain_id.to_u32().unwrap(),
            deposit_hash,
            withdrawal_hash,
        };

        hashes.push(hash);
    }

    let shifted_output = &output[3 * num_chain_ids..];

    return (hashes, shifted_output);
}

// * =====================================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DepositOutput {
    pub deposit_id: u64,
    pub token: u32,
    pub amount: u64,
    pub deposit_pub_key: String,
}

// & batched_note_info format: | deposit_id (64 bits) | token (32 bits) | amount (64 bits) |
// & --------------------------  deposit_id => chain id (32 bits) | identifier (32 bits) |

fn parse_deposit_outputs(
    output: &[BigUint],
    num_deposits: u16,
) -> (Vec<DepositOutput>, &[BigUint]) {
    // output is offset by 12 (dex state)

    let mut deposits: Vec<DepositOutput> = Vec::new();

    for i in 0..num_deposits {
        let batch_deposit_info = output[(i * 2) as usize].clone();

        let split_num = split_by_bytes(&batch_deposit_info, vec![64, 32, 64]);

        let deposit_id = split_num[0].to_u64().unwrap();
        let token = split_num[1].to_u32().unwrap();
        let amount = split_num[2].to_u64().unwrap();

        let deposit_pub_key = output[(i * 2 + 1) as usize].to_string();

        let deposit = DepositOutput {
            deposit_id,
            token,
            amount,
            deposit_pub_key,
        };

        deposits.push(deposit);
    }

    let shifted_output = &output[2 * num_deposits as usize..];

    return (deposits, shifted_output);
}

// * =====================================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WithdrawalOutput {
    pub chain_id: u32,
    pub token: u32,
    pub amount: u64,
    pub withdrawal_address: String,
}

// & batched_note_info format: | withdrawal_chain_id (32 bits) | token (32 bits) | amount (64 bits) |

fn parse_withdrawal_outputs(
    output: &[BigUint],
    num_wthdrawals: u16,
) -> (Vec<WithdrawalOutput>, &[BigUint]) {
    // output is offset by 12 (dex state)

    let mut withdrawals: Vec<WithdrawalOutput> = Vec::new();

    for i in 0..num_wthdrawals {
        let batch_withdrawal_info = output[(i * 2) as usize].clone();

        let split_vec = split_by_bytes(&batch_withdrawal_info, vec![32, 32, 64]);

        let chain_id = split_vec[0].to_u32().unwrap();
        let token = split_vec[1].to_u32().unwrap();
        let amount = split_vec[2].to_u64().unwrap();

        let withdrawal_address = output[(i * 2 + 1) as usize].to_string();

        let withdrawal = WithdrawalOutput {
            chain_id,
            token,
            amount,
            withdrawal_address,
        };

        withdrawals.push(withdrawal);
    }

    let shifted_output = &output[2 * num_wthdrawals as usize..];

    return (withdrawals, shifted_output);
}

// * =====================================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnChainMMActionOutput {
    pub mm_position_address: String,
    pub depositor: String,
    pub batched_action_info: String,
}

fn parse_onchain_mm_actions(
    output: &[BigUint],
    num_actions: u16,
) -> (Vec<OnChainMMActionOutput>, &[BigUint]) {
    // & batched_registration_info format: | vlp_token (32 bits) | max_vlp_supply (64 bits) | vlp_amount (64 bits) | action_type (8 bits) |
    // & batched_add_liq_info format:  usdcAmount (64 bits) | vlp_amount (64 bits) | action_type (8 bits) |
    // & batched_remove_liq_info format:  | initialValue (64 bits) | vlpAmount (64 bits) | returnAmount (64 bits) | action_type (8 bits) |
    // & batched_close_mm_info format:  | initialValueSum (64 bits) | vlpAmountSum (64 bits) | returnAmount (64 bits) | action_type (8 bits) |

    let mut mm_actions: Vec<OnChainMMActionOutput> = Vec::new();

    for i in 0..num_actions {
        let mm_position_address = output[(i * 2) as usize].to_string();
        let depositor = output[(i * 2 + 1) as usize].to_string();

        let batched_action_info = output[(i * 3 + 2) as usize].to_string();

        // let split_vec = split_by_bytes(&batch_registrations_info, vec![1, 32, 64]);
        // let is_perp = split_vec[0].to_u8().unwrap() == 1;
        // let vlp_token = split_vec[1].to_u32().unwrap();
        // let max_vlp_supply = split_vec[2].to_u64().unwrap();

        let registration = OnChainMMActionOutput {
            mm_position_address,
            depositor,
            batched_action_info,
        };

        mm_actions.push(registration);
    }

    let shifted_output = &output[2 * num_actions as usize..];

    return (mm_actions, shifted_output);
}

// * =====================================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EscapeType {
    OrderTabEscape,
    NoteEscape,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EscapeOutput {
    pub escape_id: u32,
    pub is_valid: bool,
    pub escape_type: EscapeType,
    pub escape_message_hash: String,
    pub signature_r: String,
    pub signature_s: String,
}

fn parse_escape_outputs(
    output: &[BigUint],
    num_escape_outputs: u16,
) -> (Vec<EscapeOutput>, &[BigUint]) {
    let mut escape_outputs: Vec<EscapeOutput> = Vec::new();

    for i in 0..num_escape_outputs {
        let escape_output = output[(i * 2) as usize].clone();

        // escape_value (64 bits) | escape_id (32 bits) | is_valid (8 bits) |
        let split_vec = split_by_bytes(&escape_output, vec![32, 8, 8]);

        let escape_id = split_vec[0].to_u32().unwrap();
        let is_valid = split_vec[1].to_u8().unwrap() == 1;
        let escape_type = if split_vec[2].to_u8().unwrap() == 0 {
            EscapeType::NoteEscape
        } else {
            EscapeType::OrderTabEscape
        };

        let escape = EscapeOutput {
            escape_id,
            is_valid,
            escape_type,
            escape_message_hash: output[(i * 2 + 1) as usize].to_string(),
            signature_r: output[(i * 2 + 2) as usize].to_string(),
            signature_s: output[(i * 2 + 3) as usize].to_string(),
        };

        escape_outputs.push(escape);
    }

    let shifted_output = &output[4 * num_escape_outputs as usize..];

    return (escape_outputs, shifted_output);
}

// * =====================================================================================
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionEscapeOutput {
    pub escape_id: u32,
    pub is_valid: bool,
    pub escape_value: u64,
    pub recipient: u64,
    pub escape_message_hash: String,
    pub signature_a_r: String,
    pub signature_a_s: String,
    pub signature_b_r: String,
    pub signature_b_s: String,
}

fn parse_position_escape_outputs(
    output: &[BigUint],
    num_escape_outputs: u16,
) -> (Vec<PositionEscapeOutput>, &[BigUint]) {
    let mut escape_outputs: Vec<PositionEscapeOutput> = Vec::new();

    for i in 0..num_escape_outputs {
        let escape_output = output[(i * 2) as usize].clone();

        // escape_id (32 bits) | is_valid (8 bits) | escape_type (8 bits) |
        let split_vec = split_by_bytes(&escape_output, vec![32, 8, 8]);

        let escape_value = split_vec[0].to_u64().unwrap();
        let escape_id = split_vec[1].to_u32().unwrap();
        let is_valid = split_vec[2].to_u8().unwrap() == 1;

        let escape = PositionEscapeOutput {
            escape_id,
            is_valid,
            escape_value,
            recipient: output[(i * 2 + 1) as usize].to_u64().unwrap(),
            escape_message_hash: output[(i * 2 + 2) as usize].to_string(),
            signature_a_r: output[(i * 2 + 3) as usize].to_string(),
            signature_a_s: output[(i * 2 + 4) as usize].to_string(),
            signature_b_r: output[(i * 2 + 5) as usize].to_string(),
            signature_b_s: output[(i * 2 + 6) as usize].to_string(),
        };

        escape_outputs.push(escape);
    }

    let shifted_output = &output[7 * num_escape_outputs as usize..];

    return (escape_outputs, shifted_output);
}

// * =====================================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoteOutput {
    pub index: u64,
    pub token: u32,
    pub hidden_amount: u64,
    pub commitment: String,
    pub address_x: String,
    pub address_y: String,
    pub hash: String,
}

// & batched_note_info format: | token (32 bits) | hidden amount (64 bits) | idx (64 bits) |

fn parse_note_outputs(output: &[BigUint], num_notes: u32) -> (Vec<NoteOutput>, &[BigUint]) {
    // output is offset by 12 (dex state)

    let mut notes: Vec<NoteOutput> = Vec::new();

    for i in 0..num_notes {
        let batched_note_info = output[(i * 3) as usize].clone();

        let split_vec = split_by_bytes(&batched_note_info, vec![32, 64, 64]);
        let token = split_vec[0].to_u32().unwrap();
        let hidden_amount = split_vec[1].to_u64().unwrap();
        let index = split_vec[2].to_u64().unwrap();

        let commitment = &output[(i * 3 + 1) as usize];
        let address_x = &output[(i * 3 + 2) as usize];
        let address_y = &output[(i * 3 + 3) as usize];

        let hash = hash_note_output(token, &commitment, &address_y).to_string();

        let note = NoteOutput {
            index,
            token,
            hidden_amount,
            commitment: commitment.to_string(),
            address_x: address_x.to_string(),
            address_y: address_y.to_string(),
            hash,
        };

        notes.push(note);
    }

    let shifted_output = &output[3 * num_notes as usize..];

    return (notes, shifted_output);
}

pub fn hash_note_output(token: u32, commitment: &BigUint, address_x: &BigUint) -> BigUint {
    let token = BigUint::from_u32(token).unwrap();
    let hash_input: Vec<&BigUint> = vec![&address_x, &token, &commitment];

    let note_hash = hash_many(&hash_input);

    return note_hash;
}

// * ==========================================================================================
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerpPositionOutput {
    pub synthetic_token: u32,
    pub position_size: u64,
    pub order_side: OrderSide,
    pub entry_price: u64,
    pub margin: u64,
    pub last_funding_idx: u32,
    pub allow_partial_liquidations: bool,
    pub vlp_token: u32,
    pub vlp_supply: u64,
    pub index: u64,
    pub public_key: String,
    pub hash: String,
}

// & format: | index (64 bits) | synthetic_token (32 bits) | position_size (64 bits) | vlp_token (32 bits) |
// & format: | entry_price (64 bits) | margin (64 bits) | vlp_supply (64 bits) | last_funding_idx (32 bits) | order_side (1 bits) | allow_partial_liquidations (1 bits) |
// & format: | public key <-> position_address (251 bits) |

fn parse_position_outputs(
    output: &[BigUint],
    num_positions: u16,
) -> (Vec<PerpPositionOutput>, &[BigUint]) {
    let mut positions: Vec<PerpPositionOutput> = Vec::new();

    for i in 0..num_positions {
        let batched_position_info_slot1 = output[(i * 3) as usize].clone();
        let batched_position_info_slot2 = output[(i * 3 + 1) as usize].clone();

        // & format: | index (64 bits) | synthetic_token (32 bits) | position_size (64 bits) | vlp_token (32 bits) |
        let split_vec_slot1 = split_by_bytes(&batched_position_info_slot1, vec![64, 32, 64, 32]);
        // & format: | entry_price (64 bits) | margin (64 bits) | vlp_supply (64 bits) | last_funding_idx (32 bits) | order_side (1 bits) | allow_partial_liquidations (1 bits) |
        let split_vec_slot2 =
            split_by_bytes(&batched_position_info_slot2, vec![64, 64, 64, 32, 1, 1]);

        let index = split_vec_slot1[0].to_u64().unwrap();
        let synthetic_token = split_vec_slot1[1].to_u32().unwrap();
        let position_size = split_vec_slot1[2].to_u64().unwrap();
        let vlp_token = split_vec_slot1[3].to_u32().unwrap();

        let entry_price = split_vec_slot2[0].to_u64().unwrap();
        let margin = split_vec_slot2[1].to_u64().unwrap();
        let vlp_supply = split_vec_slot2[2].to_u64().unwrap();
        let last_funding_idx = split_vec_slot2[3].to_u32().unwrap();
        let order_side = if split_vec_slot2[4] != BigUint::zero() {
            OrderSide::Long
        } else {
            OrderSide::Short
        };
        let allow_partial_liquidations = split_vec_slot2[5] != BigUint::zero();

        // & format: | public key <-> position_address (251 bits) |
        let public_key = &output[(i * 3 + 2) as usize];

        let hash = hash_position_output(
            synthetic_token,
            public_key,
            allow_partial_liquidations,
            vlp_token,
            //
            &order_side,
            position_size,
            entry_price,
            margin,
            last_funding_idx,
            vlp_supply,
        )
        .to_string();

        let position = PerpPositionOutput {
            synthetic_token,
            position_size,
            order_side,
            entry_price,
            margin,
            last_funding_idx,
            allow_partial_liquidations,
            vlp_supply,
            vlp_token,
            index,
            public_key: public_key.to_string(),
            hash,
        };

        positions.push(position);
    }

    let shifted_output = &output[3 * num_positions as usize..];

    return (positions, shifted_output);
}

pub fn hash_position_output(
    synthetic_token: u32,
    position_address: &BigUint,
    allow_partial_liquidations: bool,
    vlp_token: u32,
    //
    order_side: &OrderSide,
    position_size: u64,
    entry_price: u64,
    margin: u64,
    current_funding_idx: u32,
    vlp_supply: u64,
) -> BigUint {
    let liquidation_price = get_liquidation_price(
        entry_price,
        margin,
        position_size,
        &order_side,
        synthetic_token,
        allow_partial_liquidations,
    );

    // & header_hash = H({allow_partial_liquidations, synthetic_token, position_address, vlp_token})
    let allow_partial_liquidations =
        BigUint::from_u8(if allow_partial_liquidations { 1 } else { 0 }).unwrap();
    let synthetic_token = BigUint::from_u32(synthetic_token).unwrap();
    let vlp_token = BigUint::from_u32(vlp_token).unwrap();
    let hash_inputs = vec![
        &allow_partial_liquidations,
        &synthetic_token,
        position_address,
        &vlp_token,
    ];
    let header_hash = hash_many(&hash_inputs);

    // & hash = H({header_hash, order_side, position_size, entry_price, liquidation_price, current_funding_idx, vlp_supply})
    let order_side = BigUint::from_u8(if *order_side == OrderSide::Long { 1 } else { 0 }).unwrap();
    let position_size = BigUint::from_u64(position_size).unwrap();
    let entry_price = BigUint::from_u64(entry_price).unwrap();
    let liquidation_price = BigUint::from_u64(liquidation_price).unwrap();
    let current_funding_idx = BigUint::from_u32(current_funding_idx).unwrap();
    let vlp_supply = BigUint::from_u64(vlp_supply).unwrap();
    let hash_inputs = vec![
        &header_hash,
        &order_side,
        &position_size,
        &entry_price,
        &liquidation_price,
        &current_funding_idx,
        &vlp_supply,
    ];

    let position_hash = hash_many(&hash_inputs);

    return position_hash;
}

// * ==========================================================================================
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderTabOutput {
    pub index: u64,
    pub base_token: u32,
    pub quote_token: u32,
    pub base_hidden_amount: u64,
    pub quote_hidden_amount: u64,
    pub base_commitment: String,
    pub quote_commitment: String,
    pub public_key: String,
    pub hash: String,
}

// & format: | index (59 bits) | base_token (32 bits) | quote_token (32 bits) | base_hidden_amount (64 bits) | quote_hidden_amount (64 bits)
fn parse_order_tab_outputs(output: &[BigUint], num_tabs: u16) -> (Vec<OrderTabOutput>, &[BigUint]) {
    let mut order_tabs: Vec<OrderTabOutput> = Vec::new();

    for i in 0..num_tabs {
        let batched_tab_info = output[(i * 4) as usize].clone();
        let split_vec = split_by_bytes(&batched_tab_info, vec![59, 32, 32, 64, 64]);

        let index = split_vec[0].to_u64().unwrap();
        let base_token = split_vec[1].to_u32().unwrap();
        let quote_token = split_vec[2].to_u32().unwrap();
        let base_hidden_amount = split_vec[3].to_u64().unwrap();
        let quote_hidden_amount = split_vec[4].to_u64().unwrap();

        let base_commitment = &output[(i * 4 + 1) as usize];
        let quote_commitment = &output[(i * 4 + 2) as usize];
        let public_key = &output[(i * 4 + 3) as usize];

        let hash = hash_order_tab_output(
            base_token,
            quote_token,
            &public_key,
            &base_commitment,
            &quote_commitment,
        )
        .to_string();

        let order_tab = OrderTabOutput {
            index,
            base_token,
            quote_token,
            base_hidden_amount,
            quote_hidden_amount,
            base_commitment: base_commitment.to_string(),
            quote_commitment: quote_commitment.to_string(),
            public_key: public_key.to_string(),
            hash,
        };

        order_tabs.push(order_tab);
    }

    let shifted_output = &output[4 * num_tabs as usize..];

    return (order_tabs, shifted_output);
}

pub fn hash_order_tab_output(
    base_token: u32,
    quote_token: u32,
    pub_key: &BigUint,
    //
    base_commitment: &BigUint,
    quote_commitment: &BigUint,
) -> BigUint {
    // & header_hash = H({base_token, quote_token, pub_key})

    let base_token = BigUint::from_u32(base_token).unwrap();
    let quote_token = BigUint::from_u32(quote_token).unwrap();

    let hash_inputs: Vec<&BigUint> = vec![&base_token, &quote_token, pub_key];
    let header_hash = hash_many(&hash_inputs);

    // & H({header_hash, base_commitment, quote_commitment})
    let hash_inputs: Vec<&BigUint> = vec![&header_hash, base_commitment, quote_commitment];
    let tab_hash = hash_many(&hash_inputs);

    return tab_hash;
}

// * ==========================================================================================

fn parse_zero_indexes(output: &[BigUint], num_zero_idxs: u32) -> Vec<u64> {
    let slice_len = (num_zero_idxs as f32 / 3.0).ceil() as usize;

    let slice: Vec<BigUint> = output[0..slice_len].try_into().unwrap();

    let mut zero_idxs = split_vec_by_bytes(&slice, vec![64, 64, 64])
        .into_iter()
        .map(|x| x.to_u64().unwrap())
        .collect::<Vec<u64>>();

    if num_zero_idxs > zero_idxs.len() as u32 {
        zero_idxs.push(0)
    } else if (zero_idxs.len() as u32) < num_zero_idxs {
        zero_idxs = zero_idxs[0..num_zero_idxs as usize].to_vec();
    }

    return zero_idxs;
}

// * =====================================================================================

pub fn format_cairo_ouput(program_output: &str) -> Vec<&str> {
    // Split the string into an array of shorter strings at the newline character and trim the whitespace

    let program_output = program_output
        .split("\n")
        .filter(|s| !s.is_empty())
        .map(|s| s.trim())
        .collect::<Vec<&str>>();

    return program_output;
}

pub fn preprocess_cairo_output(program_output: Vec<&str>) -> Vec<BigUint> {
    let p: BigInt =
        BigInt::from_u64(2).unwrap().pow(251) + 17 * BigInt::from_u64(2).unwrap().pow(192) + 1;

    let arr2 = program_output
        .iter()
        .map(|x| BigInt::parse_bytes(x.as_bytes(), 10).unwrap())
        .collect::<Vec<BigInt>>();

    let arr = arr2
        .iter()
        .map(|x| {
            let num = if x.sign() == Sign::Minus {
                p.clone() + x
            } else {
                x.clone()
            };

            num.to_biguint().unwrap()
        })
        .collect::<Vec<BigUint>>();

    return arr;
}

pub fn split_by_bytes(num: &BigUint, bit_lenghts: Vec<u8>) -> Vec<BigUint> {
    // & returns a vector of values split by the bit_lenghts

    let mut peaces: Vec<BigUint> = Vec::new();
    let mut num = num.clone();
    for i in (0..bit_lenghts.len()).rev() {
        let (q, r) = num.div_mod_floor(&BigUint::from(2_u8).pow(bit_lenghts[i] as u32));

        peaces.push(r);
        num = q;
    }

    peaces.reverse();

    return peaces;
}

fn split_vec_by_bytes(nums: &[BigUint], bit_lenghts: Vec<u8>) -> Vec<BigUint> {
    let mut results = vec![];
    for i in 0..nums.len() {
        let num = &nums[i];

        let peaces = split_by_bytes(num, bit_lenghts.clone());

        if i == nums.len() - 1 {
            for peace in peaces {
                if peace != BigUint::zero() {
                    results.push(peace);
                }
            }

            break;
        }

        results.extend(peaces);
    }

    return results;
}

// * =====================================================================================

pub async fn store_program_output(
    program_output: ProgramOutput,
) -> Result<(), Box<dyn std::error::Error>> {
    // ? Store note data
    for note in program_output.note_outputs {
        let serialized_data = serde_json::to_vec(&note)?;

        upload_file_to_storage(
            "state/".to_string() + &note.index.to_string(),
            serialized_data,
        )
        .await?
    }

    // ? Store position data
    for position in program_output.position_outputs {
        let serialized_data = serde_json::to_vec(&position)?;

        upload_file_to_storage(
            "state/".to_string() + &position.index.to_string(),
            serialized_data,
        )
        .await?
    }

    // ? Store tab data
    for order_tab in program_output.tab_outputs {
        let serialized_data = serde_json::to_vec(&order_tab)?;

        upload_file_to_storage(
            "state/".to_string() + &order_tab.index.to_string(),
            serialized_data,
        )
        .await?
    }

    Ok(())
}
