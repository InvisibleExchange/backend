use std::str::FromStr;

use invisible_backend::utils::{
    cairo_output::{format_cairo_ouput, parse_cairo_output, preprocess_cairo_output},
    crypto_utils::{pedersen, pedersen_on_vec},
    storage::MainStorage,
};
use num_bigint::BigUint;
use num_traits::{One, Zero};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // let program_output =
    //     "-1167858433667725231675207078476186098721823340966419893778891282428750607058
    //     -1167858433667725231675207078476186098721823340966419893778891282428750607058
    //     597579297039784607745
    //     12554203473696364802333384682822702497637276928239934111746
    //     4839524406068408503119694702759214384341319683
    //     12345
    //     54321
    //     55555
    //     66666
    //     12345
    //     54321
    //     66666
    //     9
    //     9
    //     6
    //     0
    //     2500
    //     25000
    //     50000
    //     50000
    //     6
    //     6
    //     10
    //     50000000
    //     500000000
    //     350000000
    //     150000
    //     3000000
    //     1500000
    //     15000000
    //     100000000000000
    //     14000000204800000
    //     9090909
    //     7878787
    //     5656565
    //     874739451078007766457464989774322083649278607533249481151382481072868806602
    //     -293669058575504239171450380195767955102919189693631133349615525321517286156
    //     -1778709136316592932772395480593926193395835735891797916332204797460728444129
    //     296568192680735721663075531306405401515803196637037431012739700151231900092
    //     9090909
    //     953615528603744311503903171090925833574271533835808503650182590398151916787
    //     -1739042463350556655838981404226757860504257320430823033700198226961564344147
    //     7878787
    //     0
    //     0
    //     5656565
    //     0
    //     0
    //     3093476031982861765946388197939943455579280384
    //     -1451662316760511764787395817251072071457839741586948771782240428521688634416
    //     3093476031982861845174527948922094091536536576
    //     -1326477520210014736373966699848418303472644480621142795224414340228339532037
    //     720256015655390340593015018558428160
    //     649643524963080317271811968397224848924325242593
    //     720256015655413103875201976145122304
    //     649643524963080317271811968397224848924325242593
    //     1";

    // let program_output = format_cairo_ouput(program_output);

    // let program_output = parse_cairo_output(program_output);

    // let program_output = preprocess_cairo_output(program_output);
    // println!("program_output: {:?}", program_output);

    let storage = MainStorage::new();
    let funding_info = storage.read_funding_info()?;

    println!("funding_info: {:?}", funding_info);

    Ok(())
}
