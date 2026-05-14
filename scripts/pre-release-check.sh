#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

fail=0

check_missing() {
  local path="$1"
  if [[ ! -e "$path" ]]; then
    echo "error: required release file missing: $path" >&2
    fail=1
  fi
}

check_file_contains() {
  local path="$1"
  local pattern="$2"
  local message="$3"
  if [[ ! -e "$path" ]] || ! grep -Eq "$pattern" "$path"; then
    echo "error: $message" >&2
    fail=1
  fi
}

required_files=(
  README.md
  LICENSE
  SECURITY.md
  CONTRIBUTING.md
  CHANGELOG.md
  Cargo.lock
  install.sh
  .github/workflows/ci.yml
  .github/workflows/release.yml
)

for path in "${required_files[@]}"; do
  check_missing "$path"
done

release_repo=""
if ! release_repo="$(python3 - <<'PY'
import re
import sys

try:
    import tomllib
except ModuleNotFoundError:
    print("error: Python 3.11+ tomllib is required to parse Cargo.toml", file=sys.stderr)
    sys.exit(1)

try:
    with open("Cargo.toml", "rb") as fh:
        package = tomllib.load(fh)["package"]
except Exception as exc:
    print(f"error: failed to parse Cargo.toml package metadata: {exc}", file=sys.stderr)
    sys.exit(1)

repository = package.get("repository", "")
match = re.fullmatch(r"https://github\.com/([^/]+/[^/#?]+?)/?", repository)
if match:
    print(match.group(1))
PY
)"; then
  release_repo=""
fi

if [[ -z "$release_repo" ]]; then
  echo "error: Cargo.toml package.repository must be a GitHub repository URL" >&2
  fail=1
else
  check_file_contains install.sh "HYPERLIQUID_CLI_REPO:-${release_repo//\//\\/}" "install.sh default repository does not match Cargo.toml package.repository"
  check_file_contains README.md "raw\\.githubusercontent\\.com/${release_repo//\//\\/}/main/install\\.sh" "README install command does not reference the configured release repository"
fi
check_file_contains README.md "HYPERLIQUID_CLI_REPO=OWNER/REPO" "README does not document HYPERLIQUID_CLI_REPO override"
check_file_contains .github/workflows/release.yml 'HYPERLIQUID_DEFAULT_BUILDER_ADDRESS: \$\{\{ vars\.HYPERLIQUID_DEFAULT_BUILDER_ADDRESS \}\}' "release workflow must expose build-time default builder address"
check_file_contains .github/workflows/release.yml 'HYPERLIQUID_DEFAULT_BUILDER_FEE_RATE: \$\{\{ vars\.HYPERLIQUID_DEFAULT_BUILDER_FEE_RATE \}\}' "release workflow must expose build-time default builder fee rate"
check_file_contains .github/workflows/release.yml 'HYPERLIQUID_DEFAULT_REFERRAL_CODE: \$\{\{ vars\.HYPERLIQUID_DEFAULT_REFERRAL_CODE \}\}' "release workflow must expose build-time default referral code"
check_file_contains .github/workflows/release.yml 'HYPERLIQUID_FEEDBACK_URL: \$\{\{ secrets\.HYPERLIQUID_FEEDBACK_URL \}\}' "release workflow must expose build-time feedback endpoint secret"
check_file_contains .github/workflows/release.yml 'cargo build --locked --release --bin hyperliquid' "release workflow must build the hyperliquid binary with Cargo.lock"
check_file_contains .github/workflows/release.yml 'hyperliquid-linux-x86_64\.tar\.gz' "release workflow must package Linux x86_64 with a friendly asset name"
check_file_contains .github/workflows/release.yml 'hyperliquid-linux-aarch64\.tar\.gz' "release workflow must package Linux aarch64 with a friendly asset name"
check_file_contains .github/workflows/release.yml 'hyperliquid-macos-x86_64\.tar\.gz' "release workflow must package macOS x86_64 with a friendly asset name"
check_file_contains .github/workflows/release.yml 'hyperliquid-macos-aarch64\.tar\.gz' "release workflow must package macOS aarch64 with a friendly asset name"
check_file_contains .github/workflows/release.yml 'hyperliquid-windows-x86_64\.zip' "release workflow must package Windows x86_64 with a friendly asset name"
check_file_contains .github/workflows/release.yml '\$asset\.sha256' "release workflow must produce per-archive .sha256 files"
check_file_contains install.sh 'asset="\$\{BINARY_NAME\}-\$\{platform\}-\$\{arch_target\}\.\$\{archive_ext\}"' "install.sh asset name must match release workflow archive naming"
check_file_contains install.sh 'platform="windows"' "install.sh must support the Windows release target"
check_file_contains install.sh 'curl -fsSL "\$\{url\}\.sha256"' "install.sh must download the matching .sha256 file"
check_file_contains install.sh 'sha256_check "\$\{asset\}\.sha256"' "install.sh must verify downloaded release checksums"

