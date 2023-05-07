use num_bigint::BigUint;

use crate::utils::crypto_utils::pedersen;

pub fn pairwise_hash(array: &Vec<BigUint>) -> Vec<BigUint> {
    if array.len() % 2 != 0 {
        panic!("Array length must be even");
    }

    let mut hashes: Vec<BigUint> = Vec::new();
    for i in (0..array.len()).step_by(2) {
        let hash: BigUint = pedersen(&array[i], &array[i + 1]);
        hashes.push(hash);
    }

    return hashes;
}

pub fn idx_to_binary_pos(idx: u64, bin_length: usize) -> Vec<i8> {
    // bin_length = depth

    let bin_chars = format!("{idx:b}");

    if bin_chars.len() > bin_length {
        println!("idx: {}", idx);
    }

    assert!(
        bin_chars.len() <= bin_length,
        "index is to big to fit on the tree"
    );

    let mut bin_pos: Vec<i8> = Vec::new();

    for ch in bin_chars.chars() {
        bin_pos.push(ch.to_digit(10).unwrap() as i8)
    }

    for _ in 0..bin_length - bin_chars.len() {
        bin_pos.insert(0, 0);
    }

    bin_pos.reverse();

    return bin_pos;
}

pub fn proof_pos(leaf_idx: u64, depth: usize) -> Vec<u64> {
    let mut proof_pos: Vec<u64> = Vec::new();
    let proof_binary_pos = idx_to_binary_pos(leaf_idx, depth);

    if leaf_idx % 2 == 0 {
        proof_pos.push(leaf_idx + 1);
    } else {
        proof_pos.push(leaf_idx - 1);
    }

    for i in 1..depth {
        if proof_binary_pos[i] == 1 {
            let pos_i = proof_pos[i - 1] / 2 - 1;
            proof_pos.push(pos_i);
        } else {
            let pos_i = proof_pos[i - 1] / 2 + 1;
            proof_pos.push(pos_i);
        }
    }

    return proof_pos;
}

