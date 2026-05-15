#!/usr/bin/env bash
set -u -o pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

timestamp="$(date +%Y%m%d-%H%M%S)"
RUN_DIR="${HL_QA_RUN_DIR:-$ROOT_DIR/.qa/command-matrix-$timestamp}"
mkdir -p "$RUN_DIR"

HL_BIN="${HL_BIN:-hyperliquid}"
HL_QA_NETWORK="${HL_QA_NETWORK:-testnet}"
HL_QA_COIN="${HL_QA_COIN:-BTC}"
HL_QA_SPOT_PAIR="${HL_QA_SPOT_PAIR:-PURR/USDC}"
HL_QA_TOKEN="${HL_QA_TOKEN:-USDC}"
HL_QA_AMOUNT="${HL_QA_AMOUNT:-0.000001}"
HL_QA_PRICE="${HL_QA_PRICE:-50000}"
HL_QA_SIZE="${HL_QA_SIZE:-0.001}"
HL_QA_ORDER_ID="${HL_QA_ORDER_ID:-1}"
HL_QA_TWAP_ID="${HL_QA_TWAP_ID:-1}"
HL_QA_REFERRAL_CODE="${HL_QA_REFERRAL_CODE:-TESTNET}"
HL_QA_IP="${HL_QA_IP:-127.0.0.1}"
HL_QA_STREAM_EVENTS="${HL_QA_STREAM_EVENTS:-1}"
HL_QA_CANDLE_STREAM_EVENTS="${HL_QA_CANDLE_STREAM_EVENTS:-0}"
HL_QA_IDLE_TIMEOUT_MS="${HL_QA_IDLE_TIMEOUT_MS:-6000}"
HL_QA_STRICT_SKIPS="${HL_QA_STRICT_SKIPS:-0}"
HL_QA_CASE_TIMEOUT_SECONDS="${HL_QA_CASE_TIMEOUT_SECONDS:-30}"

if [[ -n "${HL_QA_ACCOUNT_KEY_PASSPHRASE:-}" ]]; then
  export HYPERLIQUID_ACCOUNT_KEY_PASSPHRASE="$HL_QA_ACCOUNT_KEY_PASSPHRASE"
fi

if [[ -z "${HL_QA_KEYSTORE:-}" && -f "$ROOT_DIR/.qa/hyperliquid-testnet-cast/wallet-meta.json" ]]; then
  HL_QA_KEYSTORE="$(python3 - "$ROOT_DIR/.qa/hyperliquid-testnet-cast/wallet-meta.json" <<'PY'
import json, sys
try:
    print(json.load(open(sys.argv[1])).get("keystore", ""))
except Exception:
    print("")
PY
)"
fi

if [[ -z "${HL_QA_KEYSTORE_PASSWORD:-}" && -z "${HL_QA_KEYSTORE_PASSWORD_FILE:-}" && -f "$ROOT_DIR/.qa/hyperliquid-testnet-cast/wallet-meta.json" ]]; then
  HL_QA_KEYSTORE_PASSWORD_FILE="$(python3 - "$ROOT_DIR/.qa/hyperliquid-testnet-cast/wallet-meta.json" <<'PY'
import json, sys
try:
    print(json.load(open(sys.argv[1])).get("password_file", ""))
except Exception:
    print("")
PY
)"
fi

if [[ -z "${HL_QA_ADDRESS:-}" && -f "$ROOT_DIR/.qa/hyperliquid-testnet-cast/wallet-address.txt" ]]; then
  HL_QA_ADDRESS="$(tr -d '[:space:]' < "$ROOT_DIR/.qa/hyperliquid-testnet-cast/wallet-address.txt")"
fi

if [[ -z "${HL_QA_KEYSTORE_PASSWORD:-}" && -n "${HL_QA_KEYSTORE_PASSWORD_FILE:-}" && -f "${HL_QA_KEYSTORE_PASSWORD_FILE:-}" ]]; then
  HL_QA_KEYSTORE_PASSWORD="$(<"$HL_QA_KEYSTORE_PASSWORD_FILE")"
fi

BASE=("$HL_BIN" "--format" "json")
if [[ "$HL_QA_NETWORK" == "testnet" ]]; then
  BASE+=("--testnet")
fi
if [[ -n "${HL_QA_KEYSTORE:-}" ]]; then
  BASE+=("--keystore" "$HL_QA_KEYSTORE")
fi
if [[ -n "${HL_QA_KEYSTORE_PASSWORD:-}" ]]; then
  BASE+=("--keystore-password" "$HL_QA_KEYSTORE_PASSWORD")
