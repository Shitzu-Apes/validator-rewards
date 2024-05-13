# Validator Rewards

This contract is a Fungible Token (FT) that can be used for the Shitzu validator as staking rewards.
The way it works is that the contract wraps other FTs.
These wrapped FTs can be redeemed by the user via burning the FT from this contract.

The overall flow via DAO proposals looks like this:

- deposit all tokens that you want to be wrapped into this contract. [Example proposal creation](./crates/contract-test/tests/util/call.rs#L88)
- mint new shares. All deposited tokens will now be wrapped via this contract. The amount of shares is essential for determining APR. If there are existing unburned shares in circulation, then the newly minted shares will change the new APR. [Example proposal creation](./crates/contract-test/tests/util/call.rs#L126)
  - Example: mint 1k shares of 100k wrapped SHITZU & 1m wrapped LONK
  - 1 share is worth 100 SHITZU & 1k LONK
  - 500 shares have been burnt so far -> users redeemed 50k SHITZU & 500k LONK
  - mint 10k new shares of 200k wrapped SHITZU & 2m wrapped LONK
  - there are now 10.5k shares total with 250k wrapped SHITZU & 2.5m wrapped LONK
  - 1 share is now worth ~23.81 SHITZU & ~238.1 LONK
  - the calculation of new APR depends on how many shares have been burned so far and how many new tokens are wrapped per share
- deposit the FTs from this contract into the validator staking pool. The first time this is done a new farm needs to be created. Any refill of validator staking rewards needs to be done via an update farm proposal:
  - [Example proposal to create a new farm](./crates/contract-test/tests/util/call.rs#L161)
  - [Example proposal to update an existing farm](./crates/contract-test/tests/util/call.rs#L207)

## Run tests

The tests are run via [near-sandbox](https://github.com/near/near-sandbox), because this is the only way to have a realistic validator setup.
You need to locally compile `near-sandbox` and then copy the binary into the `res` folder.
The tests cannot run in parallel, because the sandbox can only be used by one test at the same time.