pub const ZERO_HASHES: [[u8; 32]; 64] = [
    [
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0,
    ],
    [
        4, 104, 202, 80, 64, 222, 31, 85, 51, 119, 148, 34, 16, 11, 107, 113, 22, 159, 89, 235,
        135, 27, 238, 0, 7, 96, 193, 168, 235, 227, 158, 4,
    ],
    [
        173, 1, 250, 11, 222, 1, 49, 1, 91, 229, 149, 42, 52, 13, 250, 203, 254, 66, 250, 176, 151,
        229, 162, 29, 7, 98, 37, 252, 218, 61, 57, 7,
    ],
    [
        216, 56, 4, 111, 3, 81, 91, 82, 91, 10, 201, 217, 6, 170, 123, 138, 196, 246, 245, 188,
        182, 244, 159, 23, 98, 193, 170, 94, 228, 68, 59, 6,
    ],
    [
        241, 178, 89, 236, 119, 180, 226, 92, 38, 157, 249, 61, 153, 57, 172, 164, 221, 185, 246,
        2, 10, 164, 216, 128, 197, 224, 45, 210, 255, 203, 48, 7,
    ],
    [
        195, 93, 55, 92, 58, 192, 29, 79, 206, 125, 148, 139, 3, 152, 18, 151, 52, 63, 236, 139,
        234, 213, 58, 158, 100, 123, 203, 164, 227, 207, 157, 3,
    ],
    [
        18, 75, 71, 68, 7, 231, 34, 166, 7, 15, 8, 69, 67, 31, 5, 30, 53, 55, 248, 108, 202, 184,
        78, 99, 118, 192, 32, 234, 122, 65, 4, 6,
    ],
    [
        220, 202, 227, 143, 167, 55, 215, 52, 129, 154, 202, 21, 208, 65, 86, 16, 95, 60, 212, 170,
        111, 73, 156, 8, 199, 196, 45, 236, 37, 132, 202, 5,
    ],
    [
        154, 134, 250, 188, 96, 254, 171, 33, 6, 15, 103, 199, 187, 217, 90, 243, 84, 83, 9, 222,
        199, 178, 199, 62, 48, 54, 64, 130, 56, 47, 55, 5,
    ],
    [
        45, 15, 90, 189, 95, 46, 240, 32, 87, 244, 129, 15, 137, 175, 201, 69, 224, 13, 222, 248,
        77, 46, 164, 164, 104, 73, 9, 242, 42, 136, 241, 0,
    ],
    [
        131, 78, 124, 163, 201, 40, 113, 94, 139, 107, 174, 154, 86, 223, 238, 117, 170, 111, 92,
        195, 15, 237, 36, 151, 207, 82, 105, 184, 17, 191, 254, 0,
    ],
    [
        120, 188, 20, 253, 196, 136, 31, 243, 144, 175, 168, 242, 51, 225, 189, 22, 235, 198, 12,
        250, 212, 6, 222, 26, 92, 39, 251, 182, 64, 210, 94, 0,
    ],
    [
        147, 176, 219, 110, 37, 197, 20, 114, 178, 88, 158, 120, 212, 26, 7, 212, 211, 159, 123,
        206, 254, 134, 40, 34, 4, 142, 247, 60, 183, 181, 71, 1,
    ],
    [
        27, 96, 159, 81, 88, 24, 231, 106, 229, 237, 184, 225, 73, 157, 6, 91, 152, 121, 231, 13,
        60, 206, 88, 94, 174, 187, 73, 39, 172, 47, 187, 1,
    ],
    [
        18, 71, 8, 47, 96, 95, 63, 80, 88, 94, 121, 246, 224, 121, 139, 177, 235, 191, 142, 52,
        139, 224, 235, 166, 93, 141, 104, 142, 80, 14, 118, 6,
    ],
    [
        18, 33, 31, 250, 219, 144, 7, 198, 157, 122, 120, 84, 255, 195, 1, 141, 66, 175, 165, 189,
        109, 231, 185, 73, 144, 78, 0, 31, 165, 62, 143, 2,
    ],
    [
        81, 69, 231, 207, 52, 221, 198, 222, 9, 183, 60, 232, 145, 175, 59, 2, 32, 238, 178, 74,
        205, 135, 192, 4, 45, 252, 66, 214, 153, 123, 230, 1,
    ],
    [
        126, 0, 238, 51, 252, 31, 94, 223, 10, 201, 120, 93, 157, 0, 93, 52, 13, 49, 33, 30, 28,
        193, 23, 253, 126, 180, 17, 245, 51, 20, 59, 4,
    ],
    [
        66, 237, 61, 160, 221, 51, 7, 212, 167, 52, 126, 229, 37, 198, 253, 233, 254, 150, 82, 187,
        11, 43, 169, 195, 146, 69, 98, 81, 169, 56, 21, 0,
    ],
    [
        59, 217, 229, 198, 130, 155, 131, 254, 14, 183, 140, 253, 28, 196, 213, 161, 225, 161, 232,
        44, 157, 54, 133, 137, 143, 52, 209, 72, 178, 224, 69, 7,
    ],
    [
        6, 199, 194, 172, 104, 98, 102, 219, 104, 166, 43, 243, 232, 92, 243, 114, 141, 62, 209,
        73, 255, 214, 18, 206, 217, 233, 250, 42, 76, 23, 11, 1,
    ],
    [
        243, 211, 49, 154, 15, 246, 68, 185, 11, 17, 238, 196, 107, 77, 142, 94, 126, 20, 61, 95,
        22, 158, 85, 88, 167, 165, 181, 35, 254, 35, 162, 2,
    ],
    [
        1, 210, 251, 72, 217, 234, 155, 160, 17, 186, 61, 53, 219, 159, 72, 64, 16, 38, 43, 98, 88,
        255, 24, 160, 89, 12, 167, 188, 39, 182, 66, 7,
    ],
    [
        188, 119, 137, 17, 111, 71, 220, 78, 169, 58, 185, 171, 160, 147, 132, 38, 131, 179, 115,
        238, 222, 158, 94, 32, 50, 167, 30, 174, 31, 40, 47, 6,
    ],
    [
        69, 89, 234, 51, 140, 161, 248, 17, 75, 164, 121, 89, 29, 11, 127, 255, 112, 106, 142, 31,
        157, 18, 79, 92, 106, 42, 53, 42, 192, 130, 190, 0,
    ],
    [
        129, 84, 36, 160, 121, 29, 70, 185, 57, 34, 219, 205, 158, 121, 135, 145, 178, 130, 0, 107,
        234, 47, 147, 122, 177, 239, 204, 107, 13, 254, 221, 5,
    ],
    [
        28, 25, 18, 120, 147, 8, 170, 245, 235, 197, 215, 167, 237, 211, 22, 124, 248, 179, 215,
        172, 181, 81, 32, 210, 99, 87, 34, 97, 233, 225, 243, 6,
    ],
    [
        112, 70, 55, 227, 255, 131, 53, 255, 145, 8, 49, 253, 0, 110, 9, 46, 182, 206, 9, 202, 242,
        55, 119, 73, 33, 91, 87, 254, 172, 69, 188, 3,
    ],
    [
        86, 64, 235, 240, 109, 253, 219, 36, 29, 111, 230, 50, 146, 45, 50, 220, 146, 79, 105, 133,
        15, 104, 153, 201, 139, 170, 95, 72, 139, 33, 114, 5,
    ],
    [
        219, 110, 168, 148, 154, 113, 223, 99, 115, 196, 22, 98, 82, 113, 247, 192, 180, 252, 66,
        40, 112, 29, 120, 105, 245, 140, 236, 176, 52, 134, 49, 3,
    ],
    [
        181, 164, 146, 254, 73, 224, 20, 239, 228, 81, 90, 239, 203, 253, 27, 222, 251, 110, 185,
        83, 49, 210, 141, 255, 255, 226, 36, 60, 45, 45, 130, 2,
    ],
    [
        168, 185, 222, 140, 205, 98, 126, 183, 194, 19, 148, 198, 139, 70, 46, 228, 48, 63, 218,
        197, 208, 183, 7, 252, 234, 14, 236, 19, 157, 93, 105, 4,
    ],
    [
        47, 205, 189, 50, 17, 230, 112, 158, 134, 247, 39, 253, 34, 132, 139, 222, 77, 135, 66, 47,
        144, 141, 61, 73, 97, 180, 166, 37, 251, 3, 107, 5,
    ],
    [
        54, 145, 30, 120, 76, 152, 98, 222, 56, 27, 239, 181, 223, 22, 174, 251, 7, 120, 226, 113,
        30, 17, 55, 174, 139, 136, 150, 146, 126, 217, 135, 2,
    ],
    [
        220, 57, 163, 235, 6, 246, 250, 159, 68, 17, 213, 195, 158, 219, 101, 78, 142, 156, 187,
        224, 248, 181, 74, 231, 226, 202, 48, 185, 76, 68, 37, 3,
    ],
    [
        64, 147, 126, 169, 96, 250, 35, 79, 120, 46, 60, 184, 92, 58, 46, 54, 179, 126, 188, 162,
        254, 187, 190, 47, 124, 129, 127, 207, 166, 31, 68, 5,
    ],
    [
        92, 88, 20, 68, 113, 55, 67, 204, 165, 7, 15, 253, 212, 162, 78, 132, 137, 208, 84, 167,
        237, 204, 68, 100, 246, 8, 213, 242, 106, 93, 122, 4,
    ],
    [
        241, 17, 106, 127, 2, 113, 226, 130, 243, 19, 179, 190, 222, 11, 173, 64, 151, 30, 201,
        233, 70, 153, 172, 103, 75, 198, 105, 70, 71, 32, 42, 0,
    ],
    [
        138, 218, 206, 121, 56, 70, 29, 76, 35, 14, 197, 57, 23, 33, 98, 143, 3, 119, 68, 76, 120,
        61, 68, 215, 222, 184, 249, 90, 26, 191, 48, 0,
    ],
    [
        90, 121, 28, 230, 71, 66, 64, 184, 188, 184, 101, 73, 187, 218, 135, 36, 221, 95, 3, 85,
        245, 26, 60, 191, 206, 163, 178, 174, 233, 46, 166, 7,
    ],
    [
        182, 207, 74, 237, 251, 179, 225, 128, 174, 239, 121, 114, 127, 2, 147, 53, 121, 27, 85,
        197, 97, 15, 172, 220, 138, 132, 242, 120, 78, 74, 217, 1,
    ],
    [
        14, 105, 161, 55, 85, 178, 167, 130, 193, 133, 239, 116, 214, 105, 76, 92, 44, 106, 120,
        143, 222, 128, 20, 3, 115, 238, 111, 224, 214, 53, 61, 3,
    ],
    [
        172, 174, 20, 59, 246, 160, 128, 128, 255, 247, 245, 81, 36, 44, 23, 132, 134, 107, 110,
        118, 61, 95, 37, 188, 236, 4, 148, 140, 180, 32, 106, 3,
    ],
    [
        249, 35, 102, 12, 240, 83, 200, 179, 158, 70, 132, 245, 117, 0, 71, 145, 170, 141, 81, 188,
        64, 188, 73, 223, 108, 50, 85, 104, 150, 0, 121, 1,
    ],
    [
        211, 37, 23, 239, 46, 43, 230, 93, 109, 15, 208, 243, 242, 242, 143, 96, 124, 175, 207, 30,
        229, 129, 217, 242, 108, 200, 66, 75, 84, 159, 9, 4,
    ],
    [
        164, 205, 166, 117, 171, 94, 89, 147, 192, 67, 186, 30, 177, 123, 164, 27, 190, 198, 42,
        47, 77, 36, 149, 136, 85, 72, 245, 178, 199, 141, 236, 4,
    ],
    [
        29, 125, 206, 149, 78, 22, 118, 121, 10, 188, 172, 242, 90, 72, 177, 234, 208, 158, 252,
        40, 139, 43, 109, 58, 51, 176, 78, 208, 218, 96, 118, 0,
    ],
    [
        136, 10, 215, 208, 176, 45, 11, 108, 221, 149, 129, 244, 93, 20, 221, 101, 147, 55, 6, 149,
        72, 38, 7, 214, 91, 130, 218, 231, 78, 140, 74, 3,
    ],
    [
        8, 98, 119, 33, 220, 18, 55, 167, 214, 245, 6, 41, 230, 96, 28, 140, 129, 207, 157, 158,
        137, 64, 18, 233, 223, 94, 172, 42, 183, 231, 253, 6,
    ],
    [
        229, 134, 117, 163, 69, 239, 116, 47, 15, 94, 86, 173, 31, 53, 153, 86, 55, 5, 254, 201,
        183, 11, 29, 122, 100, 39, 254, 253, 4, 176, 183, 2,
    ],
    [
        113, 57, 110, 164, 188, 15, 91, 46, 229, 230, 241, 13, 252, 48, 102, 254, 75, 22, 62, 4,
        179, 99, 228, 38, 190, 156, 27, 211, 42, 52, 108, 2,
    ],
    [
        26, 144, 76, 226, 12, 168, 115, 78, 36, 219, 28, 151, 22, 222, 140, 232, 80, 91, 162, 208,
        37, 58, 224, 106, 14, 235, 95, 80, 78, 160, 6, 4,
    ],
    [
        88, 171, 41, 13, 63, 252, 178, 212, 209, 238, 147, 12, 155, 178, 192, 190, 167, 221, 141,
        181, 211, 207, 169, 146, 134, 103, 249, 242, 158, 56, 145, 0,
    ],
    [
        135, 148, 223, 101, 77, 94, 247, 176, 67, 247, 34, 100, 251, 128, 48, 228, 195, 183, 62,
        39, 47, 245, 2, 86, 190, 240, 208, 187, 160, 141, 197, 1,
    ],
    [
        147, 252, 32, 26, 189, 214, 71, 193, 114, 170, 149, 167, 146, 252, 246, 178, 210, 94, 163,
        21, 188, 12, 16, 125, 69, 229, 175, 22, 244, 73, 254, 7,
    ],
    [
        22, 4, 21, 244, 43, 247, 203, 212, 207, 223, 111, 66, 188, 86, 76, 103, 133, 52, 229, 225,
        98, 240, 167, 10, 182, 224, 98, 248, 172, 199, 5, 0,
    ],
    [
        69, 108, 246, 156, 36, 137, 1, 95, 184, 196, 117, 41, 174, 174, 198, 174, 157, 109, 178,
        51, 198, 156, 245, 204, 135, 154, 246, 207, 48, 58, 69, 0,
    ],
    [
        115, 75, 137, 2, 202, 77, 148, 197, 247, 81, 233, 76, 49, 31, 194, 21, 137, 78, 85, 37, 34,
        32, 129, 96, 253, 87, 100, 200, 172, 165, 196, 4,
    ],
    [
        14, 171, 182, 69, 32, 202, 55, 88, 187, 92, 7, 182, 121, 179, 6, 11, 55, 213, 39, 230, 71,
        228, 66, 33, 162, 11, 79, 128, 159, 249, 226, 2,
    ],
    [
        106, 30, 46, 177, 219, 246, 155, 142, 245, 236, 55, 33, 10, 47, 4, 242, 228, 138, 187, 114,
        128, 52, 38, 130, 125, 54, 238, 83, 222, 89, 159, 6,
    ],
    [
        192, 106, 126, 44, 67, 172, 152, 166, 70, 44, 62, 1, 144, 157, 57, 241, 205, 235, 21, 215,
        211, 245, 140, 240, 134, 45, 186, 125, 44, 82, 35, 7,
    ],
    [
        54, 73, 61, 242, 9, 119, 30, 141, 141, 186, 14, 96, 130, 1, 24, 129, 69, 125, 230, 211,
        221, 87, 67, 52, 46, 121, 185, 222, 230, 142, 163, 4,
    ],
    [
        76, 169, 60, 168, 179, 29, 51, 60, 71, 46, 177, 187, 174, 208, 34, 105, 100, 120, 0, 215,
        43, 9, 172, 36, 114, 52, 176, 182, 253, 106, 112, 5,
    ],
    [
        237, 89, 230, 149, 71, 96, 28, 231, 116, 125, 89, 251, 253, 169, 73, 119, 142, 30, 163, 36,
        163, 146, 246, 60, 115, 60, 128, 219, 11, 11, 187, 1,
    ],
];