fi
if [[ -n "${HL_QA_ACCOUNT:-}" ]]; then
  BASE+=("--account" "$HL_QA_ACCOUNT")
fi

escape_json() {
  python3 -c 'import json,sys; print(json.dumps(sys.stdin.read()))'
}

redact_text() {
  python3 -c 'import re,sys
text = sys.stdin.read()
text = re.sub(r"(?i)(--keystore-password(?:=|\\s+))\\S+", r"\\1[redacted]", text)
text = re.sub(r"(?i)(--private-key(?:=|\\s+))0x[0-9a-f]{64}", r"\\1[redacted]", text)
text = re.sub(r"(?i)(--payload-json(?:=|\\s+))\\S+", r"\\1[redacted-payload]", text)
text = re.sub(r"(?i)0x[0-9a-f]{64}", "[redacted-hex64]", text)
sys.stdout.write(text)'
}

display_command() {
  local out=""
  local redact_next=0
  for arg in "$@"; do
    if [[ "$redact_next" == "1" ]]; then
      out+=" [redacted]"
      redact_next=0
      continue
    fi
    case "$arg" in
      --keystore-password|--private-key|--payload-json)
        out+=" $arg"
        redact_next=1
        ;;
      *)
        out+=" $(printf '%q' "$arg")"
        ;;
    esac
  done
  printf "%s" "${out# }" | redact_text
}

run_process() {
  local stdout_path="$1"
  local stderr_path="$2"
  local stdin_path="$3"
  shift 3
  python3 - "$HL_QA_CASE_TIMEOUT_SECONDS" "$stdout_path" "$stderr_path" "$stdin_path" "$@" <<'PY'
import subprocess
import sys

timeout = float(sys.argv[1])
stdout_path = sys.argv[2]
stderr_path = sys.argv[3]
stdin_path = sys.argv[4]
args = sys.argv[5:]

stdin = None
stdin_handle = None
if stdin_path != "-":
    stdin_handle = open(stdin_path, "rb")
    stdin = stdin_handle

try:
    with open(stdout_path, "wb") as stdout, open(stderr_path, "wb") as stderr:
        proc = subprocess.run(
            args,
            stdin=stdin,
            stdout=stdout,
            stderr=stderr,
            timeout=timeout,
        )
    raise SystemExit(proc.returncode)
except subprocess.TimeoutExpired:
    with open(stderr_path, "ab") as stderr:
        stderr.write(f"\n[qa-command-matrix] timed out after {timeout:g}s\n".encode())
    raise SystemExit(124)
finally:
    if stdin_handle is not None:
        stdin_handle.close()
PY
}

PASS=0
FAIL=0
SKIP=0
TOTAL=0
RESULTS_JSONL="$RUN_DIR/results.jsonl"
: > "$RESULTS_JSONL"

record_result() {
  local name="$1"
  local status="$2"
  local exit_code="$3"
  local expected="$4"
  local command="$5"
  local note="$6"
  printf '{"name":%s,"status":%s,"exit_code":%s,"expected":%s,"command":%s,"note":%s}\n' \
    "$(printf "%s" "$name" | escape_json)" \
    "$(printf "%s" "$status" | escape_json)" \
    "$(printf "%s" "$exit_code" | escape_json)" \
    "$(printf "%s" "$expected" | escape_json)" \
    "$(printf "%s" "$command" | escape_json)" \
    "$(printf "%s" "$note" | escape_json)" >> "$RESULTS_JSONL"
}

run_case() {
  local expected="$1"
  local name="$2"
  shift 2
  TOTAL=$((TOTAL + 1))
  local id
  id="$(printf "%03d-%s" "$TOTAL" "$name" | tr ' /:' '---' | tr -cd '[:alnum:]_.-')"
  local stdout="$RUN_DIR/$id.stdout"
  local stderr="$RUN_DIR/$id.stderr"
  local command_display
  command_display="$(display_command "$@")"

  printf "==> %s\n" "$name"
  run_process "$stdout.raw" "$stderr.raw" "-" "$@"
  local code=$?
  redact_text < "$stdout.raw" > "$stdout"
  redact_text < "$stderr.raw" > "$stderr"
  rm -f "$stdout.raw" "$stderr.raw"

  local ok=0
  if [[ "$expected" == "any" ]]; then
    ok=1
  elif [[ "$expected" == *","* ]]; then
    IFS=',' read -r -a codes <<< "$expected"
    for expected_code in "${codes[@]}"; do
      [[ "$code" == "$expected_code" ]] && ok=1
    done
  elif [[ "$code" == "$expected" ]]; then
    ok=1
  fi

  local note=""
  if [[ "$ok" == "1" ]] && ! validate_case_contract "$name" "$code" "$stdout" "$stderr" "$@"; then
    ok=0
    note="contract assertion failed"
  fi

  if [[ "$ok" == "1" ]]; then
    PASS=$((PASS + 1))
    record_result "$name" "pass" "$code" "$expected" "$command_display" ""
  else
    FAIL=$((FAIL + 1))
    if [[ -z "$note" ]]; then
      note="unexpected exit code"
    fi
    record_result "$name" "fail" "$code" "$expected" "$command_display" "$note"
    printf "    FAIL exit=%s expected=%s\n" "$code" "$expected"
  fi
}

