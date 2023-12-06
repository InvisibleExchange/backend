use invisible_backend::perpetual::perp_helpers::perp_rollback::PerpRollbackInfo;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use std::thread::{JoinHandle, ThreadId};
use std::time::Instant;

use error_stack::Result;
use invisible_backend::perpetual::perp_helpers::perp_swap_outptut::PerpSwapResponse;
use invisible_backend::perpetual::perp_order::{CloseOrderFields, OpenOrderFields};
use invisible_backend::perpetual::perp_position::PerpPosition;
use invisible_backend::perpetual::{perp_order::PerpOrder, OrderSide};
use invisible_backend::transactions::deposit::Deposit;
use invisible_backend::transactions::transaction_helpers::rollbacks::RollbackInfo;
use invisible_backend::utils::errors::PerpSwapExecutionError;
use invisible_backend::utils::notes::Note;
use num_bigint::{BigInt, BigUint};
use num_traits::{One, Zero};

use invisible_backend::starkware_crypto::EcPoint;

use invisible_backend::{
    perpetual::perp_swap::PerpSwap, transaction_batch::transaction_batch::TransactionBatch,
};

//

#[test]
fn test_perp_swaps() {
    let n: u64 = 10;

    let rollback_safeguard: Arc<Mutex<HashMap<ThreadId, RollbackInfo>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let perp_rollback_safeguard: Arc<Mutex<HashMap<ThreadId, PerpRollbackInfo>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let mut tx_batch = TransactionBatch::new(4, 4, rollback_safeguard, perp_rollback_safeguard);

    let deposits = get_dummy_deposits(1);

    let note1 = deposits[0].notes[0].clone();

    let deposits_vec: Vec<Deposit> = vec![deposits[0].clone(); n as usize];

    for deposit in deposits_vec {
        let handle = tx_batch.execute_transaction(deposit);
        handle.join().unwrap().unwrap();
    }

    let swap_data = get_dummy_open_order_swaps(note1, 1);
    let swap_data_vec: Vec<PerpSwap> = vec![swap_data[0].clone(); n as usize];

    let now = Instant::now();

    let mut handles: Vec<JoinHandle<Result<PerpSwapResponse, PerpSwapExecutionError>>> = Vec::new();
    for swap in swap_data_vec {
        let handle = tx_batch.execute_perpetual_transaction(swap);

        handles.push(handle);
    }

    let mut output_positions: Vec<(PerpPosition, PerpPosition)> = Vec::new();
    for handle in handles {
        let res = handle.join().unwrap();
        output_positions.push((
            res.as_ref().unwrap().position_a.as_ref().unwrap().clone(),
            res.unwrap().position_b.unwrap(),
        ));
    }

    println!("perp swap time taken: {:?}", now.elapsed());

    // * ======================================================================================================== *

    // let modify_order_swaps = get_dummy_add_order_swaps(output_positions);
    // let mut handles: Vec<JoinHandle<Result<PerpSwapResponse, PerpSwapExecutionError>>> = Vec::new();
    // for swap in modify_order_swaps {
    //     let handle = tx_batch.execute_perpetual_transaction(swap);

    //     handles.push(handle);
    // }

    // let mut output_positions: Vec<(PerpPosition, PerpPosition)> = Vec::new();
    // for handle in handles {
    //     let res = handle.join().unwrap();
    //     output_positions.push((
    //         res.as_ref().unwrap().position_a.as_ref().unwrap().clone(),
    //         res.as_ref().unwrap().position_b.as_ref().unwrap().clone(),
    //     ));
    // }

    // // * ======================================================================================================== *

    // let close_order_swaps = get_dummy_close_swaps(output_positions);
    // let mut handles: Vec<JoinHandle<Result<PerpSwapResponse, PerpSwapExecutionError>>> = Vec::new();
    // for swap in close_order_swaps {
    //     let handle = tx_batch.execute_perpetual_transaction(swap);

    //     handles.push(handle);
    // }

    // let mut returned_collateral_notes: Vec<(Note, Note)> = Vec::new();
    // for handle in handles {
    //     let res = handle.join().unwrap();
    //     returned_collateral_notes.push((
    //         res.as_ref()
    //             .unwrap()
    //             .return_collateral_note_a
    //             .as_ref()
    //             .unwrap()
    //             .clone(),
    //         res.as_ref()
    //             .unwrap()
    //             .return_collateral_note_b
    //             .as_ref()
    //             .unwrap()
    //             .clone(),
    //     ));
    // }

    println!("all good")

    //

    //

    //
}

