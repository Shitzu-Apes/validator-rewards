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

## Create DAO proposals to extend rewards

`extend-rewards` is a native Rust CLI that creates the three proposals needed to
extend the validator rewards. Each step is a separate command, and the steps
must be approved and executed in order:

1. `send-tokens` deposits one or more FTs into the Dogshit contract.
2. `mint-shares` wraps all pending deposits and mints DOGSHIT shares.
3. `extend-farm` sends the DOGSHIT shares to the validator pool and updates the
   farm end date.

The CLI is a dry run unless `--send` is passed. It reads `PRIVATE_KEY` from
`.env` for the signer (`marior.near` by default). If a token is not whitelisted
in the Dogshit contract or has no storage registration there, `--send` refuses
to run; create the required `whitelist_add_token` or `storage_deposit` proposal
first.

```sh
# Step 1: one token
cargo run -p extend-rewards -- send-tokens \
    --token token.0xshitzu.near \
    --amount 300000 \
    --send

# Step 1: several tokens in one run (one proposal per token)
cargo run -p extend-rewards -- send-tokens \
    --transfer token.0xshitzu.near=300000 \
    --transfer token.lonkingnearbackto2024.near=1000 \
    --send

# Step 2: after every deposit proposal has executed
cargo run -p extend-rewards -- mint-shares --shares 600000 --send

# Step 3: after the mint has executed
cargo run -p extend-rewards -- extend-farm \
    --amount 600000 \
    --end-date 2027-04-25 \
    --send
```

For a standalone binary:

```sh
cargo build --release -p extend-rewards
./target/release/extend-rewards status
```

Amounts are human-readable and converted with the token's on-chain decimals
(SHITZU 18, LONK 8, DOGSHIT 24). `--end-date` accepts `YYYY-MM-DD`, RFC3339,
seconds, or nanoseconds.

### Remove dust rewards

`remove-rewards` creates one `remove_reward` proposal per selected token. Each
executed proposal sends that token's reward balance back to the DAO.

```sh
# Explicit tokens
cargo run -p extend-rewards -- remove-rewards \
    --token 4illia-222.meme-cooking.near \
    --token hijack-252.meme-cooking.near \
    --send

# Everything worth less than $0.01 at current prices
cargo run -p extend-rewards -- remove-rewards --below 0.01 --send

# Exclude a specific token from the automatic selection
cargo run -p extend-rewards -- remove-rewards \
    --below 1 --exclude 4illia-222.meme-cooking.near --send

# Also include tokens that have no price feed
cargo run -p extend-rewards -- remove-rewards \
    --below 0.01 --include-unpriced --send
```

`--below` selects by total reward value, not unit price. Prices come from Ref
Finance with a DexScreener fallback; override with `--price TOKEN=USD`. The
command prints every reward, its value, and whether it is selected before
creating anything. One proposal is created per token, so the signer pays the
0.1 NEAR proposal bond for each. If the RPC rate-limits the price lookups,
pass `--rpc https://free.rpc.fastnear.com`.

### Preview the APR

`preview` calculates what a deposit and farm duration generate for stakers,
using live token prices and the pool's total staked balance:

```sh
cargo run -p extend-rewards -- preview \
    --transfer token.0xshitzu.near=300000 \
    --shares 600000 \
    --end-date 2027-04-25
```

The output shows:

- the existing DOGSHIT reward basket and the new deposits, in USD
- the projected DOGSHIT supply and intrinsic basket price
- the value of the DOGSHIT shares being farmed
- APR/APY based on the pool's total staked NEAR
- both the **new deposit value APR** and the **farmed intrinsic APR**; the
  latter accounts for dilution and the existing basket
- a **fair mint** value: the DOGSHIT share amount where both APRs match
- an optional after-fee APR with `--fee 0.25`
- **DAO treasury impact**: NEAR, deposited token balances before/after,
  DOGSHIT balance through mint/farm, and the change in the DAO's remaining
  DOGSHIT claim value
- the exact `send-tokens`, `mint-shares`, and `extend-farm` commands to execute
  the preview, including `--send`

The DAO value change is the deposit spend plus the change in the DAO's own
remaining DOGSHIT claim. It is not the same as the staker reward value, because
dilution is shared by all existing DOGSHIT holders.

The two APRs differ because the farm pays out DOGSHIT shares, not the deposited
tokens directly. Minting more shares than the fair mint dilutes existing
holders and makes the farmed APR higher than the deposit-value APR; minting
fewer makes it lower. If the current farm is still active, its remaining
rewards are added to the farmed amount and reduce the fair mint.

If the current farm is still active, the preview also includes its remaining
undistributed rewards, because extending the farm redistributes them over the
new duration.

DOGSHIT has no market pair, so it is valued at its intrinsic basket value by
default. Use `--dogshit-price` to model a market price. Prices come from Ref
Finance with a DexScreener fallback; missing prices count as `$0` unless
`--strict-prices` is passed. Override any token with `--price SHITZU=0.0014` or
`--price token.0xshitzu.near=0.0014`, and override pool inputs with
`--staked-near` / `--near-price`.

If you have already sent the deposit proposals and want to preview the actual
pending deposits instead of `--transfer`, add `--use-onchain-deposits`.

`preview` makes several RPC calls. If the configured RPC rate-limits you, pass
`--rpc https://free.rpc.fastnear.com`.

`cargo run -p extend-rewards -- status` shows pending deposits, the DAO's
DOGSHIT balance, and the current farm. Before sending, the CLI checks the DAO
balance, the Dogshit token whitelist, and storage registration. `mint-shares`
refuses to send when there are no pending deposits, and `extend-farm` refuses
to send when the farm does not exist.

`extend-farm` always updates the existing farm selected by `--farm-id` (default
`0`); it never creates a new farm. It reuses the current farm name unless
`--name` is given.

Addresses, signer, RPC, farm id, and proposal bond can be overridden with flags
or environment variables (`DAO_ID`, `DOGSHIT_ID`, `POOL_ID`, `FARM_ID`,
`DAO_SIGNER`, `NEAR_RPC_URL`).

## Run tests

The tests are run via [near-sandbox](https://github.com/near/near-sandbox), because this is the only way to have a realistic validator setup.
You need to locally compile `near-sandbox` and then copy the binary into the `res` folder or you can use the prebuilt Docker image:

```sh
docker run --rm -it -v $(pwd):/app tarnadas/near-sandbox /bin/bash -c "cargo test test_basic_reward_distribution -- --nocapture"
```

The tests cannot run in parallel, because the sandbox can only be used by one test at the same time.
