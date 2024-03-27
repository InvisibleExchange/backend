use invisible_backend::utils::cairo_output::{format_cairo_ouput, preprocess_cairo_output};

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
    return "-1606974064382656037350489012671085622226373600755304918256856196125214535849
    -736514130981080489481273891992838960787092226405319647626608831800806118149
    597646784320468156425
    22300745198530623141535718272929836482691072
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
    0
    0
    359466848329860506511012054865780389755946741116009716601630866960927141857
    0
    3
    -792122359657250765314795275614521621172678771441496294234466087580593335310";
}
