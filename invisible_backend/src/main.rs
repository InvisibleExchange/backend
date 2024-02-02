use invisible_backend::{
    transaction_batch::{batch_functions::batch_transition::TREE_DEPTH, TransactionBatch},
    utils::storage::update_invalid::{update_invalid_state, verify_state_storage},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut tx_batch = TransactionBatch::new(TREE_DEPTH);
    tx_batch.init();

    verify_state_storage(&tx_batch.state_tree)?;

    // let indexes = vec![15];
    // update_invalid_state(
    //     &tx_batch.state_tree,
    //     &tx_batch.firebase_session,
    //     &tx_batch.backup_storage,
    //     indexes,
    // );

    Ok(())
}

fn test_program_output() -> &'static str {
    return "-1440434746243531904100007457269884669604392518951606614503318327883213594285
    -549845404218709580121643571776766175083620084819530925335484238982449935465
    597614602336677658626
    22300745198530623141535718272929836482691072
    210258926710712570525957419222609112870661182717955
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
    50000000
    500000000
    350000000
    150000
    3000000
    1500000
    15000000
    100000000
    1000000000
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
    704691608687245587077909074011728735611348324416891667261556284258056215266
    104465481777471529088702081153442803765281940697
    13066842889764036997701939897810346102003200000002";
}

// let program_output = test_program_output2();

// let program_output = format_cairo_ouput(program_output);
// // let program_output = preprocess_cairo_output(program_output);

// // for (i, output) in program_output.iter().enumerate() {
// //     println!("{},", output);
// // }

// let output = parse_cairo_output(program_output);
// println!("output: {:?} \n", output.mm_onchain_actions);
