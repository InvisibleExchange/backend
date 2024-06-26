//

//

use std::collections::HashMap;

use invisible_backend::{perpetual::VALID_COLLATERAL_TOKENS, utils::storage::MainStorage};

pub fn _calculate_fees() {
    let storage = MainStorage::new();

    let swap_output_json = storage.read_storage(0);

    let mut fee_map: HashMap<u64, u64> = HashMap::new();

    for transaction in swap_output_json {
        let transaction_type = transaction
            .get("transaction_type")
            .unwrap()
            .as_str()
            .unwrap();
        match transaction_type {
            "swap" => {
                let fee_taken_a = transaction
                    .get("swap_data")
                    .unwrap()
                    .get("fee_taken_a")
                    .unwrap()
                    .as_u64()
                    .unwrap();
                let fee_taken_b = transaction
                    .get("swap_data")
                    .unwrap()
                    .get("fee_taken_b")
                    .unwrap()
                    .as_u64()
                    .unwrap();
                let token_received_a = transaction
                    .get("swap_data")
                    .unwrap()
                    .get("order_a")
                    .unwrap()
                    .get("token_received")
                    .unwrap()
                    .as_u64()
                    .unwrap();
                let token_received_b = transaction
                    .get("swap_data")
                    .unwrap()
                    .get("order_b")
                    .unwrap()
                    .get("token_received")
                    .unwrap()
                    .as_u64()
                    .unwrap();

                let current_fee_a = fee_map.get(&token_received_a).unwrap_or(&0);
                let current_fee_b = fee_map.get(&token_received_b).unwrap_or(&0);

                let new_fee_a = current_fee_a + fee_taken_a;
                let new_fee_b = current_fee_b + fee_taken_b;

                fee_map.insert(token_received_a, new_fee_a);
                fee_map.insert(token_received_b, new_fee_b);
            }
            "perpetual_swap" => {
                let fee_taken_a = transaction
                    .get("swap_data")
                    .unwrap()
                    .get("fee_taken_a")
                    .unwrap()
                    .as_u64()
                    .unwrap();
                let fee_taken_b = transaction
                    .get("swap_data")
                    .unwrap()
                    .get("fee_taken_b")
                    .unwrap()
                    .as_u64()
                    .unwrap();

                let current_fee = fee_map.get(&VALID_COLLATERAL_TOKENS[0]).unwrap_or(&0);

                let new_fee = current_fee + fee_taken_a + fee_taken_b;
                fee_map.insert(VALID_COLLATERAL_TOKENS[0], new_fee);
            }
            _ => {}
        }
    }

    println!("fee map: {:?}", fee_map);
}

pub fn parse_program_output() {
    let program_output =
        "1681714975540286446064826179733025259025830596163312715622600677991254276136
    -1676333510257264228596356589662597756430224580288882883505011303184004341616
    597580416694809001986
    340282367000166625977638945029607129088
    4839524406068408503119694702759214384341319683
    12345
    54321
    55555
    66666
    12345
    54321
    66666
    9
    9
    6
    0
    2500
    25000
    50000
    50000
    6
    6
    10
    50000000
    500000000
    350000000
    150000
    3000000
    1500000
    15000000
    100000000000000
    14000000204800000
    9090909
    7878787
    5656565
    874739451078007766457464989774322083649278607533249481151382481072868806602
    -293669058575504239171450380195767955102919189693631133349615525321517286156
    -1778709136316592932772395480593926193395835735891797916332204797460728444129
    296568192680735721663075531306405401515803196637037431012739700151231900092
    9090909
    0
    0
    7878787
    0
    0
    5656565
    0
    0
    20703416456491290441237729280
    -8250617656174077946097583373886727176364889222626284983300793556939284730
    381910624860573789248581695129117664103119192065
    9856732629625703539098952454285200176020062844859158785080014647278814545
    -1327581836433910765169586223721510724037699701605605271025624615856284415262
    1361138075189787778177397299397205303297
    25289090813440523962054569164799521261759807542017161434515644970743
    -1450834709615209187970392690530624669627090029037214238845628393846677446145
    -1114254488256443169417308810670484968171828026294178206552041120961816731747
    -129974090349199529799115556804328610317435628784342141780920656145337849366
    -8250617656174077946097583373886727176364889222626284983300793556939284730";

    let program_output = format_cairo_ouput(program_output);

    let program_output_arr = preprocess_cairo_output(program_output.clone());
    println!("program_output_arr: {:?}", program_output_arr);

    let program_output = parse_cairo_output(program_output);
    println!("program_output: {:?}", program_output.mm_registrations);
}
