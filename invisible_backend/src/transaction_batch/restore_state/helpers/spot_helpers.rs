use num_bigint::{BigInt, BigUint};
use serde_json::{Map, Value};
use std::str::FromStr;

use crate::{
    order_tab::{OrderTab, TabHeader},
    perpetual::DUST_AMOUNT_PER_ASSET,
    utils::crypto_utils::EcPoint,
    utils::notes::Note,
};

pub fn rebuild_swap_note(transaction: &Map<String, Value>, is_a: bool) -> Note {
    let order_indexes_json = transaction
        .get("indexes")
        .unwrap()
        .get(if is_a { "order_a" } else { "order_b" })
        .unwrap();

    let swap_idx = order_indexes_json
        .get("swap_note_idx")
        .unwrap()
        .as_u64()
        .unwrap();

    let order_json: &Value = transaction
        .get("swap_data")
        .unwrap()
        .get(if is_a { "order_a" } else { "order_b" })
        .unwrap();
    let spot_note_info = order_json.get("spot_note_info").unwrap();
    let dest_received_address = spot_note_info.get("dest_received_address").unwrap();
    let address = EcPoint {
        x: BigInt::from_str(dest_received_address.get("x").unwrap().as_str().unwrap()).unwrap(),
        y: BigInt::from_str(dest_received_address.get("y").unwrap().as_str().unwrap()).unwrap(),
    };

    let dest_received_blinding = BigUint::from_str(
        spot_note_info
            .get("dest_received_blinding")
            .unwrap()
            .as_str()
            .unwrap(),
    )
    .unwrap();

    let spent_amount_y = transaction
        .get("swap_data")
        .unwrap()
        .get(if is_a {
            "spent_amount_b"
        } else {
            "spent_amount_a"
        })
        .unwrap()
        .as_u64()
        .unwrap();

    let fee_taken_x = transaction
        .get("swap_data")
        .unwrap()
        .get(if is_a { "fee_taken_a" } else { "fee_taken_b" })
        .unwrap()
        .as_u64()
        .unwrap();

    let token_received = order_json.get("token_received").unwrap().as_u64().unwrap();

    return Note::new(
        swap_idx,
        address,
        token_received as u32,
        spent_amount_y - fee_taken_x,
        dest_received_blinding,
    );
}

pub fn restore_partial_fill_refund_note(
    transaction: &Map<String, Value>,
    is_a: bool,
) -> Option<Note> {
    let order = transaction
        .get("swap_data")
        .unwrap()
        .get(if is_a { "order_a" } else { "order_b" })
        .unwrap();

    let prev_pfr_note = transaction.get(if is_a {
        "prev_pfr_note_a"
    } else {
        "prev_pfr_note_b"
    });

    let new_partial_refund_amount = if !prev_pfr_note.unwrap().is_null() {
        prev_pfr_note
            .unwrap()
            .get("amount")
            .unwrap()
            .as_u64()
            .unwrap()
            - transaction
                .get("swap_data")
                .unwrap()
                .get(if is_a {
                    "spent_amount_a"
                } else {
                    "spent_amount_b"
                })
                .unwrap()
                .as_u64()
                .unwrap()
    } else {
        order.get("amount_spent").unwrap().as_u64().unwrap()
            - transaction
                .get("swap_data")
                .unwrap()
                .get(if is_a {
                    "spent_amount_a"
                } else {
                    "spent_amount_b"
                })
                .unwrap()
                .as_u64()
                .unwrap()
    };

    if new_partial_refund_amount
        <= DUST_AMOUNT_PER_ASSET[&order
            .get("token_spent")
            .unwrap()
            .as_u64()
            .unwrap()
            .to_string()]
    {
        return None;
    }

    let idx = transaction
        .get("indexes")
        .unwrap()
        .get(if is_a { "order_a" } else { "order_b" })
        .unwrap()
        .get("partial_fill_idx")
        .unwrap()
        .as_u64()
        .unwrap();

    let spot_note_info = &order.get("spot_note_info").unwrap();
    let note0 = &spot_note_info.get("notes_in").unwrap().as_array().unwrap()[0];

    return Some(Note::new(
        idx,
        EcPoint::new(
            &BigUint::from_str(
                note0
                    .get("address")
                    .unwrap()
                    .get("x")
                    .unwrap()
                    .as_str()
                    .unwrap(),
            )
            .unwrap(),
            &BigUint::from_str(
                note0
                    .get("address")
                    .unwrap()
                    .get("y")
                    .unwrap()
                    .as_str()
                    .unwrap(),
            )
            .unwrap(),
        ),
        order.get("token_spent").unwrap().as_u64().unwrap() as u32,
        new_partial_refund_amount,
        BigUint::from_str(note0.get("blinding").unwrap().as_str().unwrap()).unwrap(),
    ));
}