run_case_stdin() {
  local expected="$1"
  local name="$2"
  local stdin_text="$3"
  shift 3
  TOTAL=$((TOTAL + 1))
  local id
  id="$(printf "%03d-%s" "$TOTAL" "$name" | tr ' /:' '---' | tr -cd '[:alnum:]_.-')"
  local stdout="$RUN_DIR/$id.stdout"
  local stderr="$RUN_DIR/$id.stderr"
  local command_display
  command_display="$(display_command "$@")"

  printf "==> %s\n" "$name"
  local stdin_file="$RUN_DIR/$id.stdin"
  printf "%s" "$stdin_text" > "$stdin_file"
  run_process "$stdout.raw" "$stderr.raw" "$stdin_file" "$@"
  local code=$?
  redact_text < "$stdout.raw" > "$stdout"
  redact_text < "$stderr.raw" > "$stderr"
  rm -f "$stdout.raw" "$stderr.raw"

  local ok=0
  local note=""
  if [[ "$expected" == "any" || "$code" == "$expected" ]]; then
    ok=1
  fi
  if [[ "$ok" == "1" ]] && ! validate_case_contract "$name" "$code" "$stdout" "$stderr" "$@"; then
    ok=0
    note="contract assertion failed"
  fi

  if [[ "$ok" == "1" ]]; then
    PASS=$((PASS + 1))
    record_result "$name" "pass" "$code" "$expected" "$command_display" ""
  else
    FAIL=$((FAIL + 1))
    if [[ -z "$note" ]]; then
      note="unexpected exit code"
    fi
    record_result "$name" "fail" "$code" "$expected" "$command_display" "$note"
    printf "    FAIL exit=%s expected=%s\n" "$code" "$expected"
  fi
}

skip_case() {
  local name="$1"
  local note="$2"
  TOTAL=$((TOTAL + 1))
  if [[ "$HL_QA_STRICT_SKIPS" == "1" ]]; then
    FAIL=$((FAIL + 1))
    record_result "$name" "fail" "skip" "0" "" "$note"
    printf "==> %s\n    FAIL skipped in strict mode: %s\n" "$name" "$note"
  else
    SKIP=$((SKIP + 1))
    record_result "$name" "skip" "skip" "skip" "" "$note"
    printf "==> %s\n    SKIP %s\n" "$name" "$note"
  fi
}

first_json_address() {
  python3 <<'PY'
import json, re, sys
try:
    data = json.load(sys.stdin)
except Exception:
    sys.exit(1)

def walk(value):
    if isinstance(value, str):
        if re.fullmatch(r"0x[0-9a-fA-F]{40}", value):
            print(value)
            sys.exit(0)
    elif isinstance(value, dict):
        for item in value.values():
            walk(item)
    elif isinstance(value, list):
        for item in value:
            walk(item)

walk(data)
sys.exit(1)
PY
}

json_field() {
  local field="$1"
  python3 - "$field" <<'PY'
import json, sys
field = sys.argv[1]
try:
    data = json.load(sys.stdin)
except Exception:
    sys.exit(1)
value = data
for part in field.split("."):
    if isinstance(value, dict):
        value = value.get(part)
    else:
        value = None
        break
if value is None:
    sys.exit(1)
print(value)
PY
}

command_requests_json() {
  local expect_format_value=0
  for arg in "$@"; do
    if [[ "$expect_format_value" == "1" ]]; then
      [[ "$arg" == "json" ]] && return 0
      expect_format_value=0
      continue
    fi
    case "$arg" in
      --format|-f)
        expect_format_value=1
        ;;
      --format=json|-f=json|HYPERLIQUID_AGENT=1)
        return 0
        ;;
    esac
  done
  return 1
}

