use invisible_backend::perpetual::perp_helpers::perp_rollback::PerpRollbackInfo;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use std::thread::ThreadId;

use invisible_backend::perpetual::perp_order::{CloseOrderFields, OpenOrderFields};
use invisible_backend::perpetual::perp_position::PerpPosition;
use invisible_backend::perpetual::{perp_order::PerpOrder, OrderSide};
use invisible_backend::transactions::deposit::Deposit;
use invisible_backend::transactions::transaction_helpers::rollbacks::RollbackInfo;
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
    let rollback_safeguard: Arc<Mutex<HashMap<ThreadId, RollbackInfo>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let perp_rollback_safeguard: Arc<Mutex<HashMap<ThreadId, PerpRollbackInfo>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let mut tx_batch = TransactionBatch::new(5, 3, rollback_safeguard, perp_rollback_safeguard);

    let deposits = get_dummy_deposits();

    let note1 = deposits.0.notes[0].clone();

    let z_idx1: u64 = tx_batch
        .execute_transaction(deposits.0)
        .join()
        .unwrap()
        .unwrap()
        .1
        .unwrap()[0];
    let z_idx2: u64 = tx_batch
        .execute_transaction(deposits.1)
        .join()
        .unwrap()
        .unwrap()
        .1
        .unwrap()[0];

    // ---------------------------------------------------------------------------------------

    let open_swap = get_dummy_open_order_swaps(note1, z_idx1, z_idx2);

    let perp_swap_resp = tx_batch
        .execute_perpetual_transaction(open_swap)
        .join()
        .unwrap();

    // * ======================================================================================================== *

    let close_swap = get_dummy_close_swaps((
        perp_swap_resp
            .as_ref()
            .unwrap()
            .position_a
            .as_ref()
            .unwrap()
            .clone(),
        perp_swap_resp.unwrap().position_b.unwrap(),
    ));

    let _perp_swap_resp = tx_batch
        .execute_perpetual_transaction(close_swap)
        .join()
        .unwrap();

    println!(
        "perp_state: {:?}",
        tx_batch.perpetual_state_tree.lock().leaf_nodes
    );
    println!("state: {:?}", tx_batch.state_tree.lock().leaf_nodes);

    tx_batch.finalize_batch().expect("err");

    //

    //

    //
}

//

//

//

fn get_dummy_deposits() -> (Deposit, Deposit) {
    let pk1 = BigInt::from_str("89327498632463297463274632493286132").unwrap();
    let address1 = EcPoint::from_priv_key(&pk1);

    let note1 = Note::new(
        0,
        address1.clone(),
        55555,
        1000_000_000,
        BigUint::from_str("1").unwrap(),
    );

    let deposit1 = Deposit::new(
        0,
        55555,
        1000_000_000,
        address1.x.to_biguint().unwrap(),
        vec![note1.clone()],
        &pk1.to_biguint().unwrap(),
    );

    let deposit2 = Deposit::new(
        1,
        55555,
        1000_000_000,
        address1.x.to_biguint().unwrap(),
        vec![note1],
        &pk1.to_biguint().unwrap(),
    );

    return (deposit1, deposit2);
}

fn get_dummy_open_order_swaps(note_: Note, z_idx1: u64, z_idx2: u64) -> PerpSwap {
    let pk1 = BigInt::from_str("89327498632463297463274632493286132").unwrap();
    let address1 = EcPoint::from_priv_key(&pk1).x.to_biguint().unwrap();
    let pk2 = BigInt::from_str("32875426429834347923748248234235723").unwrap();
    let address2 = EcPoint::from_priv_key(&pk2).x.to_biguint().unwrap();

    let mut note1 = note_.clone();
    note1.index = z_idx1;
    let refund_note1 = Note::new(
        z_idx1,
        note1.address.clone(),
        note1.token,
        250_000_000,
        note1.blinding.clone(),
    );

    let order_id = 1;
    let expiration_timestamp = 10000;
    let order_side = OrderSide::Long;
    let synthetic_token = 54321;
    let collateral_token = note1.token;
    let synthetic_amount = 1_000_000;
    let collateral_amount = 1000_000_000;
    let initial_margin = 750_000_000;
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

    // _____---------------------------------------------------------------

    let mut note2 = note_.clone();
    note2.index = z_idx2;
    let refund_note2 = Note::new(
        z_idx2,
        note2.address.clone(),
        note2.token,
        500_000_000,
        note2.blinding.clone(),
    );

    let order_id = 3;
    let expiration_timestamp = 10000;
    let order_side = OrderSide::Short;
    let synthetic_token = 54321;
    let collateral_token = note2.token;
    let synthetic_amount = 1_000_000;
    let collateral_amount = 1000_000_000;
    let initial_margin = 500_000_000;
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
        Some(vec![&pk1.to_biguint().unwrap()]),
        None,
    );
    let signature2 = perp_order2.sign_order(
        //
        Some(vec![&pk1.to_biguint().unwrap()]),
        None,
    );

    let swap = PerpSwap::new(
        perp_order1.clone(),
        perp_order2,
        Some(signature1),
        Some(signature2),
        1000_000_000,
        1_000_000,
        0,
        0,
    );

    return swap;
}

