#!/usr/bin/env bash
# types (Rust → TypeScript) → WASM → UI (TypeScript → JS) → CLI (embeds the UI and the WASM)
set -euo pipefail
cd "$(dirname "$0")"
cargo test -q -p plan-contracts --features ts export_ts > /dev/null
cargo build -q -p plan-web --target wasm32-unknown-unknown --release
wasm-bindgen --target no-modules --out-dir web/pkg target/wasm32-unknown-unknown/release/plan_web.wasm
[ -d web/node_modules ] || (cd web && npm ci --silent)
(cd web && npx --no-install tsc -p .)
cargo build -q -p plan-cli --release
echo "built: $(pwd)/target/release/plan"
