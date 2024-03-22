const { ethers } = require("ethers");

const path = require("path");
const dotenv = require("dotenv");
dotenv.config({ path: path.join(__dirname, "../.env") });

async function transitionBatch(invisibleAddress, programOutput) {
  let privateKey = process.env.ETH_PRIVATE_KEY ?? "";
  let rpcUrl = process.env.SEPOLIA_RPC_URL ?? "";
  const provider = new ethers.JsonRpcProvider(rpcUrl);
  const signer = new ethers.Wallet(privateKey, provider);

  const invisibleL1Abi = require(path.join(
    __dirname,
    "../abis/InvisibleL1.json"
  )).abi;
  const invisibleContract = new ethers.Contract(
    invisibleAddress,
    invisibleL1Abi,
    signer ?? undefined
  );

  let gasFeeData = await signer.provider?.getFeeData();
  let overrides = {
    // todo gasLimit: 3_000_000,
    maxFeePerGas: gasFeeData?.maxFeePerGas,
    maxPriorityFeePerGas: gasFeeData?.maxPriorityFeePerGas,
  };

  let txRes = await invisibleContract
    .updateStateAfterTxBatch(programOutput, overrides)
    .catch((err) => {
      console.log("Error: ", err);
    });
  console.log("tx hash: ", txRes.hash);
  let receipt = await txRes.wait();
  console.log("Successfully updated state after tx batch: ", txRes.hash);

  console.log(
    "events: ",
    receipt.logs.map((log) => log.args)
  );

  return receipt;
}

async function getProgramOutput(txBatchId) {
  // TODO: Fetch the program_output from Starkware's SHARP

  let programOutput = [
    188138731066207823867626532571600903895851223277100219965876949659914577625n,
    2413538521893157956869522357006235860322019039808149590262438952731562665373n,
    597637518624311738370n,
    5846006549323611672814740539809398437326614429696n,
    210258926710712570525957419222609112870661182717954n,
    3592681469n,
    453755560n,
    2413654107n,
    277158171n,
    3592681469n,
    453755560n,
    277158171n,
    8n,
    8n,
    6n,
    8n,
    250n,
    2500n,
    50000n,
    250000n,
    6n,
    6n,
    6n,
    5000000n,
    50000000n,
    350000000n,
    150000n,
    3000000n,
    1500000n,
    15000000n,
    100000000n,
    1000000000n,
    40161n,
    40231n,
    874739451078007766457464989774322083649278607533249481151382481072868806602n,
    3324833730090626974525872402899302150520188025637965566623476530814354734325n,
    1839793652349538280924927302501143912227271479439798783640887258675143576352n,
    296568192680735721663075531306405401515803196637037431012739700151231900092n,
    40231n,
    3033253555390069154270782512895262507623650195758227655347885210059161580550n,
    1080171247920677033652792283203548254040283123926024513604289393883155901768n,
    13666080137911976457790303480501301096170048n,
    3225200283062039681311450510140452982672304159186741365074365564954203911314n,
    3181948508010967063970497791648000n,
    246527065650711893932399548081420727619250335348n,
    224375749224849234217687462644374828045904800448925631482890369690601120122n,
  ];

  return programOutput;
}

module.exports = {
  transitionBatch,
  getProgramOutput,
};