python3 - <<'PY' || fail=1
import re
import sys

try:
    import tomllib
except ModuleNotFoundError:
    print("error: Python 3.11+ tomllib is required to parse Cargo.toml", file=sys.stderr)
    sys.exit(1)

try:
    with open("Cargo.toml", "rb") as fh:
        package = tomllib.load(fh)["package"]
except Exception as exc:
    print(f"error: failed to parse Cargo.toml package metadata: {exc}", file=sys.stderr)
    sys.exit(1)

failed = False
strict_expected = {
    "name": "hyperliquid-cli",
    "edition": "2024",
    "license": "MIT",
    "readme": "README.md",
}

for key, value in strict_expected.items():
    if package.get(key) != value:
        print(f"error: Cargo.toml package.{key} must be {value!r}", file=sys.stderr)
        failed = True

version = package.get("version", "")
if not re.fullmatch(r"\d+\.\d+\.\d+([+-][0-9A-Za-z.-]+)?", version):
    print("error: Cargo.toml package.version must be a semver-like version", file=sys.stderr)
    failed = True

repository = package.get("repository", "")
match = re.fullmatch(r"https://github\.com/([^/]+/[^/#?]+?)/?", repository)
if not match:
    print("error: Cargo.toml package.repository must be a GitHub repository URL", file=sys.stderr)
    failed = True
    release_repo = ""
else:
    release_repo = match.group(1)

if release_repo:
    expected_urls = {
        "homepage": f"https://github.com/{release_repo}",
        "documentation": f"https://github.com/{release_repo}#readme",
    }
    for key, value in expected_urls.items():
        if package.get(key) != value:
            print(f"error: Cargo.toml package.{key} must be {value!r}", file=sys.stderr)
            failed = True

if not package.get("description"):
    print("error: Cargo.toml package.description is required", file=sys.stderr)
    failed = True

if not package.get("keywords"):
    print("error: Cargo.toml package.keywords is required", file=sys.stderr)
    failed = True

if not package.get("categories"):
    print("error: Cargo.toml package.categories is required", file=sys.stderr)
    failed = True

rust_version = package.get("rust-version", "")
if not re.fullmatch(r"\d+\.\d+(\.\d+)?", rust_version):
    print("error: Cargo.toml package.rust-version must be a Rust semver version", file=sys.stderr)
    failed = True

sys.exit(1 if failed else 0)
PY

if [[ -n "$(git status --porcelain -- . ':!target' ':!.kanna' ':!.sc' ':!.qa')" ]]; then
  echo "warning: working tree has uncommitted tracked/new release changes" >&2
fi

blocked_paths=(
  .env
  .env.local
  .env.production
  .env.test
  .qa
  .kanna
  .sc
  .config
  accounts.db
  wallet-meta.json
  wallet-address.txt
)

for path in "${blocked_paths[@]}"; do
  if [[ -e "$path" ]]; then
    echo "error: local-only artifact exists under repo root: $path" >&2
    fail=1
  fi
done

while IFS= read -r -d '' path; do
  echo "error: secret-bearing artifact exists under repo root: $path" >&2
  fail=1
done < <(find . \
  -path './.git' -prune -o \
  -path './target' -prune -o \
  -path './docs' -prune -o \
  -type f \( \
    -name '.env' -o \
    -name '.env.*' -o \
    -name 'accounts.db' -o \
    -name 'wallet-meta.json' -o \
    -name 'wallet-address.txt' -o \
    -name '*.sqlite' -o \
    -name '*.sqlite3' -o \
    -name '*.key' -o \
    -name '*.pem' -o \
    -iname '*keystore*' -o \
    -iname '*password*' \
  \) -print0)

if git grep -n -E 'BEGIN (RSA |EC |OPENSSH |)PRIVATE KEY|HYPERLIQUID_PRIVATE_KEY=|OWS_PASSPHRASE=' -- ':!scripts/pre-release-check.sh' >/tmp/hyperliquid-release-grep.$$ 2>/dev/null; then
  cat /tmp/hyperliquid-release-grep.$$ >&2
  rm -f /tmp/hyperliquid-release-grep.$$
  echo "error: tracked files contain high-risk secret markers" >&2
  fail=1
else
  rm -f /tmp/hyperliquid-release-grep.$$
fi

if [[ "$fail" -ne 0 ]]; then
  echo "pre-release check failed" >&2
  exit 1
fi

echo "pre-release check passed"
