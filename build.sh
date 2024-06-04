#!/bin/bash
set -e
cd "`dirname $0`"

cargo near build --manifest-path crates/contract/Cargo.toml
cargo build --release -p test-token --target wasm32-unknown-unknown
cp target/wasm32-unknown-unknown/release/*.wasm ./res/
cp target/near/contract/contract.wasm ./res/
cp target/near/contract/contract_abi.json ./res/
