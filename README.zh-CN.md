# Hyperliquid CLI

[![Crates.io](https://img.shields.io/badge/crates.io-v0.1.0-orange.svg)](https://crates.io/crates/hyperliquid-cli)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.93%2B-blue.svg)](https://www.rust-lang.org)
[![Built on hypersdk](https://img.shields.io/badge/built%20on-hypersdk-blueviolet.svg)](https://github.com/infinitefield/hypersdk)

Languages: [English](README.md) | [简体中文](README.zh-CN.md) | [日本語](README.ja-JP.md) | [한국어](README.ko-KR.md)

**为你的 AI 代理提供一个用于交易 Hyperliquid 的 CLI 和钱包。**

`hyperliquid` 是一个单一二进制文件，可为你的个人代理——OpenClaw、Hermes、PicoClaw、Claude、Codex，或任何能够调用 shell 的 LLM——提供生产级命令行和加密钱包，用于 [Hyperliquid](https://app.hyperliquid.xyz)。市场、订单、转账、质押、金库、借贷、builder 费用、WebSocket 流：每个操作都以干净的 JSON 命令呈现，代理可以读取、推理并执行。

把它放进你的代理工具箱，它就能检查价格、下单、管理仓位并流式读取订单簿——全部通过一个二进制文件完成，并内置 dry-run、schema 和安全闸门。

## 为什么选择 hyperliquid-cli

- **代理优先。** 为代理循环而构建。每个命令都支持通过 `--format json` 输出 JSON、字段投影（`--select`）、结果上限（`--max-results`）以及机器可读的 `schema` 输出。`HYPERLIQUID_AGENT=1`（或非 TTY stdout）会自动默认输出 JSON。你的代理读取稳定的 snake_case 键、结构化错误对象和定义明确的退出码——无需抓取文本，无需猜测。
- **给代理使用的钱包。** 创建一个可以交易但永远不能提现的 API wallet（也称为 agent wallet）。把它交给 OpenClaw、Hermes、Claude 或任何自动化系统，它就能在受限权限内运行。OWS wallet secrets 保存在加密 vault 中；由 CLI 生成的 API wallet private key 会且只会打印一次，方便你安全保存。
- **一个工具，覆盖广泛协议。** 市场、perps、spot、HIP-3 DEXes、订单、转账、子账户、金库、质押、借贷、builder 费用、推荐、账户抽象以及 WebSocket 订阅——全部在一个二进制文件后面。
- **默认安全。** 被 schema 标记为 prompt-gated 的 live mainnet 可变更操作需要确认。`--dry-run` 会在受支持的副作用发生前预览。testnet 只需一个标志即可使用。API wallets 按协议设计无法提现。
- **十进制正确。** 每个价格、数量和金额都使用 `rust_decimal`。没有浮点数，也没有意外舍入。
- **单个二进制文件。** 使用 Rust 构建，并基于 [`hypersdk`](https://github.com/infinitefield/hypersdk)。数秒安装，可放入容器，随处运行。

## 安装

```bash
curl -fsSLO https://raw.githubusercontent.com/hypurrclaw/hyperliquid-cli/main/install.sh
sh install.sh
hyperliquid --version
```

安装器会在把二进制文件复制到 `~/.local/bin` 之前验证 SHA-256 校验和。默认安装最新 release；可使用 `HYPERLIQUID_CLI_REPO=OWNER/REPO`、`HYPERLIQUID_CLI_VERSION=v0.1.0` 和 `BIN_DIR=/path/to/bin` 覆盖仓库、固定版本或安装目录。

从源码安装：

```bash
cargo install --path . --bin hyperliquid
```

需要 Rust 1.93+。

## 快速开始

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

## 钱包设置

`hyperliquid` 使用 **Open Wallet Standard (OWS)** 作为其唯一钱包后端。钱包保存在磁盘上的加密 vault 中（默认 `~/.hyperliquid`，可通过 `HYPERLIQUID_OWS_VAULT_PATH` 覆盖）。通过隐藏提示交互式输入的秘密不会回显、记录到日志或打印；显式的 `wallet export` 和生成 API wallet 的流程是有意例外，可能一次性显示 secret。

### 引导式设置

新操作者的最快路径：

```bash
hyperliquid setup
```

向导会引导你创建或导入钱包、选择默认网络、持久化任何打包的 builder fee / referral 默认值，并可选择批准默认 builder fee 上限。对于无人值守环境，可以非交互式接受所有默认值：

```bash
hyperliquid setup -y
```

### 直接创建或导入钱包

```bash
hyperliquid wallet create               # generate a new wallet
hyperliquid wallet import               # paste a private key (hidden prompt)
hyperliquid wallet import-mnemonic      # paste a BIP-39 mnemonic (hidden prompt)
```

新创建或导入的钱包会成为默认签名者。

### 管理多个钱包

```bash
hyperliquid wallet list                 # all wallets in the OWS vault
hyperliquid wallet show                 # current default
hyperliquid wallet address              # just the address
hyperliquid wallet rename <SELECTOR> <NEW_NAME>
hyperliquid wallet export <SELECTOR>    # reveal secret (with confirmation)
hyperliquid wallet delete <SELECTOR>
```

在不更改默认值的情况下为单个命令选择特定钱包：

```bash
hyperliquid --account alice orders open
hyperliquid --ows-signer 0xabc... positions list
```

### API / agent wallets

API wallets（也称为 agent wallets）是由主账户批准的委托 Hyperliquid 交易密钥。它们可以交易但不能提现——非常适合把受限签名者交给自动化系统或 AI 代理：

```bash
hyperliquid api-wallet create --name trading-agent
hyperliquid api-wallet list <MASTER_ADDRESS>
hyperliquid api-wallet revoke --name trading-agent
```

### 其他签名者来源

如果你不想为单个命令使用 OWS vault，请显式传入签名者：

```bash
hyperliquid --keystore ~/.foundry/keystores/my-wallet ...
hyperliquid --private-key 0x... ...     # avoid in shared shells / history
```

或设置环境变量：

```bash
export HYPERLIQUID_PRIVATE_KEY=0x...
export OWS_PASSPHRASE=...               # unlock encrypted OWS wallet
```

### 安全规则

- **绝不要** 提交私钥、助记词、keystore 文件、OWS secrets 或配置数据库。
- 在共享环境中，优先使用 OWS wallets 或 keystores，而不是原始 `--private-key` 标志。
- 将交易委托给脚本或代理时使用 API wallets——它们无法提取资金。
- 每当你想在上线前演练一个流程时，`--testnet` 只需一个标志即可使用。

## 输出格式

每个数据命令都公开相同的自动化界面：

| 标志 | 用途 |
| --- | --- |
| `--format pretty\|table\|json` | 面向人类或机器的输出。 |
| `--select <FIELDS>` | 将 JSON 投影到逗号分隔的字段。 |
| `--results-only` | 去除 envelope，只返回数据。 |
| `--max-results <N>` | 在客户端限制顶层 list/map 大小。 |
| `--dry-run` | 验证并预览受支持的可变更操作。 |
| `--payload-json` / `--payload-file` | 将原始 JSON 输入到 dry-run。 |

设置 `HYPERLIQUID_AGENT=1`（或在非 TTY 中运行），一次性命令会自动默认输出 JSON。错误是稳定对象：

```json
{"error": "Authentication required. Run `hyperliquid setup` to configure your wallet."}
```

每个可变更命令都附带一个代理可在行动前读取的 `schema` 描述——包括风险等级、确认要求和 dry-run 支持。

请参阅 [`SKILL.md`](SKILL.md) 获取代理操作指南。

## 术语和地址选择器

| 领域 | 示例 |
| --- | --- |
| OWS wallet/account record | 由 `account add`、`account ls`、`account set-default` 及相关命令管理的 OWS wallet record。 |
| Selected signer | 用于签署认证操作的密钥，来自标志、环境/配置、全局 `--account` 或 OWS selector。 |
| Protocol user address | 用于 fills、portfolio、fees 或 order status 等 info queries 的公开 Hyperliquid 用户地址。 |
| Master account | 可以批准 API wallets 并拥有 subaccounts 的协议所有者账户。 |
| API wallet / agent wallet | 由 master account 批准的委托 Hyperliquid 交易密钥。它可以为 master account 交易，但不能提现。 |
| OWS signer | 使用 `--ows-signer` 选择的 Open Wallet Standard signer source。 |
| Subaccount | 由 master account 控制的协议 subaccount。 |
| Protocol address | 用于 recipient、vault、validator、builder 或类似对象的字面链上/协议地址。 |

类似地址的命令输入分为三类安全等级：

| 类别 | 接受的值 | 用于 |
| --- | --- | --- |
| `ACCOUNT_SELECTOR` | Stored account alias、stored account id 或 `0x` address | 使用 `--account` 选择签名者，或管理 OWS wallet records。 |
| `USER` | `0x` user address，或有文档说明的安全 stored-account selector | 公共查询，例如 `account portfolio`、`orders status --user` 或 fee queries。 |
| `*_ADDRESS` | 仅显式 `0x` protocol address | Transfer recipients、vaults、validators、builders 和其他 protocol objects。Local aliases 不会替换到这些字段。 |

对于代理，当 `hyperliquid --format json schema ...` 工具 schema 与示例或说明性文字冲突时，它们是输入语义的权威来源。

CLI 接受的规范顶层别名：

- `api-wallets` -> `api-wallet`
- `subaccounts` -> `subaccount`
- `transfers` -> `transfer`
- `vaults` -> `vault`

## 命令参考

### 全局选项

| 选项 | 描述 |
| --- | --- |
| `-f, --format pretty\|table\|json` | 输出格式。TTY 中有效默认值为 `pretty`，非 TTY stdout 或 `HYPERLIQUID_AGENT=1` 时为 JSON；显式 `--format` 优先于 `HYPERLIQUID_FORMAT`。 |
| `--private-key <PRIVATE_KEY>` | 使用原始私钥签名。覆盖环境和配置。 |
| `--keystore <PATH>` | 使用 Foundry 兼容 keystore 文件签名。 |
| `--keystore-password <PASSWORD>` | Keystore 密码。对人类用户建议使用更安全的 secret 来源。 |
| `--account <SELECTOR>` | 用作签名者的已存储 wallet alias、id 或 address。与其他签名者标志冲突。 |
| `--ows-signer <SELECTOR>` | OWS wallet selector（name 或 id）。接受 `0x` addresses 用于身份/dry-run 管线。别名：`--wallet`。与本地签名者标志冲突。 |
| `--testnet` | 将 API 调用和已签名操作路由到 Hyperliquid testnet。 |
| `--select <FIELDS>` | 将 JSON 输出投影到逗号分隔的字段。 |
| `--results-only` | 去除常见 JSON envelopes，仅返回数据。 |
| `--max-results <N>` | 在客户端限制顶层 list/map 结果。 |
| `--dry-run` | 验证并预览可变更命令，而不产生副作用。 |
| `--payload-json <JSON>` / `--payload-file <PATH\|->` | 为可变更 dry-runs 提供原始 JSON payload context。 |
| `--no-update-check` | 禁用本次调用的 release 更新检查。 |
| `-h, --help` / `-V, --version` | 打印 help 或 version 信息。 |

### 市场数据

| 命令 | 描述 |
| --- | --- |
| `perps list [--dex <DEX>]` | 列出 perpetual markets。 |
| `perps get <COIN> [--dex <DEX>]` | 显示一个 perpetual market。 |
| `spot list` | 列出 spot markets。 |
| `spot get <PAIR>` | 显示一个 spot pair，例如 `PURR/USDC`。 |
| `outcomes list [--limit <N>]` | 从 `outcomeMeta` 列出活跃的 outcome market sides。 |
| `outcomes get #<ENCODING>` / `outcomes get +<ENCODING>` | 显示 outcome side metadata 和派生的 asset ID。 |
| `book <COIN> [-w] [--max-ticks <TICKS>]` | 显示 L2 order book snapshot 或 watch updates。 |
| `mids [-w] [--max-ticks <TICKS>]` | 显示所有 mid prices。 |
| `candles <COIN> [--interval <INTERVAL>] [--limit <N>] [-w] [--max-ticks <TICKS>]` | 显示 candle history。 |
| `spread <COIN>` | 显示 bid、ask 和 spread。 |
| `funding <COIN>` | 显示当前和预测 funding。 |
| `meta` | 显示原始 exchange metadata。 |
| `status` | 显示 API health 和 rate-limit context。 |

### 账户、钱包和设置

| 命令 | 描述 |
| --- | --- |
| `setup [-y] [--approve-builder\|--no-approve-builder]` | 运行引导式首次设置向导。 |
| `wallet create` | 创建并存储新钱包。 |
| `wallet import [PRIVATE_KEY]` | 导入钱包。省略 key 可使用隐藏提示。 |
| `wallet show` | 显示当前 wallet metadata。 |
| `wallet address` | 仅打印已配置 wallet address。 |
| `wallet import-mnemonic [MNEMONIC] [--alias <ALIAS>]` | 从 BIP-39 mnemonic phrase 导入钱包。 |
| `wallet list` | 列出 OWS vault 中的所有 wallets。 |
| `wallet rename <SELECTOR> <NEW_NAME>` | 重命名钱包。 |
| `wallet delete <SELECTOR>` | 删除钱包。除非使用 `-y`，否则会提示。 |
| `wallet export <SELECTOR> [-y]` | 导出 wallet secret（mnemonic 或 private key）。 |
| `wallet reset [-y]` | 确认后移除 wallet configuration。 |
| `account fees [ADDRESS_OR_WALLET]` | 查询 fee schedule 和 volume context。 |
| `account fills [ADDRESS_OR_WALLET] [--start <TIME>] [--end <TIME>] [--aggregate-by-time]` | 查询公共 fill history，可选择按时间。 |
| `account ledger [ADDRESS_OR_WALLET] --start <TIME> [--end <TIME>]` | 查询 deposits、withdrawals、transfers 和其他非 funding ledger updates。 |
| `account funding [ADDRESS_OR_WALLET] --start <TIME> [--end <TIME>]` | 查询用户 funding payment history。 |
| `account orders [ADDRESS_OR_WALLET]` | 查询公共 open orders。 |
| `account portfolio [ADDRESS_OR_WALLET]` | 查询公共 portfolio summary。 |
| `account portfolio-history [ADDRESS_OR_WALLET]` | 查询 frontend portfolio graph/history data。 |
| `account rate-limit [ADDRESS_OR_WALLET]` | 查询用户 rate-limit context。 |
| `account subaccounts [ADDRESS_OR_WALLET]` | 查询公共 subaccounts。 |
| `account twap-history [ADDRESS_OR_WALLET]` | 查询用户 TWAP order history。 |
| `account twap-fills [ADDRESS_OR_WALLET] [--start <TIME>] [--end <TIME>] [--aggregate-by-time]` | 查询用户 TWAP slice fills。 |
| `account abstraction [ADDRESS]` | 读取某地址的 account abstraction mode；若省略 `ADDRESS`，则读取所选账户。 |
| `account abstraction set --mode disabled\|unified-account\|portfolio-margin` | 为已配置签名者设置 account abstraction；除非使用 `-y`，否则会提示。 |
| `subaccount list [ADDRESS_OR_WALLET]` | 查询 master address 的公共 subaccounts。 |
| `subaccount create --name <NAME>` | 创建由 master account 签名的 subaccount。 |
| `account add [PRIVATE_KEY] [--alias <ALIAS>] [--type <TYPE>] [--default]` / `account ls` / `account set-default [SELECTOR]` / `account remove [SELECTOR] [-y]` | 管理已存储 wallets。 |
| `api-wallet create [--name <NAME>] [--expires-in <DURATION>] [--agent-address <ADDRESS>] [--generate]` | 生成并批准 Hyperliquid API/agent wallet。 |
| `api-wallet approve [--name <NAME>] [--expires-in <DURATION>] (--agent-address <ADDRESS>\|--generate)` | 批准现有或生成的 agent wallet address。 |
| `api-wallet list [ACCOUNT]` | 列出由 master account 批准的 API wallets。 |
| `api-wallet revoke --name <NAME>` | 用一个短期一次性 agent 替换命名 API wallet。 |

API wallets 可以为批准它们的 master account 签署交易操作，但不能提现。使用 master 或 subaccount address 进行 info queries。当 `api-wallet create` 生成本地 agent keypair 时，它会在为该地址提交 `approveAgent` 之前打印一次 private key；请安全保存该 key，因为 CLI 不会在之后自动恢复它。

### 交易和转账

| 命令 | 描述 |
| --- | --- |
| `orders open [-w] [--max-ticks <TICKS>]` | 列出 open orders。 |
| `orders history` | 列出 order history。 |
| `orders status --user <ADDRESS> (--oid <OID>\|--cloid <CLOID>)` | 查询公共 order status。 |
| `orders create --coin <COIN> --side buy\|sell [--type limit\|market\|stop-loss\|take-profit\|stop-limit\|take-limit] [--price <PX>] [--trigger-price <PX>] [--size <SIZE>\|--amount <USDC>] [--dex <DEX>] [--reduce-only] [--on-behalf-of <ACCOUNT_SELECTOR>] [--cloid <CLOID>] [-y]` | 创建 limit、market、stop-loss、take-profit、stop-limit 或 take-limit orders。`--on-behalf-of` 是用作 `vaultAddress` 的 acting-account selector。 |
| `orders scale --coin <COIN> --side buy\|sell --start-price <PX> --end-price <PX> --total-size <SIZE> --orders <N>` | 创建一组等间距的 limit orders。 |
| `orders batch-create --orders-file <PATH>` | 从 JSON 创建一批 limit orders。 |
| `orders create --coin <COIN> --side buy\|sell [--take-profit <PX>] [--stop-loss <PX>] [--grouping normal-tpsl] ...` | 创建带固定大小 TP/SL 子订单的父订单。 |
| `orders tpsl --coin <COIN> (--take-profit <PX>\|--stop-loss <PX>) [--grouping position-tpsl] [--side buy\|sell] [--size <SIZE>] [--margin-mode cross\|isolated]` | 创建附加到当前 position 的 TP/SL orders。 |
| `orders cancel (ORDER_ID\|--cloid <CLOID>)` | 按 order ID 或 client order ID 取消。 |
| `orders cancel-all [--coin <COIN>] [--dex <DEX>] [-y]` | 取消所有 open orders，可按 coin 或 DEX 过滤。 |
| `orders modify (ORDER_ID\|--cloid <CLOID>) [--price <PRICE>] [--trigger-price <PRICE>] [--size <SIZE>]` | 修改现有订单。 |
| `orders twap-create --coin <COIN> --side buy\|sell --size <SIZE> --duration <SECONDS> [--dex <DEX>] [--margin-mode cross\|isolated] [-y]` | 创建 TWAP order。 |
| `orders twap-cancel <TWAP_ID> --coin <COIN> [--dex <DEX>]` | 取消 TWAP order。 |
| `orders schedule-cancel (--in <DURATION>\|--clear)` | 配置 dead man's switch。 |
| `positions list [-w] [--max-ticks <TICKS>]` | 列出 open positions。 |
| `positions update-leverage --coin <COIN> --leverage <N> [--isolated]` | 更新 leverage。 |
| `positions update-margin --coin <COIN> --amount <AMOUNT>` | 增加或移除 isolated margin。 |
| `transfer spot-to-perp --amount <USDC> [-y]` | 将 USDC 从 spot 移到 perp。 |
| `transfer perp-to-spot --amount <USDC> [-y]` | 将 USDC 从 perp 移到 spot。 |
| `transfer send --to <ADDRESS> --amount <USDC> [-y]` | 向另一个地址发送 USDC。 |
| `transfer spot-send --to <ADDRESS> --token <TOKEN> --amount <AMOUNT> [-y]` | 向另一个地址发送 spot token。 |
| `transfer send-asset --to <ADDRESS> --source perp\|spot\|dex:<DEX> --dest perp\|spot\|dex:<DEX> --token <TOKEN> --amount <AMOUNT> [--from-subaccount <ADDRESS>] [-y]` | 在账户、spot、perp 或 DEX contexts 之间发送资产。 |
| `transfer withdraw --to <ADDRESS> --amount <USDC> [-y]` | 将 USDC 提现到 Arbitrum。 |
| `subaccount transfer --subaccount <ACCOUNT_SELECTOR> --amount <USDC> --direction deposit\|withdraw [-y]` | 将 USDC 移入或移出 subaccount。subaccount 字段是 acting-account selector，不是通用 transfer recipient。 |
| `subaccount spot-transfer --subaccount <ACCOUNT_SELECTOR> --token <TOKEN> --amount <AMOUNT> --direction deposit\|withdraw [-y]` | 将 spot token 移入或移出 subaccount。subaccount 字段是 acting-account selector，不是通用 transfer recipient。 |

`api-wallets` 可作为 `api-wallet` 的别名。
`subaccounts` 可作为 `subaccount` 的别名。
`transfers` 可作为 `transfer` 的别名。

有时间范围的账户历史命令接受 RFC3339 时间戳和 epoch milliseconds。CLI 会向 Hyperliquid 发送毫秒，例如：

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

Hyperliquid 文档中的历史端点会返回有界窗口，通常根据端点限制在约 500 或 2000 行。导出时，请查询相邻且不重叠的窗口，并保持 `--format json`；只使用全局 `--max-results` 来裁剪本地 CLI 输出以便检查。

Outcome market 表示法（`#N` spot coin 和 `+N` token name）可通过 `outcomes list` 和 `outcomes get` 发现。`orders create --coin '#N' --dry-run` 会发出带有编码 asset id 的稳定 outcome order preview；配置签名者后，live limit-order submission 支持已验证的 outcome notation。在下 live outcome order 之前，先使用 `--dry-run` 检查编码 asset id 和 signed-action preview。

### 高级命令

| 命令 | 描述 |
| --- | --- |
| `staking summary [ADDRESS]` / `staking validators` / `staking rewards [ADDRESS]` / `staking history [ADDRESS]` | 读取 staking state 和 history。 |
| `staking delegate --validator <ADDRESS> --amount <AMOUNT>` / `staking undelegate --validator <ADDRESS> --amount <AMOUNT>` / `staking deposit --amount <AMOUNT>` / `staking withdraw --amount <AMOUNT>` / `staking claim-rewards` | 提交 staking actions。 |
| `staking link initiate --user <ADDRESS>` / `staking link finalize --user <ADDRESS>` | 关联 trading 和 staking accounts 以进行 fee discount attribution。Dry-runs 包含永久性/控制权警告；live commands 需要确认或 `--yes`。 |
| `vault list [--kind protocol\|user\|normal\|child\|parent] [--user <ADDRESS>] [--limit <N>] [--sort tvl\|apr\|age\|name]` / `vault search <QUERY> [--user <ADDRESS>] [--limit <N>] [--sort tvl\|apr\|age\|name]` / `vault get <ADDRESS>` / `vault positions <ADDRESS>` | 发现并查询 vault state。当 API 返回时，`--user` 包含用户存款 context。 |
| `vault deposit --vault <ADDRESS> --amount <AMOUNT>` / `vault withdraw --vault <ADDRESS> --amount <AMOUNT>` | 提交 vault transfers。 |
| `borrowlend rates` / `borrowlend get <TOKEN>` / `borrowlend user [ADDRESS]` | 查询 borrow/lend markets。 |
| `borrowlend supply <TOKEN> --amount <AMOUNT>` / `borrowlend withdraw <TOKEN> (--amount <AMOUNT>\|--max)` | 提交已验证、由钱包签名的 exchange `borrowLend` supply/withdraw actions；先使用 `--dry-run` 检查 action。 |
| `builder max-fee --user <ADDRESS> --builder <ADDRESS>` | 查询用户已批准的 max builder fee。 |
| `builder approved --user <ADDRESS>` | 列出用户批准的所有 builders 及 fee caps。 |
| `builder approve --builder <ADDRESS> --max-fee-rate <PERCENT> [-y]` | 为已配置 master signer 批准或撤销 builder fee cap。 |
| `prio status` / `prio bid --max <HYPE> --ip <IP> [--slot <N>]` | 查询 gossip priority auction 或出价。 |
| `referral register <CODE>` / `referral set [CODE]` / `referral status` | 注册你自己的 referral code、设置 referrer 或检查 referral state。 |
| `feedback (--scenario-json <JSON>\|--scenario-file <PATH\|->) [--contact <CONTACT>] [--tags <TAG>] [--url <URL>]` | 将结构化 CLI feedback 作为 scenario JSON object 发送到已配置的 feedback endpoint；在 scenario 中包含 `agent_address`、`signer_address` 或 `wallet_address` 以用于 rate-limit attribution，并使用 `--url` 覆盖默认值。 |
| `schema [COMMAND...]` | 显示供代理使用的机器可读 command schemas。 |
| `subscribe trades --asset <ASSET>` / `subscribe orderbook --asset <ASSET>` / `subscribe candles --asset <ASSET> [--interval <INTERVAL>]` / `subscribe all-mids` / `subscribe order-updates` / `subscribe fills` `[--max-events <N>] [--idle-timeout-ms <MS>]` | 流式传输 WebSocket events。 |
| `update` | 从最新 GitHub release 更新此二进制文件。使用全局 `--dry-run` 可先预览。 |

`vaults` 可作为 `vault` 的别名。

Builder approvals 使用类似 `0.001%` 的百分数字符串；`0%` 通过把已批准 max fee 设为零来撤销。批准操作必须由 master account 签名，而不是 API wallet。`orders create` 接受成对的 `--builder <ADDRESS> --builder-fee-rate <PERCENT>` 标志，并在已签名 order action 中包含官方 `builder: { b, f }` wire object；perp builder fees 上限为 `0.1%`，spot builder fees 上限为 `1%`。Forks/distributions 可以在构建时内置默认 order builder parameters：

```bash
HYPERLIQUID_DEFAULT_BUILDER_ADDRESS=0x... \
HYPERLIQUID_DEFAULT_BUILDER_FEE_RATE=0.001% \
cargo build --release --bin hyperliquid
```

同名运行时环境变量会覆盖构建时默认值。在 `hyperliquid setup` 期间，构建时默认值会显示为建议的 builder/fee，按 Enter 即接受；存在运行时环境变量时，它们会成为建议值。`HYPERLIQUID_DEFAULT_REFERRAL_CODE` 对 setup 和 `referral set` 默认值遵循相同的构建时/运行时模式。发布工作流会从同名 GitHub Actions repository variables 传递这些值。如果 env 和 config 都没有提供默认值，用户仍然可以为每个订单传入 `--builder` 和 `--builder-fee-rate`。

Vault discovery 可以为 vault detail 和 transfer dry-runs 提供输入，而无需地址重写：

```bash
hyperliquid --format json vault list --kind protocol --limit 5 --sort tvl
hyperliquid --format json vault get 0x...
hyperliquid --format json --dry-run vault deposit --vault 0x... --amount 5
```

完整参考：`hyperliquid --help` 和 `hyperliquid <command> --help`，以及 [`docs/`](docs/) 中的指南。

## Testnet

使用 `--testnet` 在触碰 mainnet 之前演练读取、dry-runs 和已批准的 live testnet flows。Testnet 使用相同的命令界面，但使用 testnet API endpoint 和单独的账户状态。

## 安全模型

CLI 旨在让副作用可见：

- 只读命令永远不会触碰私钥。
- 签名只会通过显式的 `--account`、`--ows-signer`、`--keystore`、`--private-key` 或已存储的 OWS wallets 发生。
- `--testnet` 会清晰地将 API 调用和已签名操作路由到 Hyperliquid testnet。
- `--dry-run` 会验证并预览任何受支持的变更，而不会发送它。
- 当 schema 将其标记为 prompt-gated 时，live mainnet mutations 和破坏性本地 secret 操作需要确认；使用 `hyperliquid --format json schema ...` 查看每个命令的确认策略。支持时，`-y` / `--yes` 可跳过提示。
- 转账接收者和协议对象地址必须是显式 `0x` addresses——local aliases 永远不会被静默替换。

## 退出码

| 代码 | 含义 |
| --- | --- |
| `0` | 成功 |
| `1` | 内部错误 |
| `2` | 用法、验证或配置错误 |
| `10` | 缺失或无效认证 |
| `11` | 受到速率限制 |
| `12` | API 或网络不可用 |
| `13` | 不支持的输入、无效资产或未知 DEX |
| `14` | 过期数据 |
| `15` | 部分结果 |

## 配置

解析顺序：CLI flags → environment variables → `~/.config/hyperliquid/config.json`。

| 变量 | 用途 |
| --- | --- |
| `HYPERLIQUID_PRIVATE_KEY` | 用于签名的 private key（优先使用 OWS 或 keystore）。 |
| `HYPERLIQUID_NETWORK` | `mainnet` 或 `testnet`。 |
| `HYPERLIQUID_FORMAT` | 在 agent/non-TTY fallback 之前显式设置默认输出格式（`pretty`、`table` 或 `json`）。 |
| `HYPERLIQUID_AGENT` | 设置为 `1` 以强制代理默认值。 |
| `HYPERLIQUID_WATCH_MAX_TICKS` | snapshot watch mode 的默认 tick limit。 |
| `HYPERLIQUID_SUBSCRIBE_MAX_EVENTS` | agent contexts 中 WebSocket subscribe commands 的默认 event limit。 |
| `OWS_PASSPHRASE` | 用于解锁加密 OWS wallet 的 passphrase。 |
| `HYPERLIQUID_OWS_VAULT_PATH` | 覆盖 OWS vault path（默认 `~/.hyperliquid`）。 |
| `HYPERLIQUID_API_BASE_URL` / `HYPERLIQUID_MAINNET_API_BASE_URL` / `HYPERLIQUID_TESTNET_API_BASE_URL` | 覆盖 API base URLs；所有 override 都限制为 loopback/local test endpoints。 |
| `HYPERLIQUID_DEFAULT_BUILDER_ADDRESS` / `HYPERLIQUID_DEFAULT_BUILDER_FEE_RATE` | per-order builder fee parameters 和 setup suggestions 的运行时默认值。 |
| `HYPERLIQUID_DEFAULT_REFERRAL_CODE` | setup 和 `referral set` 的运行时默认 referral code。 |
| `HYPERLIQUID_FEEDBACK_URL` | `hyperliquid feedback` 的运行时或构建时默认 endpoint。 |
| `HYPERLIQUID_NO_UPDATE_CHECK` | 为 truthy 时禁用 release 更新检查。 |

## 开发

要为 `hyperliquid feedback` 嵌入默认 endpoint，请在构建环境中设置 `HYPERLIQUID_FEEDBACK_URL`。运行时，`--url` 优先，其次是运行时 `HYPERLIQUID_FEEDBACK_URL`，最后是嵌入的构建时默认值。

```bash
HYPERLIQUID_FEEDBACK_URL="https://<worker-subdomain>/feedback" cargo build --release
```

```bash
cargo build
cargo test
cargo clippy -- -D warnings
cargo fmt --check
```

通过 Taskfile 进行可选的可重复 QA：

```bash
task bind
task qa:matrix
```

约定、测试规则和代理优先输出契约位于 [`AGENTS.md`](AGENTS.md) 和 [`CONTRIBUTING.md`](CONTRIBUTING.md)。

## 当你需要其他东西时

- 构建完整应用或交易系统？请直接使用 [`hypersdk`](https://github.com/infinitefield/hypersdk)。
- 需要长时间运行的策略执行、回测或托管 bots？请选择专用 bot framework。
- 想要深度历史 tick data 或跨交易所研究？请使用 market-data platform。

当你想要一个面向 Hyperliquid 的统一操作界面，并且它对人类、脚本和代理都以相同方式工作时，请使用 `hyperliquid`。

## 致谢

基于 [`hypersdk`](https://github.com/infinitefield/hypersdk) 构建；`hypersdk` 由 Infinite Field 提供，用于 Hyperliquid HTTP、WebSocket 和 EIP-712 签名。`hypersdk` 根据 [Mozilla Public License 2.0](https://www.mozilla.org/en-US/MPL/2.0/) 授权。

## 许可证

MIT — 见 [`LICENSE`](LICENSE)。

### 第三方许可证

`hyperliquid-cli` 是 MIT 授权，但它依赖带有各自许可证的开源 crates：

| 依赖 | 许可证 | 备注 |
| --- | --- | --- |
| [`hypersdk`](https://github.com/infinitefield/hypersdk) | MPL-2.0 | 作为 Cargo dependency 未修改使用。 |
| [`alloy`](https://github.com/alloy-rs/alloy) family | MIT OR Apache-2.0 | EVM primitives 和 signers。 |
| [`tokio`](https://github.com/tokio-rs/tokio) | MIT | Async runtime。 |
| [`clap`](https://github.com/clap-rs/clap) | MIT OR Apache-2.0 | CLI framework。 |
| [`reqwest`](https://github.com/seanmonstar/reqwest) | MIT OR Apache-2.0 | HTTP client。 |
| [`rust_decimal`](https://github.com/paupino/rust-decimal) | MIT | Fixed-point decimal math。 |
| [`ows-lib`](https://crates.io/crates/ows-lib) | See crate metadata | OWS wallet backend。 |

由于 `hypersdk` 是作为未修改的上游 Cargo dependency 使用的，MPL-2.0 的文件级 copyleft 通过公开上游仓库得以满足。如果你 fork 此 CLI 并在树内修改 `hypersdk` 源文件，这些文件必须保持 MPL-2.0，并且其修改后的源码必须可用。`hyperliquid-cli` 的其余部分保持 MIT。

生成完整的传递性许可证报告：

```bash
cargo install cargo-license
cargo license
```

## 免责声明

本软件按“原样”提供，不附带任何形式的保证。在去中心化交易所交易涉及重大亏损风险。你需独自负责你的密钥、已签名操作和交易决策。本项目未与 Hyperliquid 官方关联。