// * ORDER TABS * //

// * Order tabs ** //
pub fn get_updated_order_tab(transaction: &Map<String, Value>, is_a: bool) -> OrderTab {
    // ? Get the info --------------------------

    let tab_json = transaction
        .get(if is_a {
            "prev_order_tab_a"
        } else {
            "prev_order_tab_b"
        })
        .unwrap();

    let order_json: &Value = transaction
        .get("swap_data")
        .unwrap()
        .get(if is_a { "order_a" } else { "order_b" })
        .unwrap();
    let token_received = order_json.get("token_received").unwrap().as_u64().unwrap() as u32;

    let spent_amount_x = transaction
        .get("swap_data")
        .unwrap()
        .get(if is_a {
            "spent_amount_a"
        } else {
            "spent_amount_b"
        })
        .unwrap()
        .as_u64()
        .unwrap();

    let spent_amount_y = transaction
        .get("swap_data")
        .unwrap()
        .get(if is_a {
            "spent_amount_b"
        } else {
            "spent_amount_a"
        })
        .unwrap()
        .as_u64()
        .unwrap();

    let fee_taken_x = transaction
        .get("swap_data")
        .unwrap()
        .get(if is_a { "fee_taken_a" } else { "fee_taken_b" })
        .unwrap()
        .as_u64()
        .unwrap();

    // ? Make the update

    let mut order_tab = order_tab_from_json(tab_json);

    let is_buy = order_tab.tab_header.base_token == token_received;

    if is_buy {
        order_tab.quote_amount -= spent_amount_x;
        order_tab.base_amount += spent_amount_y - fee_taken_x;
    } else {
        order_tab.base_amount -= spent_amount_x;
        order_tab.quote_amount += spent_amount_y - fee_taken_x;
    }

    order_tab.update_hash();

    return order_tab;
}

pub fn open_new_tab(transaction: &Map<String, Value>) -> OrderTab {
    let add_only = transaction.get("add_only").unwrap().as_bool().unwrap();

    if add_only {
        let order_tab = transaction.get("order_tab").unwrap();
        let order_tab = order_tab_from_json(order_tab);

        return order_tab;
    } else {
        let prev_order_tab = transaction.get("order_tab").unwrap();
        let mut order_tab = order_tab_from_json(prev_order_tab);

        let base_amount = sum_notes_in(
            transaction
                .get("base_notes_in")
                .unwrap()
                .as_array()
                .unwrap(),
            transaction.get("base_refund_note").unwrap(),
        );
        let quote_amount = sum_notes_in(
            transaction
                .get("quote_notes_in")
                .unwrap()
                .as_array()
                .unwrap(),
            transaction.get("quote_refund_note").unwrap(),
        );

        order_tab.base_amount += base_amount;
        order_tab.quote_amount += quote_amount;

        order_tab.update_hash();

        return order_tab;
    }
}

