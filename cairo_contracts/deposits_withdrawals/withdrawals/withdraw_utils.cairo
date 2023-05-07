from starkware.cairo.common.cairo_builtins import HashBuiltin, SignatureBuiltin
from starkware.cairo.common.hash import hash2
from starkware.cairo.common.alloc import alloc
from starkware.cairo.common.registers import get_fp_and_pc
from starkware.cairo.common.signature import verify_ecdsa_signature
from starkware.cairo.common.dict import dict_new, dict_write, dict_update, dict_squash, dict_read
from starkware.cairo.common.dict_access import DictAccess
from starkware.cairo.common.math import unsigned_div_rem, assert_le
from starkware.cairo.common.math_cmp import is_le
from starkware.cairo.common.hash_state import (
    hash_init,
    hash_finalize,
    hash_update,
    hash_update_single,
)
from starkware.cairo.common.ec_point import EcPoint

from helpers.utils import Note, hash_note, sum_notes, hash_notes_array
from helpers.signatures.signatures import sum_pub_keys

// & This is the public output sent on-chain
struct Withdrawal {
    token: felt,
    amount: felt,
    withdrawal_address: felt,  // This should be the eth address to withdraw from
}

// & Gets the notes that the user wants to spend as the input
// & The notes should sum to at least the amount the user wants to withdraw
// & The rest is refunded back to him
func get_withdraw_and_refund_notes() -> (
    withdraw_notes_len: felt, withdraw_notes: Note*, refund_note: Note
) {
    alloc_locals;

    local withdraw_notes_len: felt;
    local withdraw_notes: Note*;
    local refund_note: Note;

    let (__fp__, _) = get_fp_and_pc();
    handle_inputs(&withdraw_notes_len, &withdraw_notes, &refund_note);

    return (withdraw_notes_len, withdraw_notes, refund_note);
}

func verify_withdraw_notes{
    pedersen_ptr: HashBuiltin*, range_check_ptr, ecdsa_ptr: SignatureBuiltin*
}(withdraw_notes_len: felt, withdraw_notes: Note*, refund_note: Note, withdrawal: Withdrawal) {
    alloc_locals;

    // ? Sum the notes and verify that the amount is correct
    let (withdraw_notes_sum) = sum_notes(withdraw_notes_len, withdraw_notes, withdrawal.token, 0);
    assert withdraw_notes_sum = withdrawal.amount + refund_note.amount;

    // ? Hash the withdraw notes to verify signature
    let (local empty_arr: felt*) = alloc();
    let (note_hashes_len: felt, note_hashes: felt*) = hash_notes_array(
        withdraw_notes_len, withdraw_notes, 0, empty_arr
    );
    let (refund_hash: felt) = hash_note(refund_note);

    let (withdraw_hash: felt) = withdraw_tx_hash(
        note_hashes_len, note_hashes, refund_hash, withdrawal
    );

    local signature_r: felt;
    local signature_s: felt;
    %{
        sig = current_withdrawal["signature"]
        ids.signature_r = int(sig[0])
        ids.signature_s = int(sig[1])
    %}

    let (pub_key_sum: EcPoint) = sum_pub_keys(withdraw_notes_len, withdraw_notes, EcPoint(0, 0));

    verify_ecdsa_signature(
        message=withdraw_hash,
        public_key=pub_key_sum.x,
        signature_r=signature_r,
        signature_s=signature_s,
    );

    return ();
}

func withdraw_tx_hash{pedersen_ptr: HashBuiltin*}(
    note_hashes_len: felt, note_hashes: felt*, refund_hash: felt, withdrawal: Withdrawal
) -> (res: felt) {
    let hash_ptr = pedersen_ptr;
    with hash_ptr {
        let (hash_state_ptr) = hash_init();
        let (hash_state_ptr) = hash_update_single(hash_state_ptr, withdrawal.withdrawal_address);
        let (hash_state_ptr) = hash_update_single(hash_state_ptr, refund_hash);
        let (hash_state_ptr) = hash_update(hash_state_ptr, note_hashes, note_hashes_len);
        let (res) = hash_finalize(hash_state_ptr);
        let pedersen_ptr = hash_ptr;
        return (res=res);
    }
}

func handle_inputs(notes_len: felt*, notes: Note**, refund_note: Note*) {
    %{
        withdraw_notes = current_withdrawal["notes_in"]

        memory[ids.notes_len] = len(withdraw_notes)
        memory[ids.notes] = notes = segments.add()
        for i, note in enumerate(withdraw_notes):
            memory[notes + i*NOTE_SIZE + ADDRESS_OFFSET+0] = int(note["address"]["x"])
            memory[notes + i*NOTE_SIZE + ADDRESS_OFFSET+1] = int(note["address"]["y"])
            memory[notes + i*NOTE_SIZE + TOKEN_OFFSET] = int(current_withdrawal["withdrawal_token"])
            memory[notes + i*NOTE_SIZE + AMOUNT_OFFSET] = int(note["amount"])
            memory[notes + i*NOTE_SIZE + BLINDING_FACTOR_OFFSET] = int(note["blinding"])
            memory[notes + i*NOTE_SIZE + INDEX_OFFSET] = int(note["index"])
            memory[notes + i*NOTE_SIZE + HASH_OFFSET] = int(note["hash"])

        # REFUND NOTE ==============================

        refund_note__ = current_withdrawal["refund_note"]
        if refund_note__ is not None:
            memory[ids.refund_note.address_ + ADDRESS_OFFSET+0] = int(refund_note__["address"]["x"])
            memory[ids.refund_note.address_ + ADDRESS_OFFSET+1] = int(refund_note__["address"]["y"])
            memory[ids.refund_note.address_ + TOKEN_OFFSET] = int(current_withdrawal["withdrawal_token"])
            memory[ids.refund_note.address_ + AMOUNT_OFFSET] = int(refund_note__["amount"])
            memory[ids.refund_note.address_ + BLINDING_FACTOR_OFFSET] = int(refund_note__["blinding"])
            memory[ids.refund_note.address_ + INDEX_OFFSET] = int(refund_note__["index"])
            memory[ids.refund_note.address_ + HASH_OFFSET] = int(refund_note__["hash"])
        else:
            memory[ids.refund_note.address_ + ADDRESS_OFFSET+0] = 0
            memory[ids.refund_note.address_ + ADDRESS_OFFSET+1] = 0
            memory[ids.refund_note.address_ + TOKEN_OFFSET] = 0
            memory[ids.refund_note.address_ + AMOUNT_OFFSET] = 0
            memory[ids.refund_note.address_ + BLINDING_FACTOR_OFFSET] = 0
            memory[ids.refund_note.address_ + INDEX_OFFSET] = 0
            memory[ids.refund_note.address_ + HASH_OFFSET] = 0
    %}

    return ();
}
