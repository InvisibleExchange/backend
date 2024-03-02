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

    // let indexes = vec![15];
    // update_invalid_state(
    //     &tx_batch.state_tree,
    //     &tx_batch.firebase_session,
    //     &tx_batch.backup_storage,
    //     indexes,
    // );

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
    return "188138731066207823867626532571600903895851223277100219965876949659914577625
    -1204964266772973256827800426088834245301088175523447109710653103404309355108
    597637518624311738370
    5846006549323611672814740539809398437326614429696
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
    -585249233276062059426540270199807597999457019573369044625206846076710439931
    1080171247920677033652792283203548254040283123926024513604289393883155901768
    13666080137911976457790303480501301096170048
    -393302505604091532385872272954617122950803056144855334898726491181668109167
    3181948508010967063970497791648000
    246527065650711893932399548081420727619250335348
    224375749224849234217687462644374828045904800448925631482890369690601120122";
}
