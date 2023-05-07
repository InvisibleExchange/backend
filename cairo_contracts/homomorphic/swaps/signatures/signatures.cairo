// %builtins output pedersen range_check

from starkware.cairo.common.cairo_builtins import HashBuiltin
from starkware.cairo.common.alloc import alloc
from starkware.cairo.common.hash import hash2
from starkware.cairo.common.math import split_felt, unsigned_div_rem
from starkware.cairo.common.cairo_secp.bigint import BigInt3, bigint_to_uint256, uint256_to_bigint
from starkware.cairo.common.cairo_secp.ec import EcPoint, ec_double, ec_add, ec_mul, ec_negate
from starkware.cairo.common.uint256 import Uint256
from starkware.cairo.common.hash_state import (
    hash_init,
    hash_finalize,
    hash_update,
    hash_update_single,
)

from helpers.utils import generator

func main{output_ptr, pedersen_ptr: HashBuiltin*, range_check_ptr}() {
    alloc_locals;

    // let (local G : EcPoint) = generator()
    local addresses_len: felt;
    local addresses: EcPoint*;
    local tx_hash: felt;
    local c: felt;
    local rs_len: felt;
    local rs: felt*;
    %{
        ids.tx_hash = program_input["tx_hash"]
        sig = program_input["signature"]
        ids.c = sig[0]

        addresses_ = program_input["addresses"]
        ids.addresses_len = len(addresses_)
        ids.addresses = addresses = segments.add()

        POINT_SIZE = ids.EcPoint.SIZE
        X_OFFSET = ids.EcPoint.x
        Y_OFFSET = ids.EcPoint.y

        BIG_INT_SIZE = ids.BigInt3.SIZE
        BIG_INT_0_OFFSET = ids.BigInt3.d0
        BIG_INT_1_OFFSET = ids.BigInt3.d1
        BIG_INT_2_OFFSET = ids.BigInt3.d2

        for i, addr in enumerate(addresses_):
            # addr = [x:[d0,d1,d2], y:[d0,d1,d2]]
            addr_x = addresses + POINT_SIZE * i + X_OFFSET
            addr_y = addresses + POINT_SIZE * i + Y_OFFSET

            memory[addr_x + BIG_INT_0_OFFSET] = addr[0][0]
            memory[addr_x + BIG_INT_1_OFFSET] = addr[0][1]
            memory[addr_x + BIG_INT_2_OFFSET] = addr[0][2]

            memory[addr_y + BIG_INT_0_OFFSET] = addr[1][0]
            memory[addr_y + BIG_INT_1_OFFSET] = addr[1][1]
            memory[addr_y + BIG_INT_2_OFFSET] = addr[1][2]

        rs_ = sig[1:]
        ids.rs_len = len(rs_)
        ids.rs = rs = segments.add()
        for i, val in enumerate(rs_):
            memory[rs + i] = val
    %}

    verify_sig(addresses_len, addresses, tx_hash, c, rs_len, rs);

    %{ print("All good") %}

    return ();
}

func verify_sig{output_ptr, pedersen_ptr: HashBuiltin*, range_check_ptr}(
    addresses_len: felt, addresses: EcPoint*, tx_hash: felt, c: felt, rs_len: felt, rs: felt*
) {
    alloc_locals;

    let (local empty_arr: felt*) = alloc();

    let (c_inputs_len: felt, c_inputs: felt*) = build_c_inputs_array(
        addresses_len, addresses, tx_hash, c, rs_len, rs, 0, empty_arr
    );

    let (c_prime: felt) = get_c_prime(tx_hash, c_inputs_len, c_inputs);

    with_attr error_message("!====== (c_prime and c don't match verify_sig) ======!") {
        assert c_prime = c;
    }

    return ();
}

// initial value of c_inputs should be an empty array
func build_c_inputs_array{output_ptr, pedersen_ptr: HashBuiltin*, range_check_ptr}(
    addresses_len: felt,
    addresses: EcPoint*,
    tx_hash: felt,
    c: felt,
    rs_len: felt,
    rs: felt*,
    c_inputs_len: felt,
    c_inputs: felt*,
) -> (c_inputs_len: felt, c_inputs: felt*) {
    alloc_locals;

    if (addresses_len == 0) {
        return (c_inputs_len, c_inputs);
    }

    // c_input_point = rG + K - c*G
    let (c_input_point: EcPoint) = get_c_input(addresses[0], c, rs[0]);
    // cx is the x coordinate of c_input_point
    // (NOTE could colide with c_input_point + P_infinity - check if thats sound maybe use y point instead or both)
    let (cx: Uint256) = bigint_to_uint256(c_input_point.x);
    // c_input is the hash of the high and low bits of cx
    let (c_input: felt) = hash2{hash_ptr=pedersen_ptr}(cx.high, cx.low);

    assert c_inputs[c_inputs_len] = c_input;

    return build_c_inputs_array(
        addresses_len - 1, &addresses[1], tx_hash, c, rs_len - 1, &rs[1], c_inputs_len + 1, c_inputs
    );
}

func get_c_input{output_ptr, pedersen_ptr: HashBuiltin*, range_check_ptr}(
    address: EcPoint, c_: felt, r_: felt
) -> (res: EcPoint) {
    alloc_locals;
    let (local G: EcPoint) = generator();

    let (c_high: felt, c_low: felt) = split_felt(c_);
    let c_trimmed: felt = c_high + c_low;

    let (high, low) = split_felt(c_trimmed);
    let _c: Uint256 = Uint256(low=low, high=high);
    let (c: BigInt3) = uint256_to_bigint(_c);

    let (high, low) = split_felt(r_);
    let _r: Uint256 = Uint256(low=low, high=high);
    let (r: BigInt3) = uint256_to_bigint(_r);

    // c_input = rG - K + c*G
    let (rG: EcPoint) = ec_mul(G, r);
    let (cG: EcPoint) = ec_mul(G, c);

    let (K_neg: EcPoint) = ec_negate(address);

    let (rG_minus_K: EcPoint) = ec_add(rG, K_neg);

    let (c_input: EcPoint) = ec_add(rG_minus_K, cG);

    return (c_input,);
}

func get_c_prime{output_ptr, pedersen_ptr: HashBuiltin*, range_check_ptr}(
    tx_hash: felt, c_input_xs_len: felt, c_input_xs: felt*
) -> (res: felt) {
    alloc_locals;

    // c_prime = H(tx_hash, c_input_1, ..., c_input_n)
    let hash_ptr = pedersen_ptr;
    with hash_ptr {
        let (hash_state_ptr) = hash_init();
        let (hash_state_ptr) = hash_update_single(hash_state_ptr, tx_hash);
        let (hash_state_ptr) = hash_update(hash_state_ptr, c_input_xs, c_input_xs_len);

        let (res) = hash_finalize(hash_state_ptr);
        let pedersen_ptr = hash_ptr;
        return (res=res);
    }
}
