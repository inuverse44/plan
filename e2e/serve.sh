#!/usr/bin/env bash
# Starts `plan serve` on a fresh SQLite (e2e/.tmp/plan.db) with the fixture epics. Playwright runs this (playwright.config.ts).
set -euo pipefail
cd "$(dirname "$0")/.."
P=target/release/plan
[ -x "$P" ] || { echo "build first: ./build.sh" >&2; exit 1; }
rm -rf e2e/.tmp && mkdir -p e2e/.tmp
export PLAN_DB="$PWD/e2e/.tmp/plan.db"
# office: read-only cases. drag, conflict, status: cases that write (one epic each, so they do not disturb the others)
for k in office drag conflict status; do "$P" import e2e/fixtures/stories.toml --key "$k" > /dev/null; done
"$P" import examples/sample.toml --key demo > /dev/null
exec "$P" serve --port "${PORT_E2E:-47391}" --no-open
