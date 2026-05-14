#!/bin/sh
set -eu

REPO="${HYPERLIQUID_CLI_REPO:-hypurrclaw/hyperliquid-cli}"
VERSION="${HYPERLIQUID_CLI_VERSION:-latest}"
BIN_DIR="${BIN_DIR:-${HOME}/.local/bin}"
BINARY_NAME="hyperliquid"
QUIET=0
JSON=0
CHECK=0

usage() {
  cat <<'EOF'
Usage: install.sh [--quiet|-q] [--json] [--check]

Environment:
  HYPERLIQUID_CLI_REPO       GitHub repo (default: hypurrclaw/hyperliquid-cli)
  HYPERLIQUID_CLI_VERSION    Release tag (default: latest)
  BIN_DIR                    Install directory (default: $HOME/.local/bin)

Supported targets:
  macOS x86_64 / arm64, Linux x86_64 / aarch64, Windows x86_64 via Git Bash/MSYS/Cygwin
EOF
}

log() {
  if [ "$QUIET" -eq 0 ] && [ "$JSON" -eq 0 ]; then
    echo "$@"
  fi
}

json_escape() {
  printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'
}

json_status() {
  status="$1"
  version="$2"
  path="$3"
  printf '{"status":"%s","version":"%s","path":"%s"}\n' \
    "$(json_escape "$status")" "$(json_escape "$version")" "$(json_escape "$path")"
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    -q | --quiet) QUIET=1 ;;
    --json) JSON=1 ;;
    --check) CHECK=1 ;;
    -h | --help)
      usage
      exit 0
      ;;
    *)
      echo "error: unknown argument '$1'" >&2
      usage >&2
      exit 2
      ;;
  esac
  shift
done

need() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "error: required command '$1' not found" >&2
    exit 1
  fi
}

need curl
need uname

if [ "$CHECK" -eq 0 ]; then
  need mktemp

  if command -v sha256sum >/dev/null 2>&1; then
    sha256_check() {
      sha256sum -c "$1" >/dev/null
    }
  elif command -v shasum >/dev/null 2>&1; then
    sha256_check() {
      shasum -a 256 -c "$1" >/dev/null
    }
  else
    echo "error: required command 'sha256sum' or 'shasum' not found" >&2
    exit 1
  fi
fi

os="$(uname -s)"
arch="$(uname -m)"
archive_ext="tar.gz"
install_binary="$BINARY_NAME"
windows=0

case "$os" in
  Darwin) os_target="apple-darwin" ;;
  Linux) os_target="unknown-linux-gnu" ;;
  MINGW* | MSYS* | CYGWIN*)
    os_target="pc-windows-msvc"
    archive_ext="zip"
    install_binary="${BINARY_NAME}.exe"
    windows=1
    ;;
  *)
    echo "error: unsupported OS '$os' (supported: macOS, Linux, Windows via Git Bash/MSYS/Cygwin)" >&2
    exit 1
    ;;
esac

case "$arch" in
  x86_64 | amd64) arch_target="x86_64" ;;
  arm64 | aarch64) arch_target="aarch64" ;;
  *)
    echo "error: unsupported architecture '$arch' (supported: x86_64, aarch64)" >&2
    exit 1
    ;;
esac

if [ "$windows" -eq 1 ] && [ "$arch_target" != "x86_64" ]; then
  echo "error: unsupported Windows architecture '$arch' (supported: x86_64)" >&2
  exit 1
fi

target="${arch_target}-${os_target}"

if [ "$CHECK" -eq 0 ]; then
  if [ "$archive_ext" = "tar.gz" ]; then
    need tar
  elif ! command -v unzip >/dev/null 2>&1 && ! command -v powershell.exe >/dev/null 2>&1; then
    echo "error: required command 'unzip' or 'powershell.exe' not found" >&2
    exit 1
  fi
fi

if [ "$VERSION" = "latest" ]; then
  VERSION="$(
    curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
      | sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' \
      | head -n 1
  )"
fi

if [ -z "$VERSION" ]; then
  echo "error: could not determine latest release for ${REPO}" >&2
  exit 1
fi

installed_path="${BIN_DIR}/${install_binary}"
if [ "$CHECK" -eq 1 ]; then
  if [ ! -x "$installed_path" ]; then
    [ "$JSON" -eq 1 ] && json_status "missing" "$VERSION" "$installed_path"
    [ "$JSON" -eq 0 ] && log "${BINARY_NAME} is not installed (${installed_path})"
    exit 1
  fi
  installed_version="$("$installed_path" --version 2>/dev/null | awk '{print $NF}' || true)"
  latest_without_v="$(printf '%s' "$VERSION" | sed 's/^v//')"
  if [ "$installed_version" = "$latest_without_v" ] || [ "$installed_version" = "$VERSION" ]; then
    [ "$JSON" -eq 1 ] && json_status "up_to_date" "$VERSION" "$installed_path"
    [ "$JSON" -eq 0 ] && log "${BINARY_NAME} is up to date (${VERSION})"
    exit 0
  fi
  [ "$JSON" -eq 1 ] && json_status "outdated" "$VERSION" "$installed_path"
  [ "$JSON" -eq 0 ] && log "${BINARY_NAME} is not up to date (installed: ${installed_version:-unknown}, latest: ${VERSION})"
  exit 1
fi

windows_path() {
  if command -v cygpath >/dev/null 2>&1; then
    cygpath -w "$1"
  else
    printf '%s' "$1"
  fi
}

asset="${BINARY_NAME}-${target}.${archive_ext}"
url="https://github.com/${REPO}/releases/download/${VERSION}/${asset}"
tmpdir="$(mktemp -d)"

cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT INT TERM

log "Downloading ${BINARY_NAME} ${VERSION} for ${target}..."
curl -fsSL "$url" -o "${tmpdir}/${asset}"
curl -fsSL "${url}.sha256" -o "${tmpdir}/${asset}.sha256"

(
  cd "$tmpdir"
  sha256_check "${asset}.sha256"
)

if [ "$archive_ext" = "tar.gz" ]; then
  tar -xzf "${tmpdir}/${asset}" -C "$tmpdir"
elif command -v unzip >/dev/null 2>&1; then
  unzip -q "${tmpdir}/${asset}" -d "$tmpdir"
else
  ARCHIVE_PATH="$(windows_path "${tmpdir}/${asset}")" \
    DEST_PATH="$(windows_path "$tmpdir")" \
    powershell.exe -NoProfile -ExecutionPolicy Bypass -Command \
      "Expand-Archive -LiteralPath \$env:ARCHIVE_PATH -DestinationPath \$env:DEST_PATH -Force"
fi

if [ -f "${tmpdir}/${install_binary}" ]; then
  binary_path="${tmpdir}/${install_binary}"
else
  binary_path="$(find "$tmpdir" -type f \( -name "$install_binary" -o -name "$BINARY_NAME" \) | head -n 1)"
fi

if [ -z "${binary_path:-}" ] || [ ! -f "$binary_path" ]; then
  echo "error: release archive did not contain executable '${install_binary}'" >&2
  exit 1
fi

mkdir -p "$BIN_DIR"
cp "$binary_path" "${BIN_DIR}/${install_binary}"
chmod 755 "${BIN_DIR}/${install_binary}"

if [ "$JSON" -eq 1 ]; then
  json_status "installed" "$VERSION" "${BIN_DIR}/${install_binary}"
else
  log "Installed ${BINARY_NAME} to ${BIN_DIR}/${install_binary}"
  log "Run '${BINARY_NAME} --version' to verify the installation."
fi
