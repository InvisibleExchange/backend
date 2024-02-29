use std::collections::HashMap;

use invisible_backend::{
    transaction_batch::{batch_functions::batch_transition::TREE_DEPTH, TransactionBatch},
    utils::cairo_output::{format_cairo_ouput, parse_cairo_output, preprocess_cairo_output},
    utils::storage::update_invalid::{update_invalid_state, verify_state_storage},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // let mut tx_batch = TransactionBatch::new(TREE_DEPTH);
    // tx_batch.init();

    // verify_state_storage(&tx_batch.state_tree)?;

    // // let indexes = vec![15];
    // // update_invalid_state(
    // //     &tx_batch.state_tree,
    // //     &tx_batch.firebase_session,
    // //     &tx_batch.backup_storage,
    // //     indexes,
    // // );

    let program_output_ = test_program_output();

    let program_output_ = format_cairo_ouput(program_output_);
    let program_output = preprocess_cairo_output(program_output_);

    for (i, output) in program_output.iter().enumerate() {
        println!("{}n,", output);
    }

    // let output = parse_cairo_output(program_output);
    // println!("output: {:?} \n", output.mm_onchain_actions);

    Ok(())
}

fn test_program_output() -> &'static str {
    return "361774114494094996144832610614300124642270252465375182615864945613907231066
    -468361830747493218341522583102606374737152044722120007322566991385354141299
    597636100146937724932
    4384504911992708754690283869607379755264225837056
    210258926710712570525957419222609112870661182717954
    3592681469
    453755560
    2413654107
    277158171
    3592681469
    453755560
    277158171
    8
    8
    6
    8
    250
    2500
    50000
    250000
    6
    6
    6
    5000000
    50000000
    350000000
    150000
    3000000
    1500000
    15000000
    100000000
    1000000000
    40161
    40231
    874739451078007766457464989774322083649278607533249481151382481072868806602
    -293669058575504239171450380195767955102919189693631133349615525321517286156
    -1778709136316592932772395480593926193395835735891797916332204797460728444129
    296568192680735721663075531306405401515803196637037431012739700151231900092
    40231
    -1741155699452595857822029973049071794979620407304546296836923953457165784509
    785551284925916095344101617032869380398524571387710336238935744872183160729
    13666080137912487980512296010737975578774016
    1669987464367741806901581703315727722326801619559351826421346426798401265671
    3181926758794964349064301776331008
    987253332575707135225395624901186832535835507542
    140649424408447970444639526463658352079186466053097235459433038843084977497";
}