validate_case_contract() {
  local name="$1"
  local code="$2"
  local stdout_path="$3"
  local stderr_path="$4"
  shift 4

  if ! command_requests_json "$@"; then
    return 0
  fi

  python3 - "$name" "$code" "$stdout_path" "$stderr_path" <<'PY'
import json
import sys

name, code, stdout_path, stderr_path = sys.argv[1:5]
stdout = open(stdout_path, encoding="utf-8").read().strip()
stderr = open(stderr_path, encoding="utf-8").read().strip()

if not stdout:
    print(f"{name}: expected JSON stdout for --format json; stderr={stderr!r}", file=sys.stderr)
    sys.exit(1)

try:
    data = json.loads(stdout)
except Exception as err:
    # Streaming commands in --format json emit bounded NDJSON. Accept each
    # non-empty line as a JSON value while retaining single-document checks for
    # ordinary commands.
    try:
        data = [json.loads(line) for line in stdout.splitlines() if line.strip()]
    except Exception:
        print(f"{name}: stdout is not valid JSON/NDJSON: {err}", file=sys.stderr)
        sys.exit(1)

required_fields = {
    "schema orders create": ["command", "json_schema"],
    "wallet address": ["address"],
    "borrowlend supply dry-run": ["dry_run", "command", "would_execute"],
    "orders create dry run": ["dry_run", "command", "would_execute"],
    "orders payload dry run": ["dry_run", "command", "payload"],
    "transfer payload dry run": ["dry_run", "command", "payload"],
    "vault payload dry run": ["dry_run", "command", "payload"],
}

if code == "0":
    if isinstance(data, dict) and "error" in data:
        print(f"{name}: successful JSON command returned error envelope", file=sys.stderr)
        sys.exit(1)
    for field in required_fields.get(name, []):
        if not isinstance(data, dict) or field not in data:
            print(f"{name}: expected top-level JSON field {field!r}", file=sys.stderr)
            sys.exit(1)
else:
    if not isinstance(data, dict) or "error" not in data:
        print(f"{name}: failing JSON command must return an error envelope", file=sys.stderr)
        sys.exit(1)

sys.exit(0)
PY
}

if [[ -z "${HL_QA_ADDRESS:-}" ]]; then
  address_out="$("${BASE[@]}" wallet address 2>/dev/null || true)"
  HL_QA_ADDRESS="$(printf "%s" "$address_out" | json_field "address" 2>/dev/null || true)"
fi

if [[ -z "${HL_QA_ADDRESS:-}" ]]; then
  printf "QA wallet address not found. Set HL_QA_ADDRESS or configure HL_QA_KEYSTORE/HL_QA_KEYSTORE_PASSWORD.\n" >&2
  exit 2
fi

HL_QA_TO="${HL_QA_TO:-$HL_QA_ADDRESS}"
HL_QA_MULTI_SIG_ADDR="${HL_QA_MULTI_SIG_ADDR:-$HL_QA_ADDRESS}"
HL_QA_REQUIRED_SIGNER="${HL_QA_REQUIRED_SIGNER:-$HL_QA_ADDRESS}"
HL_QA_VAULT="${HL_QA_VAULT:-0x0057f763b73fa67b20bd5c4adbd28dade2018ca0}"

if [[ -z "${HL_QA_VALIDATOR:-}" ]]; then
  validators_json="$("${BASE[@]}" staking validators 2>/dev/null || true)"
  HL_QA_VALIDATOR="$(printf "%s" "$validators_json" | first_json_address 2>/dev/null || true)"
fi
HL_QA_VALIDATOR="${HL_QA_VALIDATOR:-0x0000472d488d33b7329ca53bfcc3918961d55f8e}"

printf "# Hyperliquid CLI QA command matrix\n" > "$RUN_DIR/REPORT.md"
printf "\nRun dir: \`%s\`\n\n" "$RUN_DIR" >> "$RUN_DIR/REPORT.md"
printf "Binary: \`%s\`\nNetwork: \`%s\`\nWallet: \`%s\`\n\n" "$HL_BIN" "$HL_QA_NETWORK" "$HL_QA_ADDRESS" >> "$RUN_DIR/REPORT.md"

run_case 0 "top version" "$HL_BIN" --version
run_case 0 "top help" "$HL_BIN" --help
run_case 2 "json usage error" "${BASE[@]}" wallet show --bogus
run_case 0 "agent default json mids" env HYPERLIQUID_AGENT=1 "$HL_BIN" --testnet --select coin,price mids
run_case 0 "explicit pretty override" "$HL_BIN" --format pretty --testnet mids
run_case 13 "json watch requires bound" "$HL_BIN" --testnet mids --watch
run_case 0 "schema all" "${BASE[@]}" schema
run_case 0 "schema orders create" "${BASE[@]}" schema orders create
run_case 0 "max results select" "${BASE[@]}" --max-results 2 --select name,max_leverage perps list

