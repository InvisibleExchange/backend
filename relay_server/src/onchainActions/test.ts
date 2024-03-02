import ethers from "ethers";

let provider = new ethers.JsonRpcProvider("", "sepolia");

// const { Options } = require("@layerzerolabs/lz-v2-utilities");
import { Options } from "@layerzerolabs/lz-v2-utilities";

const executorGas = 500000;
const executorValue = 0;

const options = Options.newOptions().addExecutorLzReceiveOption(
  executorGas,
  executorValue
);


options.toBytes();