//

//

//

fn get_dummy_deposits(n: u64) -> Vec<Deposit> {
    let pk1 = BigInt::from_str("8932749863246329746327463249328632").unwrap();
    let address1 = EcPoint::from_priv_key(&pk1);

    let mut deposits: Vec<Deposit> = Vec::new();

    for i in 0..n {
        let note1 = Note::new(
            0,
            address1.clone(),
            55555,
            100_000_000,
            BigUint::from_str("1").unwrap(),
        );

        let deposit1 = Deposit::new(
            i,
            55555,
            100_000_000,
            address1.x.to_biguint().unwrap(),
            vec![note1.clone()],
            &pk1.to_biguint().unwrap(),
        );

        let deposit2 = Deposit::new(
            n + i,
            55555,
            100_000_000,
            address1.x.to_biguint().unwrap(),
            vec![note1],
            &pk1.to_biguint().unwrap(),
        );

        deposits.push(deposit1);
        deposits.push(deposit2);
    }

    return deposits;
}

fn get_dummy_open_order_swaps(note_: Note, n: u64) -> Vec<PerpSwap> {
    let pk = BigInt::from_str("8932749863246329746327463249328632").unwrap();

    let pk1 = BigInt::from_str("89327498632463297463274632493286132").unwrap();
    let address1 = EcPoint::from_priv_key(&pk1).x.to_biguint().unwrap();
    let pk2 = BigInt::from_str("32875426429834347923748248234235723").unwrap();
    let address2 = EcPoint::from_priv_key(&pk2).x.to_biguint().unwrap();

    let mut swaps: Vec<PerpSwap> = Vec::new();

    for i in 0..n {
        let mut note1 = note_.clone();
        note1.index = 2 * i;
        let refund_note1 = Note::new(
            2 * i,
            note1.address.clone(),
            note1.token,
            0,
            note1.blinding.clone(),
        );

        let mut note2 = note_.clone();
        note2.index = 2 * i + 1;
        let refund_note2 = Note::new(
            2 * i + 1,
            note2.address.clone(),
            note2.token,
            0,
            note2.blinding.clone(),
        );

        let order_id = 2 * i;
        let expiration_timestamp = 10000;
        let order_side = OrderSide::Long;
        let synthetic_token = 54321;
        let collateral_token = note1.token;
        let synthetic_amount = note1.amount * 2;
        let collateral_amount = note1.amount;
        let initial_margin = note1.amount / 2;
        let fee_limit = 0;
        let notes_in = vec![note1];
        let refund_note = Some(refund_note1);

        let open_order_fields = OpenOrderFields {
            initial_margin,
            collateral_token,
            notes_in,
            refund_note,
            position_address: address1.clone(),
            blinding: BigUint::one(),
        };

        let perp_order1 = PerpOrder::new_open_order(
            order_id,
            expiration_timestamp,
            order_side,
            synthetic_token,
            synthetic_amount,
            collateral_amount,
            fee_limit,
            open_order_fields,
        );

        let order_id = 2 * i + 1;
        let expiration_timestamp = 10000;
        let order_side = OrderSide::Short;
        let synthetic_token = 54321;
        let collateral_token = note2.token;
        let synthetic_amount = note2.amount * 2;
        let collateral_amount = note2.amount;
        let initial_margin = note2.amount / 3;
        let fee_limit = 0;
        let notes_in = vec![note2];
        let refund_note = Some(refund_note2);

        let open_order_fields = OpenOrderFields {
            initial_margin,
            collateral_token,
            notes_in,
            refund_note,
            position_address: address2.clone(),
            blinding: BigUint::one(),
        };

        let perp_order2 = PerpOrder::new_open_order(
            order_id,
            expiration_timestamp,
            order_side,
            synthetic_token,
            synthetic_amount,
            collateral_amount,
            fee_limit,
            open_order_fields,
        );

        let signature1 = perp_order1.sign_order(
            //
            Some(vec![&pk.to_biguint().unwrap()]),
            None,
        );
        let signature2 = perp_order2.sign_order(
            //
            Some(vec![&pk.to_biguint().unwrap()]),
            None,
        );

        let swap = PerpSwap::new(
            perp_order1.clone(),
            perp_order2,
            Some(signature1),
            Some(signature2),
            perp_order1.collateral_amount,
            perp_order1.synthetic_amount,
            0,
            0,
        );

        swaps.push(swap);
    }

    return swaps;
}