pub fn close_tab(
    transaction: &Map<String, Value>,
    order_tab: OrderTab,
) -> (Note, Note, Option<OrderTab>) {
    // ? GENERATE THE RETURN NOTES -------------------

    let base_return_note = get_return_note_info(transaction, &order_tab, true);

    let quote_return_note = get_return_note_info(transaction, &order_tab, false);

    let base_amount_change = transaction
        .get("base_amount_change")
        .unwrap()
        .as_u64()
        .unwrap();
    let quote_amount_change = transaction
        .get("quote_amount_change")
        .unwrap()
        .as_u64()
        .unwrap();

    let updated_base_amount = order_tab.base_amount - base_amount_change;
    let updated_quote_amount = order_tab.quote_amount - quote_amount_change;

    let updated_order_tab;
    if (updated_base_amount > DUST_AMOUNT_PER_ASSET[&order_tab.tab_header.base_token.to_string()])
        && (updated_quote_amount
            > DUST_AMOUNT_PER_ASSET[&order_tab.tab_header.quote_token.to_string()])
    {
        updated_order_tab = Some(OrderTab::new(
            order_tab.tab_header.clone(),
            updated_base_amount,
            updated_quote_amount,
        ));
    } else {
        updated_order_tab = None;
    }

    return (base_return_note, quote_return_note, updated_order_tab);
}

// * HELPERS * //

pub fn note_from_json(note_json: &Value) -> Note {
    let index = note_json.get("index").unwrap().as_u64().unwrap();
    let token = note_json.get("token").unwrap().as_u64().unwrap() as u32;
    let amount = note_json.get("amount").unwrap().as_u64().unwrap();

    let addr = note_json.get("address").unwrap();
    let address = EcPoint::new(
        &BigUint::from_str(addr.get("x").unwrap().as_str().unwrap()).unwrap(),
        &BigUint::from_str(addr.get("y").unwrap().as_str().unwrap()).unwrap(),
    );
    let blinding = BigUint::from_str(note_json.get("blinding").unwrap().as_str().unwrap()).unwrap();

    return Note::new(index, address, token, amount, blinding);
}

pub fn order_tab_from_json(tab_json: &Value) -> OrderTab {
    let tab_header = tab_json.get("tab_header").unwrap();

    let tab_header = TabHeader::new(
        tab_header.get("base_token").unwrap().as_u64().unwrap() as u32,
        tab_header.get("quote_token").unwrap().as_u64().unwrap() as u32,
        BigUint::from_str(tab_header.get("base_blinding").unwrap().as_str().unwrap()).unwrap(),
        BigUint::from_str(tab_header.get("quote_blinding").unwrap().as_str().unwrap()).unwrap(),
        BigUint::from_str(tab_header.get("pub_key").unwrap().as_str().unwrap()).unwrap(),
    );

    return OrderTab::new(
        tab_header,
        tab_json.get("base_amount").unwrap().as_u64().unwrap(),
        tab_json.get("quote_amount").unwrap().as_u64().unwrap(),
    );
}

fn sum_notes_in(notes_in: &Vec<Value>, refund_note: &Value) -> u64 {
    let mut sum = 0;
    for note in notes_in {
        sum += note.get("amount").unwrap().as_u64().unwrap();
    }
    sum -= refund_note.get("amount").unwrap().as_u64().unwrap();
    return sum;
}

fn get_return_note_info(
    transaction: &Map<String, Value>,
    order_tab: &OrderTab,
    is_base: bool,
) -> Note {
    let return_note_idx = transaction
        .get(if is_base {
            "base_return_note"
        } else {
            "quote_return_note"
        })
        .unwrap()
        .get("index")
        .unwrap()
        .as_u64()
        .unwrap();
    let close_order_fields = transaction
        .get(if is_base {
            "base_close_order_fields"
        } else {
            "quote_close_order_fields"
        })
        .unwrap();
    let address = close_order_fields.get("dest_received_address").unwrap();
    let address = EcPoint::new(
        &BigUint::from_str(address.get("x").unwrap().as_str().unwrap()).unwrap(),
        &BigUint::from_str(address.get("y").unwrap().as_str().unwrap()).unwrap(),
    );
    let token = if is_base {
        order_tab.tab_header.base_token
    } else {
        order_tab.tab_header.quote_token
    };
    let amount_change = transaction
        .get(if is_base {
            "base_amount_change"
        } else {
            "quote_amount_change"
        })
        .unwrap()
        .as_u64()
        .unwrap();
    let blinding = BigUint::from_str(
        close_order_fields
            .get("dest_received_blinding")
            .unwrap()
            .as_str()
            .unwrap(),
    )
    .unwrap();

    let return_note = Note::new(return_note_idx, address, token, amount_change, blinding);

    return return_note;
}
