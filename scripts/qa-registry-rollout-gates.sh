#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
POLICY="$ROOT/docs/registry-rollout-policy.md"
TASKFILE="$ROOT/Taskfile.yml"
WORKFLOW="$ROOT/.github/workflows/ci.yml"
QA_MATRIX="$ROOT/scripts/qa-command-matrix.sh"

require_marker() {
  local file="$1"
  local marker="$2"
  if ! grep -Fq "$marker" "$file"; then
    echo "missing rollout gate marker '$marker' in ${file#$ROOT/}" >&2
    exit 1
  fi
}

write_canary_plan() {
  local stamp
  stamp="$(date -u +%Y%m%d-%H%M%S)"
  local dir="$ROOT/.qa/registry-rollout-canary-$stamp"
  mkdir -p "$dir"
  cat >"$dir/PLAN.md" <<'PLAN'
# Registry Rollout Canary Plan

## Scope

- Child issue:
- Command family:
- Rollout stage: testnet mutating canary
- Fallback mode: legacy-dispatch | legacy-child | fail-closed
- Network: testnet
- Signer source:

## Required Evidence

- Per-command transcript JSON with argv, network, signer address, query address, exit code, output, and emitted IDs.
- `cleanup-orders-open.json` from `orders open`.
- `cleanup-positions-list.json` from `positions list`.
- `cleanup-account-portfolio.json` from `account portfolio`.
- Vault/borrow-lend cleanup evidence when applicable.
- Command-specific emitted-ID cleanup evidence.
- `SUMMARY.md` with pass/fail status, rollback decision, and residual live-gated cases.

## Commands

```bash
task qa:registry-rollout
task qa:matrix:strict
```
PLAN
  echo "$dir/PLAN.md"
}

if [[ "${1:-}" == "--write-canary-plan" ]]; then
  write_canary_plan
  exit 0
fi

for marker in \
  "hidden/internal registry" \
  "read-only default" \
  "testnet mutating canary" \
  "mainnet dry-run comparison" \
  "mainnet opt-in" \
  "mainnet default" \
  "legacy removal" \
  "legacy-child" \
  "legacy-dispatch" \
  "fail-closed" \
  ".qa/registry-rollout-canary-" \
  "cleanup-orders-open.json" \
  "cleanup-positions-list.json" \
  "cleanup-account-portfolio.json" \
  "Live Order Create And Cancel (#60)" \
  "Remaining Live Order Paths (#61)" \
  "Funded-live canaries are manual or scheduled QA only"; do
  require_marker "$POLICY" "$marker"
done

for marker in \
  "qa:registry-rollout:" \
  "qa:registry-canary-plan:"; do
  require_marker "$TASKFILE" "$marker"
done

require_marker "$WORKFLOW" "Registry rollout policy gate"
require_marker "$QA_MATRIX" "cleanup orders open after dry-runs"
require_marker "$QA_MATRIX" "cleanup positions list after dry-runs"

echo "registry rollout gates ok"