fn _get_dummy_add_order_swaps(positions: Vec<(PerpPosition, PerpPosition)>) -> Vec<PerpSwap> {
    let pk1 = BigInt::from_str("89327498632463297463274632493286132").unwrap();
    let pk2 = BigInt::from_str("32875426429834347923748248234235723").unwrap();

    let mut swaps: Vec<PerpSwap> = Vec::new();

    for (pos1, pos2) in positions {
        let perp_order1 = PerpOrder::new_modify_order(
            1,
            1000,
            pos1.clone(),
            pos1.order_side.clone(),
            pos1.synthetic_token,
            20_000_000,
            10_000_000,
            0,
        );

        let perp_order2 = PerpOrder::new_modify_order(
            2,
            1000,
            pos2.clone(),
            pos2.order_side.clone(),
            pos2.synthetic_token,
            20_000_000,
            10_000_000,
            0,
        );

        let signature1 = perp_order1.sign_order(
            //
            None,
            Some(&pk1.to_biguint().unwrap()),
        );
        let signature2 = perp_order2.sign_order(
            //
            None,
            Some(&pk2.to_biguint().unwrap()),
        );

        let swap = PerpSwap::new(
            perp_order1,
            perp_order2,
            Some(signature1),
            Some(signature2),
            10_000_000,
            20_000_000,
            0,
            0,
        );

        swaps.push(swap);
    }

    return swaps;
}

fn _get_dummy_close_swaps(positions: Vec<(PerpPosition, PerpPosition)>) -> Vec<PerpSwap> {
    let pk1 = BigInt::from_str("89327498632463297463274632493286132").unwrap();
    let pk2 = BigInt::from_str("32875426429834347923748248234235723").unwrap();
    let address1 = EcPoint::from_priv_key(&pk1);

    let mut swaps: Vec<PerpSwap> = Vec::new();

    for (pos1, pos2) in positions {
        let close_order_fields = CloseOrderFields {
            dest_received_address: address1.clone(),
            dest_received_blinding: BigUint::zero(),
        };

        let perp_order1 = PerpOrder::new_close_order(
            3,
            1000,
            pos1.clone(),
            pos2.order_side.clone(),
            pos1.synthetic_token,
            pos1.position_size,
            pos1.position_size * 2 / 5,
            0,
            close_order_fields,
        );

        let close_order_fields = CloseOrderFields {
            dest_received_address: address1.clone(),
            dest_received_blinding: BigUint::zero(),
        };

        let perp_order2 = PerpOrder::new_close_order(
            4,
            1000,
            pos2.clone(),
            pos1.order_side.clone(),
            pos2.synthetic_token,
            pos1.position_size,
            pos1.position_size * 2 / 5,
            0,
            close_order_fields,
        );

        let signature1 = perp_order1.sign_order(
            //
            None,
            Some(&pk1.to_biguint().unwrap()),
        );
        let signature2 = perp_order2.sign_order(
            //
            None,
            Some(&pk2.to_biguint().unwrap()),
        );

        let swap = PerpSwap::new(
            perp_order2,
            perp_order1,
            Some(signature2),
            Some(signature1),
            pos1.position_size * 2 / 5,
            pos1.position_size,
            0,
            0,
        );

        swaps.push(swap);
    }

    return swaps;
}
