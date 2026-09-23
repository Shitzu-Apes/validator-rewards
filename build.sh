#!/bin/bash
set -e
cd "`dirname $0`"

(cd crates/contract && cargo near build reproducible-wasm && cargo near abi)
# NOTE: test_token.wasm is intentionally NOT rebuilt here. The integration
# tests run on near-sandbox 2.3.0 (protocol 72), which rejects wasm produced by
# current rustc toolchains. res/test_token.wasm is pinned to the last build the
# sandbox accepts. It only needs to be a standard NEP-141 token, so it does not
# track contract changes.

cp target/near/contract/contract.wasm ./res/
cp target/near/contract/contract_abi.json ./res/
