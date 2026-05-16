# Hyperliquid CLI

[![Crates.io](https://img.shields.io/badge/crates.io-v0.11.0-orange.svg)](https://crates.io/crates/hyperliquid-cli)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.93%2B-blue.svg)](https://www.rust-lang.org)
[![Built on hypersdk](https://img.shields.io/badge/built%20on-hypersdk-blueviolet.svg)](https://github.com/infinitefield/hypersdk)

Languages: [English](README.md) | [简体中文](README.zh-CN.md) | [日本語](README.ja-JP.md) | [한국어](README.ko-KR.md)

**AI エージェントに Hyperliquid を取引するための CLI とウォレットを提供します。**

`hyperliquid` は、OpenClaw、Hermes、PicoClaw、Claude、Codex、またはシェル実行できる任意の LLM といったあなたのパーソナルエージェントに、[Hyperliquid](https://app.hyperliquid.xyz) 向けの本番グレードのコマンドラインと暗号化ウォレットを渡す単一バイナリです。マーケット、注文、送金、ステーキング、Vault、借入/貸付、ビルダー手数料、WebSocket ストリームまで、あらゆる操作が、エージェントが読み取り、推論し、実行できるクリーンな JSON コマンドとして公開されます。

エージェントのツールベルトに入れるだけで、価格確認、注文発注、ポジション管理、板情報のストリーミングを、すべて 1 つのバイナリ経由で行えます。dry-run、スキーマ、安全ゲートも組み込み済みです。

## hyperliquid-cli を選ぶ理由

- **エージェント優先。** エージェントループのために構築されています。すべてのコマンドは `--format json`、フィールド投影 (`--select`)、結果上限 (`--max-results`)、機械可読な `schema` 出力で JSON を扱います。`HYPERLIQUID_AGENT=1`（または非 TTY stdout）では自動的に JSON がデフォルトになります。エージェントは安定した snake_case キー、構造化エラーオブジェクト、明確な終了コードを読み取れます。スクレイピングも推測も不要です。
- **エージェント用ウォレット。** 取引はできても出金はできない API wallet（別名 agent wallet）を作成できます。OpenClaw、Hermes、Claude、または任意の自動化に渡せば、限定された権限で動作します。OWS wallet secrets は暗号化 vault に保存されます。CLI が生成する API wallet private key は安全に保管できるよう一度だけ表示されます。
- **1 つのツールで広範なプロトコルをカバー。** マーケット、perps、spot、HIP-3 DEX、注文、送金、サブアカウント、Vault、ステーキング、借入/貸付、ビルダー手数料、リファラル、account abstraction、WebSocket サブスクリプションまで、すべて 1 つのバイナリの背後にあります。
- **デフォルトで安全。** schema で prompt-gated と示された live mainnet の変更操作は確認を要求します。`--dry-run` は対応する副作用を実行前にプレビューします。testnet は 1 つのフラグで利用できます。API wallet はプロトコル設計上、出金できません。
- **Decimal 正確性。** すべての価格、サイズ、金額は `rust_decimal` を使用します。float も予期しない丸めもありません。
- **単一のバイナリ。** [`hypersdk`](https://github.com/infinitefield/hypersdk) の上に Rust で構築されています。数秒でインストールでき、コンテナに同梱でき、どこでも実行できます。

## インストール

```bash
curl -fsSLO https://raw.githubusercontent.com/hypurrclaw/hyperliquid-cli/main/install.sh
sh install.sh
hyperliquid --version
```

インストーラはバイナリを `~/.local/bin` にコピーする前に SHA-256 チェックサムを検証します。デフォルトでは最新 release をインストールします。repo、固定 version、install directory は `HYPERLIQUID_CLI_REPO=OWNER/REPO`、`HYPERLIQUID_CLI_VERSION=v0.11.0`、`BIN_DIR=/path/to/bin` で上書きできます。

ソースから:

```bash
cargo install --path . --bin hyperliquid
```

Rust 1.93+ が必要です。

## クイックスタート

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

## ウォレット設定

`hyperliquid` は唯一のウォレットバックエンドとして **Open Wallet Standard (OWS)** を使用します。ウォレットはディスク上の暗号化 vault（デフォルトは `~/.hyperliquid`、`HYPERLIQUID_OWS_VAULT_PATH` で上書き可能）に保存されます。非表示プロンプトで対話的に入力されたシークレットはエコー、ログ出力、表示されません。明示的な `wallet export` と API wallet 生成フローは、secret を一度表示し得る意図的な例外です。

### ガイド付きセットアップ

新しいオペレーターにとって最速の手順:

```bash
hyperliquid setup
```

ウィザードは、ウォレットの作成またはインポート、デフォルトネットワークの選択、パッケージ化されたビルダー手数料 / リファラルのデフォルト保存、必要に応じたデフォルトのビルダー手数料上限の承認まで案内します。無人環境では、すべてのデフォルトを非対話的に受け入れます:

```bash
hyperliquid setup -y
```

### ウォレットを直接作成またはインポート

```bash
hyperliquid wallet create               # generate a new wallet
hyperliquid wallet import               # paste a private key (hidden prompt)
hyperliquid wallet import-mnemonic      # paste a BIP-39 mnemonic (hidden prompt)
```

新しく作成またはインポートされたウォレットがデフォルト signer になります。

### 複数ウォレットの管理

```bash
hyperliquid wallet list                 # all wallets in the OWS vault
hyperliquid wallet show                 # current default
hyperliquid wallet address              # just the address
hyperliquid wallet rename <SELECTOR> <NEW_NAME>
hyperliquid wallet export <SELECTOR>    # reveal secret (with confirmation)
hyperliquid wallet delete <SELECTOR>
```

デフォルトを変更せずに、コマンドごとに特定のウォレットを選択します:

```bash
hyperliquid --account alice orders open
hyperliquid --ows-signer 0xabc... positions list
```

### API / agent wallet

API wallet（別名 agent wallet）は、master account によって承認された Hyperliquid の委任取引用キーです。取引はできますが出金はできないため、自動化や AI エージェントに限定権限の signer を渡すのに最適です:

```bash
hyperliquid api-wallet create --name trading-agent
hyperliquid api-wallet list <MASTER_ADDRESS>
hyperliquid api-wallet revoke --name trading-agent
```

### 代替 signer ソース

単一コマンドで OWS vault を使いたくない場合は、signer を明示的に渡します:

```bash
hyperliquid --keystore ~/.foundry/keystores/my-wallet ...
hyperliquid --private-key 0x... ...     # avoid in shared shells / history
```

または環境変数を設定します:

```bash
export HYPERLIQUID_PRIVATE_KEY=0x...
export OWS_PASSPHRASE=...               # unlock encrypted OWS wallet
```

### 安全ルール

- 秘密鍵、ニーモニック、keystore ファイル、OWS シークレット、設定データベースは**絶対に**コミットしないでください。
- 共有環境では、生の `--private-key` フラグより OWS wallet または keystore を優先してください。
- スクリプトやエージェントに取引を委任する場合は API wallet を使用してください。これらは資金を出金できません。
- 本番移行前にフローをリハーサルしたいときは、いつでも `--testnet` を 1 つ付けるだけです。

## 出力形式

すべてのデータコマンドは同じ自動化インターフェイスを公開します:

| Flag | Purpose |
| --- | --- |
| `--format pretty\|table\|json` | 人間向けまたは機械向けの出力。 |
| `--select <FIELDS>` | JSON をカンマ区切りのフィールドに投影します。 |
| `--results-only` | エンベロープを取り除き、データのみを返します。 |
| `--max-results <N>` | クライアント側でトップレベルの list/map サイズを制限します。 |
| `--dry-run` | 対応する変更操作を検証しプレビューします。 |
| `--payload-json` / `--payload-file` | raw JSON を dry-run に入力します。 |

`HYPERLIQUID_AGENT=1` を設定する（または非 TTY で実行する）と、単発コマンドは自動的に JSON をデフォルトにします。エラーは安定したオブジェクトです:

```json
{"error": "Authentication required. Run `hyperliquid setup` to configure your wallet."}
```

すべての変更コマンドには、エージェントが実行前に読める `schema` 記述が同梱されています。これにはリスク分類、確認要件、dry-run サポートが含まれます。

エージェント運用ガイドは [`SKILL.md`](SKILL.md) を参照してください。

## 用語とアドレス selector

| Domain | Examples |
| --- | --- |
| OWS wallet/account record | `account add`、`account ls`、`account set-default`、および関連コマンドで管理される OWS wallet record。 |
| Selected signer | 認証済みアクションの署名に使われるキー。フラグ、環境/設定、グローバル `--account`、または OWS selector から選択されます。 |
| Protocol user address | fills、portfolio、fees、order status などの info クエリで使われる公開 Hyperliquid user address。 |
| Master account | API wallet を承認し、subaccount を所有できるプロトコル上の所有者アカウント。 |
| API wallet / agent wallet | master account によって承認された委任 Hyperliquid trading key。master account のために取引できますが、出金はできません。 |
| OWS signer | `--ows-signer` で選択される Open Wallet Standard signer ソース。 |
| Subaccount | master account によって制御されるプロトコル subaccount。 |
| Protocol address | recipient、vault、validator、builder、または類似オブジェクトのためのリテラルなオンチェーン/プロトコルアドレス。 |

アドレスに似たコマンド入力は、3 つの安全クラスに分類されます:

| Class | Accepted values | Used for |
| --- | --- | --- |
| `ACCOUNT_SELECTOR` | 保存済み account alias、保存済み account id、または `0x` address | `--account` で signer を選択する、または OWS wallet レコードを管理するため。 |
| `USER` | `0x` user address、または文書化された安全な stored-account selector | `account portfolio`、`orders status --user`、fee クエリなどの公開 lookup。 |
| `*_ADDRESS` | 明示的な `0x` protocol address のみ | 送金 recipient、vault、validator、builder、その他のプロトコルオブジェクト。ローカル alias がこれらのフィールドに代入されることはありません。 |

エージェントにとって、`hyperliquid --format json schema ...` のツールスキーマは、例や説明文と矛盾する場合の入力セマンティクスの信頼できる情報源です。

CLI が受け付ける正規のトップレベル alias:

- `api-wallets` -> `api-wallet`
- `subaccounts` -> `subaccount`
- `transfers` -> `transfer`
- `vaults` -> `vault`

## コマンドリファレンス

### グローバルオプション

| Option | Description |
| --- | --- |
| `-f, --format pretty\|table\|json` | 出力形式。有効なデフォルトは TTY では `pretty`、非 TTY stdout または `HYPERLIQUID_AGENT=1` では JSON です。明示的な `--format` は `HYPERLIQUID_FORMAT` より優先されます。 |
| `--private-key <PRIVATE_KEY>` | 生の秘密鍵で署名します。環境と設定を上書きします。 |
| `--keystore <PATH>` | Foundry 互換 keystore ファイルで署名します。 |
| `--keystore-password <PASSWORD>` | Keystore パスワード。人間にはより安全なシークレットソースを推奨します。 |
| `--account <SELECTOR>` | signer として使用する保存済み wallet alias、id、または address。他の signer フラグと競合します。 |
| `--ows-signer <SELECTOR>` | OWS wallet selector（name または id）。identity/dry-run の配管用に `0x` address を受け付けます。Alias: `--wallet`。ローカル signer フラグと競合します。 |
| `--testnet` | API 呼び出しと署名済みアクションを Hyperliquid testnet にルーティングします。 |
| `--select <FIELDS>` | JSON 出力をカンマ区切りのフィールドに投影します。 |
| `--results-only` | 共通 JSON エンベロープを取り除き、データのみを返します。 |
| `--max-results <N>` | クライアント側でトップレベルの list/map 結果を制限します。 |
| `--dry-run` | 副作用なしで変更コマンドを検証しプレビューします。 |
| `--payload-json <JSON>` / `--payload-file <PATH\|->` | 変更 dry-run 用の raw JSON payload context を提供します。 |
| `--no-update-check` | この呼び出しの release update check を無効にします。 |
| `-h, --help` / `-V, --version` | help または version 情報を表示します。 |

### マーケットデータ

| Command | Description |
| --- | --- |
| `perps list [--dex <DEX>]` | perpetual market を一覧表示します。 |
| `perps get <COIN> [--dex <DEX>]` | 1 つの perpetual market を表示します。 |
| `spot list` | spot market を一覧表示します。 |
| `spot get <PAIR>` | 1 つの spot pair を表示します。例: `PURR/USDC`。 |
| `outcomes list [--limit <N>]` | `outcomeMeta` からアクティブな outcome market side を一覧表示します。 |
| `outcomes get #<ENCODING>` / `outcomes get +<ENCODING>` | outcome side metadata と派生 asset ID を表示します。 |
| `book <COIN> [-w] [--max-ticks <TICKS>]` | L2 order book snapshot を表示するか、更新を watch します。 |
| `mids [-w] [--max-ticks <TICKS>]` | すべての mid price を表示します。 |
| `candles <COIN> [--interval <INTERVAL>] [--limit <N>] [-w] [--max-ticks <TICKS>]` | candle 履歴を表示します。 |
| `spread <COIN>` | bid、ask、spread を表示します。 |
| `funding <COIN>` | 現在および予測 funding を表示します。 |
| `meta` | raw exchange metadata を表示します。 |
| `status` | API health と rate-limit context を表示します。 |

### アカウント、ウォレット、セットアップ

| Command | Description |
| --- | --- |
| `setup [-y] [--approve-builder\|--no-approve-builder]` | 初回セットアップのガイド付きウィザードを実行します。 |
| `wallet create` | 新しいウォレットを作成して保存します。 |
| `wallet import [PRIVATE_KEY]` | ウォレットをインポートします。非表示プロンプトを使う場合はキーを省略します。 |
| `wallet show` | 現在のウォレット metadata を表示します。 |
| `wallet address` | 設定済み wallet address のみを出力します。 |
| `wallet import-mnemonic [MNEMONIC] [--alias <ALIAS>]` | BIP-39 mnemonic phrase からウォレットをインポートします。 |
| `wallet list` | OWS vault 内のすべてのウォレットを一覧表示します。 |
| `wallet rename <SELECTOR> <NEW_NAME>` | ウォレットの名前を変更します。 |
| `wallet delete <SELECTOR>` | ウォレットを削除します。`-y` がない限りプロンプトを表示します。 |
| `wallet export <SELECTOR> [-y]` | ウォレットシークレット（mnemonic または private key）をエクスポートします。 |
| `wallet reset [-y]` | 確認後にウォレット設定を削除します。 |
| `account fees [ADDRESS_OR_WALLET]` | fee schedule と volume context を照会します。 |
| `account fills [ADDRESS_OR_WALLET] [--start <TIME>] [--end <TIME>] [--aggregate-by-time]` | 公開 fill 履歴を照会します。必要に応じて時間単位で集計します。 |
| `account ledger [ADDRESS_OR_WALLET] --start <TIME> [--end <TIME>]` | 入金、出金、送金、その他の funding 以外の ledger update を照会します。 |
| `account funding [ADDRESS_OR_WALLET] --start <TIME> [--end <TIME>]` | ユーザーの funding payment 履歴を照会します。 |
| `account orders [ADDRESS_OR_WALLET]` | 公開 open orders を照会します。 |
| `account portfolio [ADDRESS_OR_WALLET]` | 公開 portfolio summary を照会します。 |
| `account portfolio-history [ADDRESS_OR_WALLET]` | frontend portfolio graph/history data を照会します。 |
| `account rate-limit [ADDRESS_OR_WALLET]` | user rate-limit context を照会します。 |
| `account subaccounts [ADDRESS_OR_WALLET]` | 公開 subaccounts を照会します。 |
| `account twap-history [ADDRESS_OR_WALLET]` | user TWAP order history を照会します。 |
| `account twap-fills [ADDRESS_OR_WALLET] [--start <TIME>] [--end <TIME>] [--aggregate-by-time]` | user TWAP slice fills を照会します。 |
| `account abstraction [ADDRESS]` | アドレスの account abstraction mode を読み取ります。`ADDRESS` が省略された場合は selected account を読み取ります。 |
| `account abstraction set --mode disabled\|unified-account\|portfolio-margin` | 設定済み signer の account abstraction を設定します。`-y` がない限りプロンプトを表示します。 |
| `subaccount list [ADDRESS_OR_WALLET]` | master address の公開 subaccounts を照会します。 |
| `subaccount create --name <NAME>` | master account によって署名された subaccount を作成します。 |
| `account add [PRIVATE_KEY] [--alias <ALIAS>] [--type <TYPE>] [--default]` / `account ls` / `account set-default [SELECTOR]` / `account remove [SELECTOR] [-y]` | 保存済みウォレットを管理します。 |
| `api-wallet create [--name <NAME>] [--expires-in <DURATION>] [--agent-address <ADDRESS>] [--generate]` | Hyperliquid API/agent wallet を生成し承認します。 |
| `api-wallet approve [--name <NAME>] [--expires-in <DURATION>] (--agent-address <ADDRESS>\|--generate)` | 既存または生成済みの agent wallet address を承認します。 |
| `api-wallet list [ACCOUNT]` | master account によって承認された API wallet を一覧表示します。 |
| `api-wallet revoke --name <NAME>` | 名前付き API wallet を短命の使い捨て agent に置き換えます。 |

API wallet は承認元の master account のために取引アクションへ署名できますが、出金はできません。info クエリには master または subaccount address を使用してください。`api-wallet create` がローカル agent keypair を生成する場合、そのアドレスに対する `approveAgent` を送信する前に private key を一度だけ表示します。その key は CLI が後から自動復元しないため安全に保管してください。

### 取引と送金

| Command | Description |
| --- | --- |
| `orders open [-w] [--max-ticks <TICKS>]` | open orders を一覧表示します。 |
| `orders history` | order history を一覧表示します。 |
| `orders status --user <ADDRESS> (--oid <OID>\|--cloid <CLOID>)` | 公開 order status を照会します。 |
| `orders create --coin <COIN> --side buy\|sell [--type limit\|market\|stop-loss\|take-profit\|stop-limit\|take-limit] [--price <PX>] [--trigger-price <PX>] [--size <SIZE>\|--amount <USDC>] [--dex <DEX>] [--reduce-only] [--on-behalf-of <ACCOUNT_SELECTOR>] [--cloid <CLOID>] [-y]` | limit、market、stop-loss、take-profit、stop-limit、take-limit 注文を作成します。`--on-behalf-of` は `vaultAddress` として使われる acting-account selector です。 |
| `orders scale --coin <COIN> --side buy\|sell --start-price <PX> --end-price <PX> --total-size <SIZE> --orders <N>` | 等間隔に並べた limit order のバッチを作成します。 |
| `orders batch-create --orders-file <PATH>` | JSON から limit order のバッチを作成します。 |
| `orders create --coin <COIN> --side buy\|sell [--take-profit <PX>] [--stop-loss <PX>] [--grouping normal-tpsl] ...` | 固定サイズの TP/SL 子注文を持つ親注文を作成します。 |
| `orders tpsl --coin <COIN> (--take-profit <PX>\|--stop-loss <PX>) [--grouping position-tpsl] [--side buy\|sell] [--size <SIZE>] [--margin-mode cross\|isolated]` | 現在のポジションに紐づく TP/SL 注文を作成します。 |
| `orders cancel (ORDER_ID\|--cloid <CLOID>)` | order ID または client order ID でキャンセルします。 |
| `orders cancel-all [--coin <COIN>] [--dex <DEX>] [-y]` | すべての open orders をキャンセルします。必要に応じて coin または DEX で絞り込みます。 |
| `orders modify (ORDER_ID\|--cloid <CLOID>) [--price <PRICE>] [--trigger-price <PRICE>] [--size <SIZE>]` | 既存の注文を変更します。 |
| `orders twap-create --coin <COIN> --side buy\|sell --size <SIZE> --duration <SECONDS> [--dex <DEX>] [--margin-mode cross\|isolated] [-y]` | TWAP order を作成します。 |
| `orders twap-cancel <TWAP_ID> --coin <COIN> [--dex <DEX>]` | TWAP order をキャンセルします。 |
| `orders schedule-cancel (--in <DURATION>\|--clear)` | dead man's switch を設定します。 |
| `positions list [-w] [--max-ticks <TICKS>]` | open positions を一覧表示します。 |
| `positions update-leverage --coin <COIN> --leverage <N> [--isolated]` | leverage を更新します。 |
| `positions update-margin --coin <COIN> --amount <AMOUNT>` | isolated margin を追加または削除します。 |
| `transfer spot-to-perp --amount <USDC> [-y]` | USDC を spot から perp に移動します。 |
| `transfer perp-to-spot --amount <USDC> [-y]` | USDC を perp から spot に移動します。 |
| `transfer send --to <ADDRESS> --amount <USDC> [-y]` | USDC を別のアドレスに送信します。 |
| `transfer spot-send --to <ADDRESS> --token <TOKEN> --amount <AMOUNT> [-y]` | spot token を別のアドレスに送信します。 |
| `transfer send-asset --to <ADDRESS> --source perp\|spot\|dex:<DEX> --dest perp\|spot\|dex:<DEX> --token <TOKEN> --amount <AMOUNT> [--from-subaccount <ADDRESS>] [-y]` | アカウント、spot、perp、または DEX context 間で asset を送信します。 |
| `transfer withdraw --to <ADDRESS> --amount <USDC> [-y]` | USDC を Arbitrum に出金します。 |
| `subaccount transfer --subaccount <ACCOUNT_SELECTOR> --amount <USDC> --direction deposit\|withdraw [-y]` | USDC を subaccount に、または subaccount から移動します。subaccount フィールドは acting-account selector であり、汎用の transfer recipient ではありません。 |
| `subaccount spot-transfer --subaccount <ACCOUNT_SELECTOR> --token <TOKEN> --amount <AMOUNT> --direction deposit\|withdraw [-y]` | spot token を subaccount に、または subaccount から移動します。subaccount フィールドは acting-account selector であり、汎用の transfer recipient ではありません。 |

`api-wallets` は `api-wallet` の alias として受け付けられます。
`subaccounts` は `subaccount` の alias として受け付けられます。
`transfers` は `transfer` の alias として受け付けられます。

時間範囲付きの account history コマンドは RFC3339 タイムスタンプと epoch milliseconds を受け付けます。CLI は milliseconds を Hyperliquid に送信します。例:

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

Hyperliquid の文書化された history endpoint は範囲制限されたウィンドウを返し、多くの場合 endpoint に応じて約 500 行または 2000 行に制限されます。エクスポートでは、隣接する重複しないウィンドウを照会し、`--format json` を維持してください。グローバル `--max-results` は、確認用にローカル CLI 出力を切り詰める場合にのみ使用してください。

Outcome market notation（`#N` spot coin と `+N` token name）は、`outcomes list` と `outcomes get` による発見で利用できます。`orders create --coin '#N' --dry-run` は、エンコードされた asset id を含む安定した outcome order preview を出力し、signer が設定されている場合、ライブの limit-order 送信は検証済み outcome notation をサポートします。ライブ outcome order を発注する前に、まず `--dry-run` を使ってエンコードされた asset id と signed-action preview を確認してください。

### 高度なコマンド

| Command | Description |
| --- | --- |
| `staking summary [ADDRESS]` / `staking validators` / `staking rewards [ADDRESS]` / `staking history [ADDRESS]` | staking state と history を読み取ります。 |
| `staking delegate --validator <ADDRESS> --amount <AMOUNT>` / `staking undelegate --validator <ADDRESS> --amount <AMOUNT>` / `staking deposit --amount <AMOUNT>` / `staking withdraw --amount <AMOUNT>` / `staking claim-rewards` | staking action を送信します。 |
| `staking link initiate --user <ADDRESS>` / `staking link finalize --user <ADDRESS>` | fee discount attribution のために trading account と staking account をリンクします。Dry-run には永続性/制御に関する警告が含まれます。live コマンドには確認または `--yes` が必要です。 |
| `vault list [--kind protocol\|user\|normal\|child\|parent] [--user <ADDRESS>] [--limit <N>] [--sort tvl\|apr\|age\|name]` / `vault search <QUERY> [--user <ADDRESS>] [--limit <N>] [--sort tvl\|apr\|age\|name]` / `vault get <ADDRESS>` / `vault positions <ADDRESS>` | vault state を発見および照会します。API が返す場合、`--user` は user deposit context を含めます。 |
| `vault deposit --vault <ADDRESS> --amount <AMOUNT>` / `vault withdraw --vault <ADDRESS> --amount <AMOUNT>` | vault transfer を送信します。 |
| `borrowlend rates` / `borrowlend get <TOKEN>` / `borrowlend user [ADDRESS]` | borrow/lend market を照会します。 |
| `borrowlend supply <TOKEN> --amount <AMOUNT>` / `borrowlend withdraw <TOKEN> (--amount <AMOUNT>\|--max)` | 検証済み wallet-signed exchange `borrowLend` supply/withdraw action を送信します。まず `--dry-run` を使って action を確認してください。 |
| `builder max-fee --user <ADDRESS> --builder <ADDRESS>` | ユーザーが承認した max builder fee を照会します。 |
| `builder approved --user <ADDRESS>` | ユーザーが fee cap 付きで承認したすべての builder を一覧表示します。 |
| `builder approve --builder <ADDRESS> --max-fee-rate <PERCENT> [-y]` | 設定済み master signer の builder fee cap を承認または取り消します。 |
| `prio status` / `prio bid --max <HYPE> --ip <IP> [--slot <N>]` | gossip priority auction を照会するか入札します。 |
| `referral register <CODE>` / `referral set [CODE]` / `referral status` | 自分の referral code を登録する、referrer を設定する、または referral state を確認します。 |
| `feedback (--scenario-json <JSON>\|--scenario-file <PATH\|->) [--contact <CONTACT>] [--tags <TAG>] [--url <URL>]` | 構造化された CLI feedback を scenario JSON object として設定済み feedback endpoint に送信します。rate-limit attribution のために scenario に `agent_address`、`signer_address`、または `wallet_address` を含め、デフォルトを上書きするには `--url` を使用します。 |
| `schema [COMMAND...]` | エージェント向けの機械可読な command schema を表示します。 |
| `subscribe trades --asset <ASSET>` / `subscribe orderbook --asset <ASSET>` / `subscribe candles --asset <ASSET> [--interval <INTERVAL>]` / `subscribe all-mids` / `subscribe order-updates` / `subscribe fills` `[--max-events <N>] [--idle-timeout-ms <MS>]` | WebSocket event をストリーミングします。 |
| `update` | Linux/macOS では最新の GitHub release からこのバイナリを更新します。Windows ユーザーは `install.sh` を再実行して最新の `.zip` release をインストールしてください。グローバル `--dry-run` でプレビューできます。 |

`vaults` は `vault` の alias として受け付けられます。

Builder approval は `0.001%` のような percent string を使用します。`0%` は承認済み max fee をゼロに設定して取り消します。approval action は API wallet ではなく master account によって署名される必要があります。`orders create` は対になった `--builder <ADDRESS> --builder-fee-rate <PERCENT>` フラグを受け付け、署名済み order action に公式の `builder: { b, f }` wire object を含めます。perp builder fee は `0.1%`、spot builder fee は `1%` が上限です。fork/distribution はビルド時にデフォルトの order builder parameters を組み込めます:

```bash
HYPERLIQUID_DEFAULT_BUILDER_ADDRESS=0x... \
HYPERLIQUID_DEFAULT_BUILDER_FEE_RATE=0.001% \
cargo build --release --bin hyperliquid
```

同じ名前の runtime env vars は build-time defaults を上書きします。`hyperliquid setup` 中、build-time defaults は builder/fee の候補として表示され、Enter を押すと受け入れます。runtime env vars が存在する場合はそれらが候補になります。`HYPERLIQUID_DEFAULT_REFERRAL_CODE` も setup と `referral set` defaults について同じ build-time/runtime パターンに従います。release workflow は、同名の GitHub Actions repository variables からこれらの値を渡します。env も config も defaults を提供しない場合でも、ユーザーは注文ごとに `--builder` と `--builder-fee-rate` を渡せます。

Vault discovery は、アドレスを書き換えずに vault detail と transfer dry-run に入力できます:

```bash
hyperliquid --format json vault list --kind protocol --limit 5 --sort tvl
hyperliquid --format json vault get 0x...
hyperliquid --format json --dry-run vault deposit --vault 0x... --amount 5
```

完全なリファレンスは `hyperliquid --help` と `hyperliquid <command> --help`、および [`docs/`](docs/) 内のガイドを参照してください。

## Testnet

mainnet に触れる前に reads、dry-run、承認済み live testnet flow をリハーサルするには `--testnet` を使用してください。Testnet は同じコマンドインターフェイスを、testnet API endpoint と別個の account state で使用します。

## 安全モデル

CLI は副作用を可視化するように設計されています:

- 読み取り専用コマンドは秘密鍵に一切触れません。
- 署名は明示的な `--account`、`--ows-signer`、`--keystore`、`--private-key`、または保存済み OWS wallet 経由でのみ発生します。
- `--testnet` は API 呼び出しと署名済みアクションを Hyperliquid testnet に明確にルーティングします。
- `--dry-run` は対応するすべての変更操作を送信せずに検証しプレビューします。
- live mainnet の変更操作と破壊的なローカルシークレット操作は、schema が prompt-gated と示す場合に確認を要求します。コマンドごとの確認ポリシーは `hyperliquid --format json schema ...` で確認してください。対応箇所では `-y` / `--yes` がプロンプトをスキップします。
- 送金 recipient と protocol object address は明示的な `0x` address でなければなりません。ローカル alias が暗黙に代入されることはありません。

## 終了コード

| Code | Meaning |
| --- | --- |
| `0` | 成功 |
| `1` | 内部エラー |
| `2` | 使用方法、検証、または設定エラー |
| `10` | 認証がない、または無効 |
| `11` | Rate limited |
| `12` | API またはネットワークが利用不可 |
| `13` | サポートされていない入力、無効な asset、または不明な DEX |
| `14` | Stale data |
| `15` | 部分的な結果 |

## 設定

解決順序: CLI flags → environment variables → `~/.config/hyperliquid/config.json`。

| Variable | Purpose |
| --- | --- |
| `HYPERLIQUID_PRIVATE_KEY` | 署名用の private key（OWS または keystore を推奨）。 |
| `HYPERLIQUID_NETWORK` | `mainnet` または `testnet`。 |
| `HYPERLIQUID_FORMAT` | agent/non-TTY fallback より前に使う明示的な default output format（`pretty`、`table`、または `json`）。 |
| `HYPERLIQUID_AGENT` | agent defaults を強制するには `1` に設定します。 |
| `HYPERLIQUID_WATCH_MAX_TICKS` | snapshot watch mode の default tick limit。 |
| `HYPERLIQUID_SUBSCRIBE_MAX_EVENTS` | agent context の WebSocket subscribe commands 用 default event limit。 |
| `OWS_PASSPHRASE` | 暗号化 OWS wallet を unlock するための passphrase。 |
| `HYPERLIQUID_OWS_VAULT_PATH` | OWS vault path を上書きします（デフォルト `~/.hyperliquid`）。 |
| `HYPERLIQUID_API_BASE_URL` / `HYPERLIQUID_MAINNET_API_BASE_URL` / `HYPERLIQUID_TESTNET_API_BASE_URL` | API base URL を上書きします。すべての override は loopback/local test endpoints に制限されます。 |
| `HYPERLIQUID_DEFAULT_BUILDER_ADDRESS` / `HYPERLIQUID_DEFAULT_BUILDER_FEE_RATE` | per-order builder fee parameters と setup suggestions の runtime defaults。 |
| `HYPERLIQUID_DEFAULT_REFERRAL_CODE` | setup と `referral set` の runtime default referral code。 |
| `HYPERLIQUID_FEEDBACK_URL` | `hyperliquid feedback` の runtime または build-time default endpoint。 |
| `HYPERLIQUID_NO_UPDATE_CHECK` | truthy の場合に release update checks を無効にします。 |

## 開発

`hyperliquid feedback` の default endpoint を埋め込むには、build environment に `HYPERLIQUID_FEEDBACK_URL` を設定します。runtime では、`--url`、runtime `HYPERLIQUID_FEEDBACK_URL`、埋め込み build-time default の順に優先されます。

```bash
HYPERLIQUID_FEEDBACK_URL="https://<worker-subdomain>/feedback" cargo build --release
```

```bash
cargo build
cargo test
cargo clippy -- -D warnings
cargo fmt --check
```

Taskfile による任意の再現可能な QA:

```bash
task bind
task qa:matrix
```

規約、テストルール、エージェント優先の出力契約は [`AGENTS.md`](AGENTS.md) と [`CONTRIBUTING.md`](CONTRIBUTING.md) にあります。

## 別のものが必要な場合

- 完全なアプリケーションや取引システムを構築しますか？[`hypersdk`](https://github.com/infinitefield/hypersdk) を直接使用してください。
- 長時間実行の戦略実行、バックテスト、ホスト型 bot が必要ですか？専用の bot framework を選んでください。
- 深い historical tick data や cross-exchange research を探していますか？market-data platform を使用してください。

人間、スクリプト、エージェントのすべてで同じように動作する Hyperliquid への単一の運用インターフェイスが必要なときに、`hyperliquid` を使用してください。

## 謝辞

Hyperliquid HTTP、WebSocket、EIP-712 signing のために Infinite Field による [`hypersdk`](https://github.com/infinitefield/hypersdk) の上に構築されています。`hypersdk` は [Mozilla Public License 2.0](https://www.mozilla.org/en-US/MPL/2.0/) の下でライセンスされています。

## ライセンス

MIT — [`LICENSE`](LICENSE) を参照してください。

### サードパーティライセンス

`hyperliquid-cli` は MIT ですが、独自のライセンスを持つ open-source crate に依存しています:

| Dependency | License | Notes |
| --- | --- | --- |
| [`hypersdk`](https://github.com/infinitefield/hypersdk) | MPL-2.0 | Cargo dependency として変更せずに使用。 |
| [`alloy`](https://github.com/alloy-rs/alloy) family | MIT OR Apache-2.0 | EVM primitives と signers。 |
| [`tokio`](https://github.com/tokio-rs/tokio) | MIT | Async runtime。 |
| [`clap`](https://github.com/clap-rs/clap) | MIT OR Apache-2.0 | CLI framework。 |
| [`reqwest`](https://github.com/seanmonstar/reqwest) | MIT OR Apache-2.0 | HTTP client。 |
| [`rust_decimal`](https://github.com/paupino/rust-decimal) | MIT | Fixed-point decimal math。 |
| [`ows-lib`](https://crates.io/crates/ows-lib) | See crate metadata | OWS wallet backend。 |

`hypersdk` は変更されていない upstream Cargo dependency として利用されているため、MPL-2.0 の file-level copyleft は公開 upstream repository によって満たされています。この CLI を fork し、ツリー内の `hypersdk` source files を変更する場合、それらのファイルは MPL-2.0 のままでなければならず、変更済みソースを利用可能にする必要があります。`hyperliquid-cli` の残りの部分は MIT のままです。

完全な推移的ライセンスレポートを生成するには:

```bash
cargo install cargo-license
cargo license
```

## 免責事項

本ソフトウェアは、いかなる種類の保証もなく「現状のまま」提供されます。分散型取引所での取引には大きな損失リスクが伴います。キー、署名済みアクション、取引判断については、あなた自身が単独で責任を負います。本プロジェクトは Hyperliquid と公式に提携していません。
