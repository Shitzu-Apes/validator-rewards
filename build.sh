#!/bin/bash
set -e
cd "`dirname $0`"

cargo build --release -p contract --target wasm32-unknown-unknown
cargo build --release -p test-token --target wasm32-unknown-unknown
cp target/wasm32-unknown-unknown/release/*.wasm ./res/

cargo near abi --manifest-path ./crates/contract/Cargo.toml
cp target/near/contract/contract_abi.json ./res/

wasm-opt -O4 res/contract.wasm -o res/contract.wasm --strip-debug --vacuum
wasm-opt -O4 res/test_token.wasm -o res/test_token.wasm --strip-debug --vacuum