run_case 0 "status" "${BASE[@]}" status
run_case 0 "meta" "${BASE[@]}" meta
run_case 0 "perps list" "${BASE[@]}" perps list
run_case 0 "perps get" "${BASE[@]}" perps get "$HL_QA_COIN"
run_case 0 "spot list" "${BASE[@]}" spot list
run_case 0 "spot get" "${BASE[@]}" spot get "$HL_QA_SPOT_PAIR"
run_case 0 "book snapshot" "${BASE[@]}" book "$HL_QA_COIN"
run_case 0 "mids" "${BASE[@]}" mids
run_case 0 "candles" "${BASE[@]}" candles "$HL_QA_COIN" --limit 2
run_case 0 "spread" "${BASE[@]}" spread "$HL_QA_COIN"
run_case 0 "funding" "${BASE[@]}" funding "$HL_QA_COIN"
run_case 0 "outcomes list" "${BASE[@]}" outcomes list --limit 5
run_case "0,13" "outcomes get" "${BASE[@]}" outcomes get +100

run_case 0 "wallet show" "${BASE[@]}" wallet show
run_case 0 "wallet address" "${BASE[@]}" wallet address
run_case 0 "account fills" "${BASE[@]}" account fills "$HL_QA_ADDRESS"
run_case 0 "account fees" "${BASE[@]}" account fees "$HL_QA_ADDRESS"
run_case 0 "account rate limit" "${BASE[@]}" account rate-limit "$HL_QA_ADDRESS"
run_case 0 "account orders" "${BASE[@]}" account orders "$HL_QA_ADDRESS"
run_case 0 "account portfolio" "${BASE[@]}" account portfolio "$HL_QA_ADDRESS"
run_case 0 "account subaccounts" "${BASE[@]}" account subaccounts "$HL_QA_ADDRESS"
run_case 0 "account portfolio history" "${BASE[@]}" account portfolio-history "$HL_QA_ADDRESS"
run_case 0 "account ledger" "${BASE[@]}" account ledger "$HL_QA_ADDRESS" --start 0
run_case 0 "account funding" "${BASE[@]}" account funding "$HL_QA_ADDRESS" --start 0
run_case 0 "account twap history" "${BASE[@]}" account twap-history "$HL_QA_ADDRESS"
run_case 0 "account twap fills" "${BASE[@]}" account twap-fills "$HL_QA_ADDRESS"
run_case 0 "account abstraction" "${BASE[@]}" account abstraction "$HL_QA_ADDRESS"
if [[ -n "${HYPERLIQUID_ACCOUNT_KEY_PASSPHRASE:-}" ]]; then
  run_case 0 "account ls" "${BASE[@]}" account ls
else
  skip_case "account ls" "local account DB may prompt without HYPERLIQUID_ACCOUNT_KEY_PASSPHRASE; mutating account commands are still dry-run covered"
fi

run_case 0 "api wallet list" "${BASE[@]}" api-wallet list "$HL_QA_ADDRESS"
run_case 0 "subaccount list" "${BASE[@]}" subaccount list "$HL_QA_ADDRESS"
run_case 0 "orders open" "${BASE[@]}" orders open
run_case "0,13" "orders status" "${BASE[@]}" orders status --user "$HL_QA_ADDRESS" --oid "$HL_QA_ORDER_ID"
run_case 0 "orders history" "${BASE[@]}" orders history
run_case 0 "positions list" "${BASE[@]}" positions list

run_case 0 "staking validators" "${BASE[@]}" staking validators
run_case 0 "staking summary" "${BASE[@]}" staking summary "$HL_QA_ADDRESS"
run_case 0 "staking rewards" "${BASE[@]}" staking rewards "$HL_QA_ADDRESS"
run_case 0 "staking history" "${BASE[@]}" staking history "$HL_QA_ADDRESS"
run_case 0 "vault get" "${BASE[@]}" vault get "$HL_QA_VAULT"
run_case 0 "vault positions" "${BASE[@]}" vault positions "$HL_QA_VAULT"
run_case 0 "borrowlend rates" "${BASE[@]}" borrowlend rates
run_case 0 "borrowlend get" "${BASE[@]}" borrowlend get "$HL_QA_TOKEN"
run_case 0 "borrowlend user" "${BASE[@]}" borrowlend user "$HL_QA_ADDRESS"
run_case 0 "borrowlend supply dry-run" "${BASE[@]}" --dry-run borrowlend supply "$HL_QA_TOKEN" --amount 1
run_case 0 "borrowlend withdraw dry-run" "${BASE[@]}" --dry-run borrowlend withdraw "$HL_QA_TOKEN" --amount 1
run_case 0 "prio status" "${BASE[@]}" prio status
run_case 0 "referral status" "${BASE[@]}" referral status

