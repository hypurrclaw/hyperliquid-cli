# Hyperliquid CLI

[![Crates.io](https://img.shields.io/badge/crates.io-v0.11.0-orange.svg)](https://crates.io/crates/hyperliquid-cli)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.93%2B-blue.svg)](https://www.rust-lang.org)
[![Built on hypersdk](https://img.shields.io/badge/built%20on-hypersdk-blueviolet.svg)](https://github.com/infinitefield/hypersdk)

Languages: [English](README.md) | [简体中文](README.zh-CN.md) | [日本語](README.ja-JP.md) | [한국어](README.ko-KR.md)

**AI 에이전트에게 Hyperliquid를 거래할 CLI와 지갑을 제공하세요.**

`hyperliquid`는 개인 에이전트 — OpenClaw, Hermes, PicoClaw, Claude, Codex, 또는 셸 명령을 실행할 수 있는 어떤 LLM이든 — 에 [Hyperliquid](https://app.hyperliquid.xyz)를 위한 프로덕션급 명령줄과 암호화된 지갑을 제공하는 단일 바이너리입니다. 시장, 주문, 이체, 스테이킹, 볼트, 대출/차입, 빌더 수수료, WebSocket 스트림까지 모든 작업은 에이전트가 읽고, 추론하고, 실행할 수 있는 깔끔한 JSON 명령으로 노출됩니다.

에이전트의 도구 벨트에 넣기만 하면 가격 확인, 주문 실행, 포지션 관리, 오더북 스트리밍을 모두 하나의 바이너리로 수행할 수 있습니다. 드라이런, 스키마, 안전 게이트가 기본 제공됩니다.

## hyperliquid-cli를 사용하는 이유

- **에이전트 우선.** 에이전트 루프를 위해 구축되었습니다. 모든 명령은 `--format json`, 필드 프로젝션(`--select`), 결과 제한(`--max-results`), 기계가 읽을 수 있는 `schema` 출력으로 JSON을 말합니다. `HYPERLIQUID_AGENT=1`(또는 non-TTY stdout)은 자동으로 JSON을 기본값으로 사용합니다. 에이전트는 안정적인 snake_case 키, 구조화된 오류 객체, 명확히 정의된 종료 코드를 읽습니다. 스크래핑도, 추측도 필요 없습니다.
- **에이전트를 위한 지갑.** 거래는 가능하지만 출금은 절대 할 수 없는 API wallet(일명 agent wallet)을 생성하세요. 이를 OpenClaw, Hermes, Claude 또는 어떤 자동화에도 전달하면 제한된 권한 안에서 동작합니다. OWS wallet secrets는 암호화된 vault에 저장됩니다. CLI가 생성하는 API wallet private key는 안전하게 보관할 수 있도록 정확히 한 번 출력됩니다.
- **하나의 도구, 광범위한 프로토콜 지원.** 시장, perps, spot, HIP-3 DEXes, 주문, 이체, subaccounts, vaults, staking, borrow/lend, builder fees, referrals, account abstraction, WebSocket subscriptions까지 모두 하나의 바이너리 뒤에 있습니다.
- **기본적으로 안전함.** schema가 prompt-gated로 표시한 live mainnet 변경 작업은 확인이 필요합니다. `--dry-run`은 지원되는 부작용을 실행 전에 미리 보여줍니다. Testnet은 플래그 하나면 됩니다. API wallets는 프로토콜 설계상 출금이 불가능합니다.
- **Decimal 정확성.** 모든 가격, 크기, 금액은 `rust_decimal`을 사용합니다. float도, 예상치 못한 반올림도 없습니다.
- **단일 바이너리.** [`hypersdk`](https://github.com/infinitefield/hypersdk) 위에서 Rust로 구축되었습니다. 몇 초 만에 설치하고, 컨테이너에 담아 배포하고, 어디서든 실행하세요.

## 설치

```bash
curl -fsSLO https://raw.githubusercontent.com/hypurrclaw/hyperliquid-cli/main/install.sh
sh install.sh
hyperliquid --version
```

설치 프로그램은 바이너리를 `~/.local/bin`에 복사하기 전에 SHA-256 체크섬을 검증합니다. 기본적으로 최신 release를 설치합니다. `HYPERLIQUID_CLI_REPO=OWNER/REPO`, `HYPERLIQUID_CLI_VERSION=v0.11.0`, `BIN_DIR=/path/to/bin`으로 repo, 고정 version, install directory를 재정의할 수 있습니다.

소스에서 설치:

```bash
cargo install --path . --bin hyperliquid
```

Rust 1.93+가 필요합니다.

## 빠른 시작

```bash
# Read the market
hyperliquid status
hyperliquid mids
hyperliquid book BTC

# Read it as JSON, projected, capped
hyperliquid --format json --select coin,price --max-results 10 mids

# Configure a signer
hyperliquid setup

# Plan an order without sending it
hyperliquid --dry-run orders create --coin BTC --side buy --price 50000 --size 0.001

# Send it on testnet
hyperliquid --testnet orders create --coin BTC --side buy --price 50000 --size 0.001

# Ask the CLI to describe its own contract
hyperliquid --format json schema orders create
```

## 지갑 설정

`hyperliquid`는 **Open Wallet Standard (OWS)**를 유일한 지갑 백엔드로 사용합니다. 지갑은 디스크의 암호화된 vault(기본값 `~/.hyperliquid`, `HYPERLIQUID_OWS_VAULT_PATH`로 재정의 가능)에 저장됩니다. 숨김 프롬프트에서 대화식으로 입력한 비밀은 에코, 로깅 또는 출력되지 않습니다. 명시적인 `wallet export`와 API wallet 생성 플로우는 secret을 한 번 표시할 수 있는 의도된 예외입니다.

### 안내형 설정

새 운영자를 위한 가장 빠른 경로:

```bash
hyperliquid setup
```

마법사는 지갑 생성 또는 가져오기, 기본 네트워크 선택, 패키지된 builder fee / referral 기본값 저장, 그리고 선택적으로 기본 builder fee cap 승인 과정을 안내합니다. 무인 환경에서는 모든 기본값을 비대화식으로 수락하세요.

```bash
hyperliquid setup -y
```

### 지갑 직접 생성 또는 가져오기

```bash
hyperliquid wallet create               # generate a new wallet
hyperliquid wallet import               # paste a private key (hidden prompt)
hyperliquid wallet import-mnemonic      # paste a BIP-39 mnemonic (hidden prompt)
```

새로 생성되거나 가져온 지갑은 기본 signer가 됩니다.

### 여러 지갑 관리

```bash
hyperliquid wallet list                 # all wallets in the OWS vault
hyperliquid wallet show                 # current default
hyperliquid wallet address              # just the address
hyperliquid wallet rename <SELECTOR> <NEW_NAME>
hyperliquid wallet export <SELECTOR>    # reveal secret (with confirmation)
hyperliquid wallet delete <SELECTOR>
```

기본값을 변경하지 않고 명령별로 특정 지갑을 선택하세요.

```bash
hyperliquid --account alice orders open
hyperliquid --ows-signer 0xabc... positions list
```

### API / agent wallets

API wallets(일명 agent wallets)는 master account가 승인한 위임형 Hyperliquid 거래 키입니다. 거래는 가능하지만 출금은 할 수 없으므로, 자동화 또는 AI 에이전트에 제한된 signer를 넘겨주기에 이상적입니다.

```bash
hyperliquid api-wallet create --name trading-agent
hyperliquid api-wallet list <MASTER_ADDRESS>
hyperliquid api-wallet revoke --name trading-agent
```

### 대체 signer 소스

단일 명령에 OWS vault를 사용하지 않으려면 signer를 명시적으로 전달하세요.

```bash
hyperliquid --keystore ~/.foundry/keystores/my-wallet ...
hyperliquid --private-key 0x... ...     # avoid in shared shells / history
```

또는 환경 변수를 설정하세요.

```bash
export HYPERLIQUID_PRIVATE_KEY=0x...
export OWS_PASSPHRASE=...               # unlock encrypted OWS wallet
```

### 안전 규칙

- private keys, mnemonics, keystore files, OWS secrets, config databases를 **절대** 커밋하지 마세요.
- 공유 환경에서는 원시 `--private-key` 플래그보다 OWS wallets 또는 keystores를 선호하세요.
- 스크립트나 에이전트에 거래를 위임할 때는 API wallets를 사용하세요. 이들은 자금을 출금할 수 없습니다.
- 실제 실행 전에 플로우를 리허설하고 싶을 때는 언제든 `--testnet` 플래그 하나면 됩니다.

## 출력 형식

모든 데이터 명령은 동일한 자동화 표면을 제공합니다.

| Flag | Purpose |
| --- | --- |
| `--format pretty\|table\|json` | 사람 또는 기계용 출력입니다. |
| `--select <FIELDS>` | JSON을 쉼표로 구분된 필드로 프로젝션합니다. |
| `--results-only` | envelope를 제거하고 데이터만 반환합니다. |
| `--max-results <N>` | 클라이언트 측에서 최상위 list/map 크기를 제한합니다. |
| `--dry-run` | 지원되는 변경 작업을 검증하고 미리 봅니다. |
| `--payload-json` / `--payload-file` | dry-runs에 원시 JSON을 입력합니다. |

`HYPERLIQUID_AGENT=1`을 설정하거나(non-TTY로 실행하면) one-shot 명령은 자동으로 JSON을 기본값으로 사용합니다. 오류는 안정적인 객체입니다.

```json
{"error": "Authentication required. Run `hyperliquid setup` to configure your wallet."}
```

모든 변경 명령은 에이전트가 실행 전에 읽을 수 있는 `schema` 설명을 제공합니다. 여기에는 위험 등급, 확인 요구사항, dry-run 지원 여부가 포함됩니다.

에이전트 운영 가이드는 [`SKILL.md`](SKILL.md)를 참조하세요.

## 용어와 주소 selector

| Domain | Examples |
| --- | --- |
| OWS wallet/account record | `account add`, `account ls`, `account set-default` 및 관련 명령으로 관리되는 OWS wallet record입니다. |
| Selected signer | 인증된 작업에 서명하는 데 사용되는 키로, 플래그, 환경/설정, 전역 `--account`, 또는 OWS selector에서 선택됩니다. |
| Protocol user address | fills, portfolio, fees, order status 같은 정보 쿼리에 사용되는 공개 Hyperliquid user address입니다. |
| Master account | API wallets를 승인하고 subaccounts를 소유할 수 있는 프로토콜 owner account입니다. |
| API wallet / agent wallet | master account가 승인한 위임형 Hyperliquid 거래 키입니다. master account를 대신해 거래할 수 있지만 출금은 할 수 없습니다. |
| OWS signer | `--ows-signer`로 선택되는 Open Wallet Standard signer 소스입니다. |
| Subaccount | master account가 제어하는 프로토콜 subaccount입니다. |
| Protocol address | recipient, vault, validator, builder 또는 유사 객체를 위한 리터럴 온체인/프로토콜 주소입니다. |

주소 형태의 명령 입력은 세 가지 안전 등급으로 나뉩니다.

| Class | Accepted values | Used for |
| --- | --- | --- |
| `ACCOUNT_SELECTOR` | 저장된 account alias, 저장된 account id, 또는 `0x` address | `--account`로 signer를 선택하거나 OWS wallet records를 관리합니다. |
| `USER` | `0x` user address, 또는 문서화된 안전한 stored-account selector | `account portfolio`, `orders status --user`, fee queries 같은 공개 조회입니다. |
| `*_ADDRESS` | 명시적인 `0x` protocol address만 | transfer recipients, vaults, validators, builders 및 기타 protocol objects입니다. Local aliases는 이 필드들에 대체되지 않습니다. |

에이전트의 경우, 예시나 설명과 충돌할 때 입력 의미론의 권위 있는 출처는 `hyperliquid --format json schema ...` 도구 스키마입니다.

CLI가 허용하는 표준 top-level aliases:

- `api-wallets` -> `api-wallet`
- `subaccounts` -> `subaccount`
- `transfers` -> `transfer`
- `vaults` -> `vault`

## 명령 참조

### 전역 옵션

| Option | Description |
| --- | --- |
| `-f, --format pretty\|table\|json` | 출력 형식입니다. 유효 기본값은 TTY에서 `pretty`, non-TTY stdout 또는 `HYPERLIQUID_AGENT=1`에서 JSON입니다. 명시적 `--format`은 `HYPERLIQUID_FORMAT`보다 우선합니다. |
| `--private-key <PRIVATE_KEY>` | 원시 private key로 서명합니다. 환경 및 설정을 재정의합니다. |
| `--keystore <PATH>` | Foundry 호환 keystore file로 서명합니다. |
| `--keystore-password <PASSWORD>` | Keystore password입니다. 사람이 사용할 때는 더 안전한 비밀 소스를 선호하세요. |
| `--account <SELECTOR>` | signer로 사용할 저장된 wallet alias, id, 또는 address입니다. 다른 signer flags와 충돌합니다. |
| `--ows-signer <SELECTOR>` | OWS wallet selector(name 또는 id)입니다. identity/dry-run plumbing을 위해 `0x` addresses를 허용합니다. Alias: `--wallet`. local signer flags와 충돌합니다. |
| `--testnet` | API calls와 signed actions를 Hyperliquid testnet으로 라우팅합니다. |
| `--select <FIELDS>` | JSON 출력을 쉼표로 구분된 필드로 프로젝션합니다. |
| `--results-only` | 공통 JSON envelopes를 제거하고 데이터만 반환합니다. |
| `--max-results <N>` | 클라이언트 측에서 최상위 list/map 결과를 제한합니다. |
| `--dry-run` | 변경 명령을 부작용 없이 검증하고 미리 봅니다. |
| `--payload-json <JSON>` / `--payload-file <PATH\|->` | mutating dry-runs를 위한 원시 JSON payload context를 제공합니다. |
| `--no-update-check` | 이 invocation의 release update checks를 비활성화합니다. |
| `-h, --help` / `-V, --version` | help 또는 version 정보를 출력합니다. |

### 시장 데이터

| Command | Description |
| --- | --- |
| `perps list [--dex <DEX>]` | perpetual markets를 나열합니다. |
| `perps get <COIN> [--dex <DEX>]` | 하나의 perpetual market을 표시합니다. |
| `spot list` | spot markets를 나열합니다. |
| `spot get <PAIR>` | 하나의 spot pair를 표시합니다. 예: `PURR/USDC`. |
| `outcomes list [--limit <N>]` | `outcomeMeta`에서 활성 outcome market sides를 나열합니다. |
| `outcomes get #<ENCODING>` / `outcomes get +<ENCODING>` | outcome side metadata와 파생 asset ID를 표시합니다. |
| `book <COIN> [-w] [--max-ticks <TICKS>]` | L2 order book snapshot을 표시하거나 updates를 watch합니다. |
| `mids [-w] [--max-ticks <TICKS>]` | 모든 mid prices를 표시합니다. |
| `candles <COIN> [--interval <INTERVAL>] [--limit <N>] [-w] [--max-ticks <TICKS>]` | candle history를 표시합니다. |
| `spread <COIN>` | bid, ask, spread를 표시합니다. |
| `funding <COIN>` | 현재 및 예상 funding을 표시합니다. |
| `meta` | 원시 exchange metadata를 표시합니다. |
| `status` | API health와 rate-limit context를 표시합니다. |

### account, wallet, setup

| Command | Description |
| --- | --- |
| `setup [-y] [--approve-builder\|--no-approve-builder]` | 안내형 최초 설정 마법사를 실행합니다. |
| `wallet create` | 새 wallet을 생성하고 저장합니다. |
| `wallet import [PRIVATE_KEY]` | wallet을 가져옵니다. 숨김 프롬프트를 사용하려면 key를 생략하세요. |
| `wallet show` | 현재 wallet metadata를 표시합니다. |
| `wallet address` | 설정된 wallet address만 출력합니다. |
| `wallet import-mnemonic [MNEMONIC] [--alias <ALIAS>]` | BIP-39 mnemonic phrase에서 wallet을 가져옵니다. |
| `wallet list` | OWS vault의 모든 wallets를 나열합니다. |
| `wallet rename <SELECTOR> <NEW_NAME>` | wallet 이름을 변경합니다. |
| `wallet delete <SELECTOR>` | wallet을 삭제합니다. `-y`가 없으면 프롬프트가 표시됩니다. |
| `wallet export <SELECTOR> [-y]` | wallet secret(mnemonic 또는 private key)을 내보냅니다. |
| `wallet reset [-y]` | 확인 후 wallet configuration을 제거합니다. |
| `account fees [ADDRESS_OR_WALLET]` | fee schedule과 volume context를 조회합니다. |
| `account fills [ADDRESS_OR_WALLET] [--start <TIME>] [--end <TIME>] [--aggregate-by-time]` | 공개 fill history를 조회하며, 선택적으로 시간별 집계가 가능합니다. |
| `account ledger [ADDRESS_OR_WALLET] --start <TIME> [--end <TIME>]` | deposits, withdrawals, transfers 및 기타 non-funding ledger updates를 조회합니다. |
| `account funding [ADDRESS_OR_WALLET] --start <TIME> [--end <TIME>]` | user funding payment history를 조회합니다. |
| `account orders [ADDRESS_OR_WALLET]` | 공개 open orders를 조회합니다. |
| `account portfolio [ADDRESS_OR_WALLET]` | 공개 portfolio summary를 조회합니다. |
| `account portfolio-history [ADDRESS_OR_WALLET]` | frontend portfolio graph/history data를 조회합니다. |
| `account rate-limit [ADDRESS_OR_WALLET]` | user rate-limit context를 조회합니다. |
| `account subaccounts [ADDRESS_OR_WALLET]` | 공개 subaccounts를 조회합니다. |
| `account twap-history [ADDRESS_OR_WALLET]` | user TWAP order history를 조회합니다. |
| `account twap-fills [ADDRESS_OR_WALLET] [--start <TIME>] [--end <TIME>] [--aggregate-by-time]` | user TWAP slice fills를 조회합니다. |
| `account abstraction [ADDRESS]` | 주소의 account abstraction mode를 읽거나, `ADDRESS`가 생략되면 선택된 account의 모드를 읽습니다. |
| `account abstraction set --mode disabled\|unified-account\|portfolio-margin` | 설정된 signer의 account abstraction을 설정합니다. `-y`가 없으면 프롬프트가 표시됩니다. |
| `subaccount list [ADDRESS_OR_WALLET]` | master address의 공개 subaccounts를 조회합니다. |
| `subaccount create --name <NAME>` | master account가 서명한 subaccount를 생성합니다. |
| `account add [PRIVATE_KEY] [--alias <ALIAS>] [--type <TYPE>] [--default]` / `account ls` / `account set-default [SELECTOR]` / `account remove [SELECTOR] [-y]` | 저장된 wallets를 관리합니다. |
| `api-wallet create [--name <NAME>] [--expires-in <DURATION>] [--agent-address <ADDRESS>] [--generate]` | Hyperliquid API/agent wallet을 생성하고 승인합니다. |
| `api-wallet approve [--name <NAME>] [--expires-in <DURATION>] (--agent-address <ADDRESS>\|--generate)` | 기존 또는 생성된 agent wallet address를 승인합니다. |
| `api-wallet list [ACCOUNT]` | master account가 승인한 API wallets를 나열합니다. |
| `api-wallet revoke --name <NAME>` | 이름이 지정된 API wallet을 수명이 짧은 일회용 agent로 교체합니다. |

API wallets는 승인한 master account를 위해 trading actions에 서명할 수 있지만 출금은 할 수 없습니다. 정보 쿼리에는 master 또는 subaccount address를 사용하세요. `api-wallet create`가 local agent keypair를 생성하면, 해당 주소에 대해 `approveAgent`를 제출하기 전에 private key를 한 번 출력합니다. CLI가 나중에 자동 복구하지 않으므로 이 key를 안전하게 보관하세요.

### 거래와 이체

| Command | Description |
| --- | --- |
| `orders open [-w] [--max-ticks <TICKS>]` | open orders를 나열합니다. |
| `orders history` | order history를 나열합니다. |
| `orders status --user <ADDRESS> (--oid <OID>\|--cloid <CLOID>)` | 공개 order status를 조회합니다. |
| `orders create --coin <COIN> --side buy\|sell [--type limit\|market\|stop-loss\|take-profit\|stop-limit\|take-limit] [--price <PX>] [--trigger-price <PX>] [--size <SIZE>\|--amount <USDC>] [--dex <DEX>] [--reduce-only] [--on-behalf-of <ACCOUNT_SELECTOR>] [--cloid <CLOID>] [-y]` | limit, market, stop-loss, take-profit, stop-limit, take-limit orders를 생성합니다. `--on-behalf-of`는 `vaultAddress`로 사용되는 acting-account selector입니다. |
| `orders scale --coin <COIN> --side buy\|sell --start-price <PX> --end-price <PX> --total-size <SIZE> --orders <N>` | 균등 간격의 limit orders 묶음을 생성합니다. |
| `orders batch-create --orders-file <PATH>` | JSON에서 limit orders 묶음을 생성합니다. |
| `orders create --coin <COIN> --side buy\|sell [--take-profit <PX>] [--stop-loss <PX>] [--grouping normal-tpsl] ...` | fixed-size TP/SL children이 있는 parent order를 생성합니다. |
| `orders tpsl --coin <COIN> (--take-profit <PX>\|--stop-loss <PX>) [--grouping position-tpsl] [--side buy\|sell] [--size <SIZE>] [--margin-mode cross\|isolated]` | 현재 position에 연결된 TP/SL orders를 생성합니다. |
| `orders cancel (ORDER_ID\|--cloid <CLOID>)` | order ID 또는 client order ID로 취소합니다. |
| `orders cancel-all [--coin <COIN>] [--dex <DEX>] [-y]` | 모든 open orders를 취소하며, 선택적으로 coin 또는 DEX로 필터링합니다. |
| `orders modify (ORDER_ID\|--cloid <CLOID>) [--price <PRICE>] [--trigger-price <PRICE>] [--size <SIZE>]` | 기존 order를 수정합니다. |
| `orders twap-create --coin <COIN> --side buy\|sell --size <SIZE> --duration <SECONDS> [--dex <DEX>] [--margin-mode cross\|isolated] [-y]` | TWAP order를 생성합니다. |
| `orders twap-cancel <TWAP_ID> --coin <COIN> [--dex <DEX>]` | TWAP order를 취소합니다. |
| `orders schedule-cancel (--in <DURATION>\|--clear)` | dead man's switch를 설정합니다. |
| `positions list [-w] [--max-ticks <TICKS>]` | open positions를 나열합니다. |
| `positions update-leverage --coin <COIN> --leverage <N> [--isolated]` | leverage를 업데이트합니다. |
| `positions update-margin --coin <COIN> --amount <AMOUNT>` | isolated margin을 추가하거나 제거합니다. |
| `transfer spot-to-perp --amount <USDC> [-y]` | USDC를 spot에서 perp로 이동합니다. |
| `transfer perp-to-spot --amount <USDC> [-y]` | USDC를 perp에서 spot으로 이동합니다. |
| `transfer send --to <ADDRESS> --amount <USDC> [-y]` | USDC를 다른 address로 보냅니다. |
| `transfer spot-send --to <ADDRESS> --token <TOKEN> --amount <AMOUNT> [-y]` | spot token을 다른 address로 보냅니다. |
| `transfer send-asset --to <ADDRESS> --source perp\|spot\|dex:<DEX> --dest perp\|spot\|dex:<DEX> --token <TOKEN> --amount <AMOUNT> [--from-subaccount <ADDRESS>] [-y]` | asset을 accounts, spot, perp, 또는 DEX contexts 사이에서 보냅니다. |
| `transfer withdraw --to <ADDRESS> --amount <USDC> [-y]` | USDC를 Arbitrum으로 출금합니다. |
| `subaccount transfer --subaccount <ACCOUNT_SELECTOR> --amount <USDC> --direction deposit\|withdraw [-y]` | USDC를 subaccount로 또는 subaccount에서 이동합니다. subaccount field는 generic transfer recipient가 아니라 acting-account selector입니다. |
| `subaccount spot-transfer --subaccount <ACCOUNT_SELECTOR> --token <TOKEN> --amount <AMOUNT> --direction deposit\|withdraw [-y]` | spot token을 subaccount로 또는 subaccount에서 이동합니다. subaccount field는 generic transfer recipient가 아니라 acting-account selector입니다. |

`api-wallets`는 `api-wallet`의 alias로 허용됩니다.
`subaccounts`는 `subaccount`의 alias로 허용됩니다.
`transfers`는 `transfer`의 alias로 허용됩니다.

시간 범위가 있는 account history 명령은 RFC3339 timestamps와 epoch milliseconds를 허용합니다. CLI는 milliseconds를 Hyperliquid로 보냅니다. 예:

```bash
hyperliquid --format json account fills 0x0000000000000000000000000000000000000000 --start 2026-05-01T00:00:00Z --end 2026-05-02T00:00:00Z --aggregate-by-time
hyperliquid --format json --select time,delta account ledger 0x0000000000000000000000000000000000000000 --start 1777593600000 --end 1777680000000
hyperliquid --format json account portfolio-history 0x0000000000000000000000000000000000000000
hyperliquid --format json orders status --user 0x0000000000000000000000000000000000000000 --oid 123
hyperliquid --format json --dry-run orders scale --coin BTC --side buy --start-price 80000 --end-price 90000 --total-size 0.005 --orders 5
hyperliquid --format json --dry-run orders batch-create --orders-file tests/fixtures/orders_batch_create.json
hyperliquid --format json --dry-run account abstraction set --mode disabled
hyperliquid --format json outcomes get '#10'
```

Hyperliquid의 문서화된 history endpoints는 제한된 windows를 반환하며, 일반적으로 endpoint에 따라 약 500 또는 2000 rows로 제한됩니다. Export의 경우 인접하지만 겹치지 않는 windows를 쿼리하고 `--format json`을 유지하세요. 전역 `--max-results`는 검사 용도로 local CLI output을 줄일 때만 사용하세요.

Outcome market notation(`#N` spot coin 및 `+N` token name)은 `outcomes list`와 `outcomes get`을 통해 발견할 수 있습니다. `orders create --coin '#N' --dry-run`은 encoded asset id가 포함된 안정적인 outcome order preview를 출력하며, signer가 설정되어 있으면 검증된 outcome notation으로 live limit-order submission을 지원합니다. live outcome order를 넣기 전에 먼저 `--dry-run`을 사용해 encoded asset id와 signed-action preview를 확인하세요.

### 고급 명령

| Command | Description |
| --- | --- |
| `staking summary [ADDRESS]` / `staking validators` / `staking rewards [ADDRESS]` / `staking history [ADDRESS]` | staking state와 history를 읽습니다. |
| `staking delegate --validator <ADDRESS> --amount <AMOUNT>` / `staking undelegate --validator <ADDRESS> --amount <AMOUNT>` / `staking deposit --amount <AMOUNT>` / `staking withdraw --amount <AMOUNT>` / `staking claim-rewards` | staking actions를 제출합니다. |
| `staking link initiate --user <ADDRESS>` / `staking link finalize --user <ADDRESS>` | fee discount attribution을 위해 trading account와 staking account를 연결합니다. Dry-runs에는 permanence/control warnings가 포함되며, live commands에는 confirmation 또는 `--yes`가 필요합니다. |
| `vault list [--kind protocol\|user\|normal\|child\|parent] [--user <ADDRESS>] [--limit <N>] [--sort tvl\|apr\|age\|name]` / `vault search <QUERY> [--user <ADDRESS>] [--limit <N>] [--sort tvl\|apr\|age\|name]` / `vault get <ADDRESS>` / `vault positions <ADDRESS>` | vault state를 발견하고 조회합니다. `--user`는 API가 반환할 때 user deposit context를 포함합니다. |
| `vault deposit --vault <ADDRESS> --amount <AMOUNT>` / `vault withdraw --vault <ADDRESS> --amount <AMOUNT>` | vault transfers를 제출합니다. |
| `borrowlend rates` / `borrowlend get <TOKEN>` / `borrowlend user [ADDRESS]` | borrow/lend markets를 조회합니다. |
| `borrowlend supply <TOKEN> --amount <AMOUNT>` / `borrowlend withdraw <TOKEN> (--amount <AMOUNT>\|--max)` | 검증된 wallet-signed exchange `borrowLend` supply/withdraw actions를 제출합니다. 먼저 `--dry-run`으로 action을 확인하세요. |
| `builder max-fee --user <ADDRESS> --builder <ADDRESS>` | 사용자의 approved max builder fee를 조회합니다. |
| `builder approved --user <ADDRESS>` | fee caps와 함께 사용자가 승인한 모든 builders를 나열합니다. |
| `builder approve --builder <ADDRESS> --max-fee-rate <PERCENT> [-y]` | 설정된 master signer에 대해 builder fee cap을 승인하거나 철회합니다. |
| `prio status` / `prio bid --max <HYPE> --ip <IP> [--slot <N>]` | gossip priority auction을 조회하거나 입찰합니다. |
| `referral register <CODE>` / `referral set [CODE]` / `referral status` | 자신의 referral code를 등록하거나, referrer를 설정하거나, referral state를 검사합니다. |
| `feedback (--scenario-json <JSON>\|--scenario-file <PATH\|->) [--contact <CONTACT>] [--tags <TAG>] [--url <URL>]` | scenario JSON object 형태의 구조화된 CLI feedback을 설정된 feedback endpoint로 보냅니다. rate-limit attribution을 위해 scenario에 `agent_address`, `signer_address`, 또는 `wallet_address`를 포함하고, 기본값을 재정의하려면 `--url`을 사용하세요. |
| `schema [COMMAND...]` | 에이전트를 위한 machine-readable command schemas를 표시합니다. |
| `subscribe trades --asset <ASSET>` / `subscribe orderbook --asset <ASSET>` / `subscribe candles --asset <ASSET> [--interval <INTERVAL>]` / `subscribe all-mids` / `subscribe order-updates` / `subscribe fills` `[--max-events <N>] [--idle-timeout-ms <MS>]` | WebSocket events를 스트리밍합니다. |
| `update` | Linux/macOS에서 최신 GitHub release로 이 바이너리를 업데이트합니다. Windows 사용자는 `install.sh`를 다시 실행해 최신 `.zip` release를 설치하세요. 전역 `--dry-run`으로 preview할 수 있습니다. |

`vaults`는 `vault`의 alias로 허용됩니다.

Builder approvals는 `0.001%` 같은 percent strings를 사용합니다. `0%`는 approved max fee를 zero로 설정해 철회합니다. 승인 action은 API wallet이 아니라 master account가 서명해야 합니다. `orders create`는 짝을 이루는 `--builder <ADDRESS> --builder-fee-rate <PERCENT>` flags를 허용하며 signed order action에 공식 `builder: { b, f }` wire object를 포함합니다. perp builder fees는 `0.1%`, spot builder fees는 `1%`로 제한됩니다. Forks/distributions는 build time에 default order builder parameters를 내장할 수 있습니다.

```bash
HYPERLIQUID_DEFAULT_BUILDER_ADDRESS=0x... \
HYPERLIQUID_DEFAULT_BUILDER_FEE_RATE=0.001% \
cargo build --release --bin hyperliquid
```

같은 이름의 runtime env vars는 build-time defaults를 재정의합니다. `hyperliquid setup` 중에는 build-time defaults가 제안된 builder/fee로 표시되며 Enter를 누르면 이를 수락합니다. runtime env vars가 있으면 그것이 제안값이 됩니다. `HYPERLIQUID_DEFAULT_REFERRAL_CODE`는 setup과 `referral set` 기본값에 대해 동일한 build-time/runtime 패턴을 따릅니다. Release workflow는 같은 이름의 GitHub Actions repository variables에서 이 값들을 전달합니다. env나 config가 defaults를 제공하지 않아도 사용자는 주문별로 `--builder`와 `--builder-fee-rate`를 전달할 수 있습니다.

Vault discovery는 주소 재작성 없이 vault detail과 transfer dry-runs에 입력으로 사용할 수 있습니다.

```bash
hyperliquid --format json vault list --kind protocol --limit 5 --sort tvl
hyperliquid --format json vault get 0x...
hyperliquid --format json --dry-run vault deposit --vault 0x... --amount 5
```

전체 참조: `hyperliquid --help`와 `hyperliquid <command> --help`, 그리고 [`docs/`](docs/)의 가이드.

## Testnet

mainnet을 건드리기 전에 `--testnet`을 사용해 reads, dry-runs, 승인된 live testnet flows를 리허설하세요. Testnet은 testnet API endpoint와 별도의 account state를 사용하면서 동일한 command surface를 제공합니다.

## 안전 모델

CLI는 부작용이 보이도록 설계되었습니다.

- 읽기 전용 명령은 private key를 절대 건드리지 않습니다.
- 서명은 명시적인 `--account`, `--ows-signer`, `--keystore`, `--private-key`, 또는 저장된 OWS wallets를 통해서만 발생합니다.
- `--testnet`은 API calls와 signed actions를 Hyperliquid testnet으로 깔끔하게 라우팅합니다.
- `--dry-run`은 지원되는 mutation을 전송하지 않고 검증하고 미리 봅니다.
- live mainnet mutations와 파괴적인 local secret operations는 schema가 prompt-gated로 표시한 경우 confirmation이 필요합니다. 명령별 confirmation policy는 `hyperliquid --format json schema ...`로 확인하세요. 지원되는 경우 `-y` / `--yes`가 prompts를 건너뜁니다.
- Transfer recipients와 protocol object addresses는 명시적인 `0x` addresses여야 합니다. local aliases가 조용히 대체되는 일은 없습니다.

## 종료 코드

| Code | Meaning |
| --- | --- |
| `0` | 성공 |
| `1` | 내부 오류 |
| `2` | 사용법, 검증 또는 설정 오류 |
| `10` | 인증 누락 또는 유효하지 않음 |
| `11` | rate limited |
| `12` | API 또는 네트워크 사용 불가 |
| `13` | 지원되지 않는 입력, 유효하지 않은 asset, 또는 알 수 없는 DEX |
| `14` | stale data |
| `15` | partial results |

## 설정

해결 순서: CLI flags → environment variables → `~/.config/hyperliquid/config.json`.

| Variable | Purpose |
| --- | --- |
| `HYPERLIQUID_PRIVATE_KEY` | 서명을 위한 private key입니다(OWS 또는 keystore 선호). |
| `HYPERLIQUID_NETWORK` | `mainnet` 또는 `testnet`. |
| `HYPERLIQUID_FORMAT` | agent/non-TTY fallback 이전의 명시적 기본 출력 형식(`pretty`, `table`, 또는 `json`)입니다. |
| `HYPERLIQUID_AGENT` | agent defaults를 강제하려면 `1`로 설정합니다. |
| `HYPERLIQUID_WATCH_MAX_TICKS` | snapshot watch mode의 기본 tick limit입니다. |
| `HYPERLIQUID_SUBSCRIBE_MAX_EVENTS` | agent contexts에서 WebSocket subscribe commands의 기본 event limit입니다. |
| `OWS_PASSPHRASE` | 암호화된 OWS wallet을 잠금 해제하는 passphrase입니다. |
| `HYPERLIQUID_OWS_VAULT_PATH` | OWS vault path를 재정의합니다(기본값 `~/.hyperliquid`). |
| `HYPERLIQUID_API_BASE_URL` / `HYPERLIQUID_MAINNET_API_BASE_URL` / `HYPERLIQUID_TESTNET_API_BASE_URL` | API base URLs를 재정의합니다. 모든 override는 loopback/local test endpoints로 제한됩니다. |
| `HYPERLIQUID_DEFAULT_BUILDER_ADDRESS` / `HYPERLIQUID_DEFAULT_BUILDER_FEE_RATE` | per-order builder fee parameters와 setup suggestions의 runtime defaults입니다. |
| `HYPERLIQUID_DEFAULT_REFERRAL_CODE` | setup 및 `referral set`의 runtime default referral code입니다. |
| `HYPERLIQUID_FEEDBACK_URL` | `hyperliquid feedback`의 runtime 또는 build-time default endpoint입니다. |
| `HYPERLIQUID_NO_UPDATE_CHECK` | truthy일 때 release update checks를 비활성화합니다. |

## 개발

`hyperliquid feedback`의 default endpoint를 내장하려면 build environment에서 `HYPERLIQUID_FEEDBACK_URL`을 설정하세요. Runtime에서는 `--url`이 우선하고, 그다음 runtime `HYPERLIQUID_FEEDBACK_URL`, 그다음 내장된 build-time default가 적용됩니다.

```bash
HYPERLIQUID_FEEDBACK_URL="https://<worker-subdomain>/feedback" cargo build --release
```

```bash
cargo build
cargo test
cargo clippy -- -D warnings
cargo fmt --check
```

Taskfile을 통한 선택적 반복 가능 QA:

```bash
task bind
task qa:matrix
```

관례, 테스트 규칙, 에이전트 우선 출력 계약은 [`AGENTS.md`](AGENTS.md)와 [`CONTRIBUTING.md`](CONTRIBUTING.md)에 있습니다.

## 다른 것이 필요할 때

- 전체 애플리케이션이나 거래 시스템을 구축하시나요? [`hypersdk`](https://github.com/infinitefield/hypersdk)를 직접 사용하세요.
- 장기 실행 전략 실행, 백테스팅, 또는 호스팅 봇이 필요하신가요? 전용 bot framework를 선택하세요.
- 깊은 historical tick data 또는 cross-exchange research를 찾고 있나요? market-data platform을 사용하세요.

인간, 스크립트, 에이전트 모두에게 같은 방식으로 동작하는 Hyperliquid용 단일 운영 인터페이스가 필요할 때 `hyperliquid`를 사용하세요.

## 감사의 글

Hyperliquid HTTP, WebSocket, EIP-712 signing을 위해 Infinite Field의 [`hypersdk`](https://github.com/infinitefield/hypersdk) 위에 구축되었습니다. `hypersdk`는 [Mozilla Public License 2.0](https://www.mozilla.org/en-US/MPL/2.0/)에 따라 라이선스됩니다.

## 라이선스

MIT — [`LICENSE`](LICENSE)를 참조하세요.

### 서드파티 라이선스

`hyperliquid-cli`는 MIT이지만, 자체 라이선스를 가진 open-source crates에 의존합니다.

| Dependency | License | Notes |
| --- | --- | --- |
| [`hypersdk`](https://github.com/infinitefield/hypersdk) | MPL-2.0 | Cargo dependency로 수정 없이 사용됩니다. |
| [`alloy`](https://github.com/alloy-rs/alloy) family | MIT OR Apache-2.0 | EVM primitives와 signers입니다. |
| [`tokio`](https://github.com/tokio-rs/tokio) | MIT | Async runtime입니다. |
| [`clap`](https://github.com/clap-rs/clap) | MIT OR Apache-2.0 | CLI framework입니다. |
| [`reqwest`](https://github.com/seanmonstar/reqwest) | MIT OR Apache-2.0 | HTTP client입니다. |
| [`rust_decimal`](https://github.com/paupino/rust-decimal) | MIT | Fixed-point decimal math입니다. |
| [`ows-lib`](https://crates.io/crates/ows-lib) | See crate metadata | OWS wallet backend입니다. |

`hypersdk`는 수정되지 않은 upstream Cargo dependency로 사용되므로, MPL-2.0의 file-level copyleft는 공개 upstream repository로 충족됩니다. 이 CLI를 fork하고 in-tree에서 `hypersdk` source files를 수정한다면, 해당 파일들은 MPL-2.0을 유지해야 하며 수정된 source를 제공해야 합니다. `hyperliquid-cli`의 나머지 부분은 MIT로 유지됩니다.

전체 transitive license report 생성:

```bash
cargo install cargo-license
cargo license
```

## 면책 조항

이 소프트웨어는 어떠한 종류의 보증도 없이 "있는 그대로" 제공됩니다. 탈중앙화 거래소에서의 거래는 상당한 손실 위험을 수반합니다. 키, 서명된 작업, 거래 결정에 대한 책임은 전적으로 사용자에게 있습니다. 이 프로젝트는 Hyperliquid와 공식적으로 제휴되어 있지 않습니다.
