#!/bin/bash
set -e
cd "`dirname $0`"

(cd crates/contract && cargo near build reproducible-wasm && cargo near abi)
# The sandbox near-sandbox 2.3.0 used by the integration tests rejects wasm
# built by newer rustc (>=1.88) due to unsupported wasm features. Build the
# test token with the same 1.86 toolchain the reproducible contract build uses.
cargo +1.86 build --release -p test-token --target wasm32-unknown-unknown

cp target/near/contract/contract.wasm ./res/
cp target/near/contract/contract_abi.json ./res/
cp target/wasm32-unknown-unknown/release/test_token.wasm ./res/