run_case 0 "mids watch one tick" "${BASE[@]}" mids --watch --max-ticks 1
run_case 0 "book watch one tick" "${BASE[@]}" book "$HL_QA_COIN" --watch --max-ticks 1
run_case 0 "candles watch one tick" "${BASE[@]}" candles "$HL_QA_COIN" --watch --max-ticks 1
run_case 0 "orders open watch one tick" "${BASE[@]}" orders open --watch --max-ticks 1
run_case 0 "positions watch one tick" "${BASE[@]}" positions list --watch --max-ticks 1

run_case 0 "subscribe trades" "${BASE[@]}" subscribe trades --asset "$HL_QA_COIN" --max-events "$HL_QA_STREAM_EVENTS" --idle-timeout-ms "$HL_QA_IDLE_TIMEOUT_MS"
run_case 0 "subscribe orderbook" "${BASE[@]}" subscribe orderbook --asset "$HL_QA_COIN" --max-events "$HL_QA_STREAM_EVENTS" --idle-timeout-ms "$HL_QA_IDLE_TIMEOUT_MS"
run_case 0 "subscribe candles" "${BASE[@]}" subscribe candles --asset "$HL_QA_COIN" --max-events "$HL_QA_CANDLE_STREAM_EVENTS" --idle-timeout-ms "$HL_QA_IDLE_TIMEOUT_MS"
run_case 0 "subscribe all mids" "${BASE[@]}" subscribe all-mids --max-events "$HL_QA_STREAM_EVENTS" --idle-timeout-ms "$HL_QA_IDLE_TIMEOUT_MS"
run_case 0 "subscribe order updates bounded" "${BASE[@]}" subscribe order-updates --max-events 0 --idle-timeout-ms "$HL_QA_IDLE_TIMEOUT_MS"
run_case 0 "subscribe fills bounded" "${BASE[@]}" subscribe fills --max-events 0 --idle-timeout-ms "$HL_QA_IDLE_TIMEOUT_MS"

