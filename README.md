# Invisible

<!-- ![zigzag](https://user-images.githubusercontent.com/57314871/154353264-211a4030-8f5d-4aa8-878f-f654fa242589.png) -->



## **Description**:
An app specific rollup exchange natively incorporating private transactions, achieving the high throughput and privacy of centralised exchanges while retaining the permissionless and self-custodial nature of Defi.
You can see the general architecture on [Diagrams.net](https://app.diagrams.net/) using the Invisibl3.drawio file in the recources directory.



---
## **Installation**:
### ***Prerequisites***:
- [Node.js](https://nodejs.org/en/download/)
- [Rust](https://www.rust-lang.org/tools/install)
- [Cairo](https://www.cairo-lang.org/docs/quickstart.html)
- [Foundry](https://book.getfoundry.sh/getting-started/installation)
- [Protobuf](https://grpc.io/docs/protoc-installation/)


---

1. Move into the invisible_react directory and run `npm i`
2. Move into the invisible_solidity directory and run `npm i`
3. From the root directory run `forge build`

---
## **Test:**
- Move into `invisible_react` directory, open a new terminal and run `npm start` 
(*looks slightly better with the dark reader browser extension*)
- Move into `invisible_react/express_server` directory, open a new terminal and run `node client.js`
- Move into `invisible_backend` directory, open a new terminal and run `cargo run --bin server`



There are 2 different ways to test it:

- you either generate random deposits/withdrawals as if they were actually made on chain
- or you run a local devnet and actually make those interactions yourself

1. For the first option you can use the deposits/withdrawals pages by selecting them in the navbar
2. and second is to use the Smart Contracts page and requires a bit of extra work. 
You need to first spin up a local devnet using: `anvil -a 3` and then run `sh deploy_contracts.sh`. Make sure the "Deployer" and "Deployed to" hashes are same as below:
```
[⠢] Compiling...
No files changed, compilation skipped
Deployer: 0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266
Deployed to: 0x5FbDB2315678afecb367f032d93F642f64180aa3
Transaction hash: 0x16673826f13e2f853e55ee830d8377685d204240538ed848a1e7be2920bcbcfc
[⠢] Compiling...
No files changed, compilation skipped
Deployer: 0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266
Deployed to: 0xe7f1725E7734CE288F8367e1Bb143E90bb3F0512
Transaction hash: 0x5edc0a8869f7c2a8891eb664d7d240a1c66c8745a01dfa47c513cf22c90187f5
```

*if you prefer to test with metamask you can add the devnet network to metamask (*you get get the info from anvil*) and import this private key `0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80`, otherweise you can edit the connectWallet function in SmartContract component of the react app*

*(note that the frontend is duct-taped together for testing purposes so you have to go to the home sreen and click refresh after every transaction for it to be reflected in the app)*.

After you are done testing different kinds of transactions deposits/swaps/perp_swaps/withdrawals you can finalize the batch by navigating to the controls page and finalizing the batch. This will generate the merkle proofs and update the state behind the scenes. 
After that you need to navigate to the `cairo_contracts/transaction_batch` directory and activate the virtual environment: `source  ~/cairo_venv/bin/activate` if you installed cairo using venv (otherweise not necessary). From that directory run: `cairo-compile transaction_batch.cairo --output tx_batch_compiled.json --cairo_path ../` and `cairo-run --program tx_batch_compiled.json --layout=all --program_input tx_batch_input.json --print_output`

The Program output should be an list of numbers, which you should copy throw into a text editor and edit into json format (*ugly but it's just for testing purposes so*), so it should look like: `[1234, ..., 73561940810248235971824843692571417829041284]` (*the last number shouldnt have a comma after it*). Then copy that and paste it into "Cairo program output:" text area in the SmartContract page and click the "Update state" button. (This updates the smart contract state so if you used option 1. to test without deploying on devnet then this will fail). And than you can make a withdrawal onchain.