fn get_dummy_close_swaps(positions: (PerpPosition, PerpPosition)) -> PerpSwap {
    let pk1 = BigInt::from_str("89327498632463297463274632493286132").unwrap();
    let pk2 = BigInt::from_str("32875426429834347923748248234235723").unwrap();
    let address1 = EcPoint::from_priv_key(&pk1);

    let close_order_fields = CloseOrderFields {
        dest_received_address: address1.clone(),
        dest_received_blinding: BigUint::zero(),
    };

    let pos1 = positions.0;
    let pos2 = positions.1;

    let perp_order1 = PerpOrder::new_close_order(
        3,
        1000,
        pos1.clone(),
        pos2.order_side.clone(),
        pos1.synthetic_token,
        pos1.position_size,
        1200_000_000,
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
        1200_000_000,
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
        1200_000_000,
        1_000_000,
        0,
        0,
    );

    println!("swap: {:#?}", swap);

    return swap;
}

//

//

//

//

// fn get_dummy_open_order_swaps(note_: Note, z_idx1: u64, z_idx2: u64) -> PerpSwap {
//     let pk1 = BigInt::from_str("89327498632463297463274632493286132").unwrap();
//     let address1 = EcPoint::from_priv_key(&pk1);
//     let pk2 = BigInt::from_str("32875426429834347923748248234235723").unwrap();
//     let address2 = EcPoint::from_priv_key(&pk2);

//     let mut note1 = note_.clone();
//     note1.index = z_idx1;
//     let refund_note1 = Note::new(
//         z_idx1,
//         note1.address.clone(),
//         note1.token,
//         0,
//         note1.blinding.clone(),
//     );

//     let order_id = 1;
//     let expiration_timestamp = 10000;
//     let order_side = OrderSide::Long;
//     let synthetic_token = 54321;
//     let collateral_token = note1.token;
//     let synthetic_amount = 1_000_000;
//     let collateral_amount = 1000_000_000;
//     let initial_margin = 1000_000_000;
//     let fee_limit = 0;
//     let notes_in = vec![note1];
//     let refund_note = refund_note1;

//     let open_order_fields = OpenOrderFields {
//         initial_margin,
//         collateral_token,
//         notes_in,
//         refund_note,
//         position_address: address1.clone(),
//         blinding: BigUint::one(),
//     };

//     let perp_order1 = PerpOrder::new_open_order(
//         order_id,
//         expiration_timestamp,
//         address1.x.to_string(),
//         order_side,
//         synthetic_token,
//         synthetic_amount,
//         collateral_amount,
//         fee_limit,
//         open_order_fields,
//     );

//     // _____---------------------------------------------------------------

//     let mut note2 = note_.clone();
//     note2.index = z_idx2;
//     let refund_note2 = Note::new(
//         z_idx2,
//         note2.address.clone(),
//         note2.token,
//         0,
//         note2.blinding.clone(),
//     );

//     let order_id = 3;
//     let expiration_timestamp = 10000;
//     let order_side = OrderSide::Short;
//     let synthetic_token = 54321;
//     let collateral_token = note2.token;
//     let synthetic_amount = 1_000_000;
//     let collateral_amount = 1000_000_000;
//     let initial_margin = 1000_000_000;
//     let fee_limit = 0;
//     let notes_in = vec![note2];
//     let refund_note = refund_note2;

//     let open_order_fields = OpenOrderFields {
//         initial_margin,
//         collateral_token,
//         notes_in,
//         refund_note,
//         position_address: address2.clone(),
//         blinding: BigUint::one(),
//     };

//     let perp_order2 = PerpOrder::new_open_order(
//         order_id,
//         expiration_timestamp,
//         address2.x.to_string(),
//         order_side,
//         synthetic_token,
//         synthetic_amount,
//         collateral_amount,
//         fee_limit,
//         open_order_fields,
//     );

//     let signature1 = perp_order1.sign_order(
//         //
//         Some(vec![&pk1.to_biguint().unwrap()]),
//         None,
//     );
//     let signature2 = perp_order2.sign_order(
//         //
//         Some(vec![&pk1.to_biguint().unwrap()]),
//         None,
//     );

//     let swap = PerpSwap::new(
//         perp_order1.clone(),
//         perp_order2,
//         Some(signature1),
//         Some(signature2),
//         1000_000_000,
//         1_000_000,
//         0,
//         0,
//     );

//     return swap;
// }