run_case 0 "wallet create dry run" "${BASE[@]}" --dry-run wallet create
run_case 0 "wallet import dry run" "${BASE[@]}" --dry-run wallet import
run_case 0 "wallet reset dry run" "${BASE[@]}" --dry-run wallet reset -y
run_case 0 "account add dry run" "${BASE[@]}" --dry-run account add --alias qa-dry-run --type api-wallet --default
run_case 0 "account set default dry run" "${BASE[@]}" --dry-run account set-default qa-dry-run
run_case 0 "account remove dry run" "${BASE[@]}" --dry-run account remove qa-dry-run -y
run_case 0 "subaccount create dry run" "${BASE[@]}" --dry-run subaccount create --name qa-subaccount
run_case 0 "subaccount transfer dry run" "${BASE[@]}" --dry-run subaccount transfer --subaccount "$HL_QA_TO" --amount "$HL_QA_AMOUNT" --direction deposit
run_case 0 "subaccount spot transfer dry run" "${BASE[@]}" --dry-run subaccount spot-transfer --subaccount "$HL_QA_TO" --token "$HL_QA_TOKEN" --amount "$HL_QA_AMOUNT" --direction deposit
run_case 0 "account abstraction set dry run" "${BASE[@]}" --dry-run account abstraction set --mode disabled
run_case 0 "builder max fee" "${BASE[@]}" builder max-fee --user "$HL_QA_TO" --builder "$HL_QA_TO"
run_case 0 "builder approved" "${BASE[@]}" builder approved --user "$HL_QA_TO"
run_case 0 "builder approve dry run" "${BASE[@]}" --dry-run builder approve --builder "$HL_QA_TO" --max-fee-rate 0.001%
run_case 0 "orders create dry run" "${BASE[@]}" --dry-run orders create --coin "$HL_QA_COIN" --side buy --price "$HL_QA_PRICE" --size "$HL_QA_SIZE" --tif alo
run_case 0 "orders create cloid dry run" "${BASE[@]}" --dry-run orders create --coin "$HL_QA_COIN" --side buy --price "$HL_QA_PRICE" --size "$HL_QA_SIZE" --tif alo --cloid 0x1234567890abcdef1234567890abcdef
run_case 0 "orders create on behalf dry run" "${BASE[@]}" --dry-run orders create --on-behalf-of "$HL_QA_TO" --coin "$HL_QA_COIN" --side buy --price "$HL_QA_PRICE" --size "$HL_QA_SIZE" --tif alo
run_case 0 "orders create builder fee dry run" "${BASE[@]}" --dry-run orders create --coin "$HL_QA_COIN" --side buy --price "$HL_QA_PRICE" --size "$HL_QA_SIZE" --tif alo --builder "$HL_QA_TO" --builder-fee-rate 0.001%
run_case 0 "orders create outcome dry run" "${BASE[@]}" --dry-run orders create --coin '#10' --side buy --price 0.5 --size 1 --tif alo
run_case 0 "orders scale dry run" "${BASE[@]}" --dry-run orders scale --coin "$HL_QA_COIN" --side buy --start-price "$HL_QA_PRICE" --end-price "$(python3 - "$HL_QA_PRICE" <<'PY'
from decimal import Decimal
import sys
print(Decimal(sys.argv[1]) + Decimal('100'))
PY
)" --total-size "$HL_QA_SIZE" --orders 2 --tif alo
run_case 0 "orders batch create dry run" "${BASE[@]}" --dry-run orders batch-create --orders-file tests/fixtures/orders_batch_create.json
run_case 0 "orders tpsl dry run" "${BASE[@]}" --dry-run orders tpsl --coin "$HL_QA_COIN" --side sell --size "$HL_QA_SIZE" --take-profit 120000 --stop-loss 40000
run_case 0 "orders cancel oid dry run" "${BASE[@]}" --dry-run orders cancel "$HL_QA_ORDER_ID"
run_case 0 "orders cancel cloid dry run" "${BASE[@]}" --dry-run orders cancel --cloid 0x1234567890abcdef1234567890abcdef
run_case 0 "orders cancel all dry run" "${BASE[@]}" --dry-run orders cancel-all --coin "$HL_QA_COIN" -y
run_case 0 "orders modify dry run" "${BASE[@]}" --dry-run orders modify "$HL_QA_ORDER_ID" --price "$HL_QA_PRICE"
run_case 0 "orders modify cloid dry run" "${BASE[@]}" --dry-run orders modify --cloid 0x1234567890abcdef1234567890abcdef --price "$HL_QA_PRICE"
run_case 0 "orders twap create dry run" "${BASE[@]}" --dry-run orders twap-create --coin "$HL_QA_COIN" --side buy --size "$HL_QA_SIZE" --duration 300
run_case 0 "orders twap cancel dry run" "${BASE[@]}" --dry-run orders twap-cancel "$HL_QA_TWAP_ID" --coin "$HL_QA_COIN"
run_case 0 "orders schedule cancel dry run" "${BASE[@]}" --dry-run orders schedule-cancel --in 30s
run_case 0 "orders payload dry run" "${BASE[@]}" --dry-run --payload-json '{"coin":"BTC","side":"buy","price":"50000","size":"0.001","type":"limit","tif":"alo"}' orders create --coin "$HL_QA_COIN" --side buy --price "$HL_QA_PRICE" --size "$HL_QA_SIZE"

run_case 0 "positions update leverage dry run" "${BASE[@]}" --dry-run positions update-leverage --coin "$HL_QA_COIN" --leverage 3
run_case 0 "positions update margin dry run" "${BASE[@]}" --dry-run positions update-margin --coin "$HL_QA_COIN" --amount "$HL_QA_AMOUNT"
run_case 0 "transfer spot to perp dry run" "${BASE[@]}" --dry-run transfer spot-to-perp --amount "$HL_QA_AMOUNT"
run_case 0 "transfer perp to spot dry run" "${BASE[@]}" --dry-run transfer perp-to-spot --amount "$HL_QA_AMOUNT"
run_case 0 "transfer send dry run" "${BASE[@]}" --dry-run transfer send --to "$HL_QA_TO" --amount "$HL_QA_AMOUNT"
run_case 0 "transfer spot send dry run" "${BASE[@]}" --dry-run transfer spot-send --to "$HL_QA_TO" --token "$HL_QA_TOKEN" --amount "$HL_QA_AMOUNT"
run_case 0 "transfer send asset dry run" "${BASE[@]}" --dry-run transfer send-asset --to "$HL_QA_TO" --source spot --dest perp --token "$HL_QA_TOKEN" --amount "$HL_QA_AMOUNT"
run_case 0 "transfer withdraw dry run" "${BASE[@]}" --dry-run transfer withdraw --to "$HL_QA_TO" --amount "$HL_QA_AMOUNT"
run_case 0 "transfer payload dry run" "${BASE[@]}" --dry-run --payload-json "{\"to\":\"$HL_QA_TO\",\"amount\":\"$HL_QA_AMOUNT\"}" transfer send --to "$HL_QA_TO" --amount "$HL_QA_AMOUNT"

