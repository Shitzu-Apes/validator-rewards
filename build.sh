#!/bin/bash
set -e
cd "`dirname $0`"

(cd crates/contract && cargo near build reproducible-wasm && cargo near abi)
cargo build --release -p test-token --target wasm32-unknown-unknown

cp target/near/contract/contract.wasm ./res/
cp target/near/contract/contract_abi.json ./res/
cp target/wasm32-unknown-unknown/release/test_token.wasm ./res/