run_case 0 "staking deposit dry run" "${BASE[@]}" --dry-run staking deposit --amount "$HL_QA_AMOUNT"
run_case 0 "staking withdraw dry run" "${BASE[@]}" --dry-run staking withdraw --amount "$HL_QA_AMOUNT"
run_case 0 "staking delegate dry run" "${BASE[@]}" --dry-run staking delegate --validator "$HL_QA_VALIDATOR" --amount "$HL_QA_AMOUNT"
run_case 0 "staking undelegate dry run" "${BASE[@]}" --dry-run staking undelegate --validator "$HL_QA_VALIDATOR" --amount "$HL_QA_AMOUNT"
run_case 0 "staking claim rewards dry run" "${BASE[@]}" --dry-run staking claim-rewards
run_case 0 "staking link initiate dry run" "${BASE[@]}" --dry-run staking link initiate --user "$HL_QA_TO"
run_case 0 "staking link finalize dry run" "${BASE[@]}" --dry-run staking link finalize --user "$HL_QA_TO"
run_case 0 "vault list" "${BASE[@]}" vault list --limit 5 --sort tvl
run_case 0 "vault list protocol" "${BASE[@]}" vault list --kind protocol --limit 5 --sort tvl
run_case 0 "vault list user context" "${BASE[@]}" vault list --kind user --user "$HL_QA_TO" --limit 5 --sort apr
run_case 0 "vault search" "${BASE[@]}" vault search "$HL_QA_VAULT" --limit 5
run_case 0 "vault search user context" "${BASE[@]}" vault search "$HL_QA_VAULT" --user "$HL_QA_TO" --limit 5
run_case 0 "vault deposit dry run" "${BASE[@]}" --dry-run vault deposit --vault "$HL_QA_VAULT" --amount "$HL_QA_AMOUNT"
run_case 0 "vault withdraw dry run" "${BASE[@]}" --dry-run vault withdraw --vault "$HL_QA_VAULT" --amount "$HL_QA_AMOUNT"
run_case 0 "vault payload dry run" "${BASE[@]}" --dry-run --payload-json "{\"vault\":\"$HL_QA_VAULT\",\"amount\":\"$HL_QA_AMOUNT\"}" vault deposit --vault "$HL_QA_VAULT" --amount "$HL_QA_AMOUNT"
run_case 0 "prio bid dry run" "${BASE[@]}" --dry-run prio bid --max "$HL_QA_AMOUNT" --ip "$HL_QA_IP"
run_case 0 "referral set dry run" "${BASE[@]}" --dry-run referral set "$HL_QA_REFERRAL_CODE"
run_case 0 "referral register dry run" "${BASE[@]}" --dry-run referral register "QA_TESTNET_MATRIX"

run_case 0 "api wallet approve dry run" "${BASE[@]}" --dry-run api-wallet approve --agent-address "$HL_QA_TO" --name qa-matrix
run_case 0 "api wallet revoke dry run" "${BASE[@]}" --dry-run api-wallet revoke --name qa-matrix
run_case 0 "cleanup orders open after dry-runs" "${BASE[@]}" orders open
run_case 0 "cleanup positions list after dry-runs" "${BASE[@]}" positions list

skip_case "setup" "interactive first-run setup is covered by dedicated isolated-state QA, not this live wallet sweep"

python3 - "$RESULTS_JSONL" "$RUN_DIR/results.json" <<'PY'
import json, sys
rows = [json.loads(line) for line in open(sys.argv[1]) if line.strip()]
json.dump(rows, open(sys.argv[2], "w"), indent=2)
PY

{
  printf "| Status | Count |\n"
  printf "| --- | ---: |\n"
  printf "| Pass | %s |\n" "$PASS"
  printf "| Fail | %s |\n" "$FAIL"
  printf "| Skip | %s |\n" "$SKIP"
  printf "| Total | %s |\n\n" "$TOTAL"
  printf "Machine-readable results: \`%s\`\n" "$RUN_DIR/results.json"
} >> "$RUN_DIR/REPORT.md"

printf "\nQA matrix complete: pass=%s fail=%s skip=%s total=%s\n" "$PASS" "$FAIL" "$SKIP" "$TOTAL"
printf "Report: %s\n" "$RUN_DIR/REPORT.md"

if [[ "$FAIL" -gt 0 ]]; then
  exit 1
fi
