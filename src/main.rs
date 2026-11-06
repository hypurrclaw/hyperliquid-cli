use clap::parser::ValueSource;
use clap::{ArgMatches, Args, CommandFactory, FromArgMatches, Parser, Subcommand};
use hypersdk::{
    Address,
    hypercore::{Chain, HttpClient, Subscription},
};
use std::ffi::OsString;
use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;

use hyperliquid_cli::command_context::{
    CommandClients, CommandContext, CommandOutputContext, CommandTransportPolicy, PayloadMetadata,
};
use hyperliquid_cli::command_registry::{
    CommandContract, CommandRegistry, ConfirmationPolicy, DryRunPolicy, Lifecycle,
    RawPayloadPolicy, Risk,
};
use hyperliquid_cli::commands::{AssetResolver, MetadataCache};
use hyperliquid_cli::config;
use hyperliquid_cli::dry_run::{
    ActionPlan, ActionReversibility, DryRunEnvelope, DryRunSigningContext, LiveSubmissionPolicy,
};
use hyperliquid_cli::errors;
use hyperliquid_cli::output;
use hyperliquid_cli::resolvers::{DefaultSignerFallback, SignerResolverInput};
use hyperliquid_cli::watch::{SubscribeEventKind, subscription_event_matches};

#[cfg(test)]
mod cli_contract_tests;
mod cli_runtime;

static ASSET_METADATA_CACHE: OnceLock<MetadataCache> = OnceLock::new();

const GLOBAL_OPTIONS: &str = "Global Options";

const ROOT_AFTER_HELP: &str = "\
Examples:
  hyperliquid --format json --select coin,price --max-results 5 mids
  hyperliquid --format json schema orders create
  hyperliquid --dry-run orders create --coin BTC --side buy --price 1 --size 0.001

Environment:
  HYPERLIQUID_FORMAT                Default output format: pretty, table, or json. --format overrides it.
  HYPERLIQUID_AGENT                 Set to 1 for JSON output and no prompts.
  HYPERLIQUID_NETWORK               Network to use: mainnet or testnet. --testnet overrides it.
  HYPERLIQUID_PRIVATE_KEY           Private key for signing. Prefer it over --private-key.
  HYPERLIQUID_KEYSTORE_PASSWORD     Password for --keystore. Prefer it over --keystore-password.
  HYPERLIQUID_API_BASE_URL          Override the API endpoint. Loopback hosts only, for testing.
  HYPERLIQUID_MAINNET_API_BASE_URL  Same override, mainnet only.
  HYPERLIQUID_TESTNET_API_BASE_URL  Same override, testnet only.
  HYPERLIQUID_OWS_VAULT_PATH        OWS vault directory. Defaults to ~/.hyperliquid.
  OWS_PASSPHRASE                    Passphrase for the OWS vault.
  HYPERLIQUID_NO_UPDATE_CHECK       Set to 1 to skip release update checks.
  HYPERLIQUID_WATCH_MAX_TICKS       Tick bound for --watch when --max-ticks is not given.
  HYPERLIQUID_SUBSCRIBE_MAX_EVENTS  Event bound for subscribe when no bound option is given.
  NO_COLOR                          Disable color when set to a non-empty value.
  FORCE_COLOR                       Force color on, or off when set to 0 or false.
  CI                                Treat the run as non-interactive: no prompts.

Learn more:
  hyperliquid <command> --help
  hyperliquid --format json schema <command>";

#[derive(Parser, Debug)]
#[command(
    name = "hyperliquid",
    version,
    about = "Trade and query Hyperliquid from the command line",
    propagate_version = true,
    disable_help_flag = true,
    disable_version_flag = true,
    after_long_help = ROOT_AFTER_HELP
)]
struct Cli {
    /// Output format: pretty, table, or json. Defaults to json when stdout is not a terminal or HYPERLIQUID_AGENT is set; overrides HYPERLIQUID_FORMAT
    #[arg(long, short, global = true, value_enum, default_value = "pretty", help_heading = GLOBAL_OPTIONS)]
    format: output::OutputFormat,

    /// Private key to sign with. Prefer HYPERLIQUID_PRIVATE_KEY: command-line options appear in process lists
    #[arg(long, global = true, help_heading = GLOBAL_OPTIONS)]
    private_key: Option<String>,

    /// Foundry-compatible keystore file to decrypt for signing
    #[arg(long, global = true, help_heading = GLOBAL_OPTIONS)]
    keystore: Option<PathBuf>,

    /// Password for --keystore. Prefer HYPERLIQUID_KEYSTORE_PASSWORD: command-line options appear in process lists
    #[arg(long, global = true, help_heading = GLOBAL_OPTIONS)]
    keystore_password: Option<String>,

    /// Wallet name, id, or address to use as the signer
    #[arg(
        long,
        global = true,
        help_heading = GLOBAL_OPTIONS,
        conflicts_with_all = ["private_key", "keystore", "keystore_password", "ows_signer"]
    )]
    account: Option<String>,

    /// OWS signer selector (0x address, or wallet name/id from the OWS vault)
    #[arg(
        long,
        global = true,
        value_name = "SELECTOR",
        alias = "wallet",
        help_heading = GLOBAL_OPTIONS,
        conflicts_with_all = ["private_key", "keystore", "keystore_password", "account"]
    )]
    ows_signer: Option<String>,

    /// Use testnet instead of mainnet
    #[arg(long, global = true, help_heading = GLOBAL_OPTIONS)]
    testnet: bool,

    /// Field selection for JSON output (comma-separated). Unknown fields are omitted when the output shape is dynamic
    #[arg(long, global = true, help_heading = GLOBAL_OPTIONS)]
    select: Option<String>,

    /// Strip envelope and metadata, return only data
    #[arg(long, global = true, help_heading = GLOBAL_OPTIONS)]
    results_only: bool,

    /// Limit the top-level result count for JSON, table, and pretty output
    #[arg(long, global = true, value_name = "N", value_parser = parse_positive_usize, help_heading = GLOBAL_OPTIONS)]
    max_results: Option<usize>,

    /// Validate and preview mutating commands without side effects
    #[arg(long, global = true, help_heading = GLOBAL_OPTIONS)]
    dry_run: bool,

    /// Never prompt. Commands fail with the missing input named instead
    #[arg(long, global = true, help_heading = GLOBAL_OPTIONS)]
    non_interactive: bool,

    /// Disable color in output
    #[arg(long, global = true, help_heading = GLOBAL_OPTIONS)]
    no_color: bool,

    /// Disable release update checks for this invocation
    #[arg(long, global = true, help_heading = GLOBAL_OPTIONS)]
    no_update_check: bool,

    /// Raw JSON payload for mutating commands
    #[arg(long, global = true, conflicts_with = "payload_file", help_heading = GLOBAL_OPTIONS)]
    payload_json: Option<String>,

    /// Raw JSON payload file for mutating commands, or '-' for stdin
    #[arg(long, global = true, conflicts_with = "payload_json", help_heading = GLOBAL_OPTIONS)]
    payload_file: Option<String>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Query perpetual markets
    Perps {
        #[command(subcommand)]
        subcommand: PerpsCommands,
    },
    /// Query spot markets
    Spot {
        #[command(subcommand)]
        subcommand: SpotCommands,
    },
    /// Decode and search protocol asset IDs
    Asset {
        #[command(subcommand)]
        subcommand: AssetCommands,
    },
    /// Show the L2 order book
    Book {
        /// Asset name (e.g., BTC)
        coin: String,
        #[command(flatten)]
        watch: SnapshotWatchArgs,
    },
    /// Show mid prices
    Mids {
        #[command(flatten)]
        watch: SnapshotWatchArgs,
    },
    /// Show candle history
    Candles {
        /// Asset name (e.g., BTC)
        coin: String,
        /// Candle interval (1m, 3m, 5m, 15m, 30m, 1h, 2h, 4h, 8h, 12h, 1d, 3d, 1w, 1M)
        #[arg(
            long,
            default_value = "1h",
            value_parser = hyperliquid_cli::commands::orderbook::parse_candle_interval
        )]
        interval: hypersdk::hypercore::CandleInterval,
        /// Number of candles to return
        #[arg(
            long,
            default_value = "100",
            value_parser = hyperliquid_cli::commands::orderbook::parse_candle_limit
        )]
        limit: usize,
        #[command(flatten)]
        watch: SnapshotWatchArgs,
    },
    /// Show the bid-ask spread
    Spread {
        /// Asset name (e.g., BTC)
        coin: String,
    },
    /// Show the current funding rate
    Funding {
        /// Asset name (e.g., BTC)
        coin: String,
    },
    #[command(
        about = "Read account data and manage OWS wallets",
        long_about = "Read account data and manage OWS wallets.\n\n\
                      Public reads (`fills`, `orders`, `portfolio`, `subaccounts`) query any \
                      Ethereum address and do not require a signer. For these commands, wallets \
                      can also be selected by name, id, or address.\n\n\
                      OWS account commands (`add`, `ls`, `set-default`, `remove`) manage wallets \
                      used for authenticated actions.\n\n\
                      Use global --account <WALLET_NAME_OR_ID_OR_ADDRESS> to set the signer \
                      for the current command without changing the default wallet."
    )]
    Account {
        #[command(subcommand)]
        subcommand: AccountCommands,
    },
    #[command(
        name = "api-wallet",
        about = "Approve and manage API wallets",
        long_about = "Approve and manage API wallets.\n\n\
                      API wallets, also called agent wallets, can sign trading actions on behalf \
                      of a master account, but cannot withdraw. Account data queries must use the \
                      master or subaccount address, not the API wallet address."
    )]
    ApiWallet {
        #[command(subcommand)]
        subcommand: ApiWalletCommands,
    },
    /// Manage subaccounts
    Subaccount {
        #[command(subcommand)]
        subcommand: SubaccountCommands,
    },
    /// Show API health and rate limits
    Status,
    /// Show raw exchange metadata
    Meta,
    /// Send structured CLI feedback as a scenario JSON object
    Feedback(hyperliquid_cli::commands::feedback::FeedbackArgs),
    /// Manage orders
    Orders {
        #[command(subcommand)]
        subcommand: OrderCommands,
    },
    /// Manage positions
    Positions {
        #[command(subcommand)]
        subcommand: PositionCommands,
    },
    /// Move funds between spot, perp, and addresses
    Transfer {
        #[command(subcommand)]
        subcommand: TransferCommands,
    },
    /// Manage wallets
    Wallet {
        #[command(subcommand)]
        subcommand: WalletCommands,
    },
    /// Manage staking
    Staking {
        #[command(subcommand)]
        subcommand: StakingCommands,
    },
    /// Query and fund vaults
    #[command(name = "vault")]
    Vaults {
        #[command(subcommand)]
        subcommand: VaultCommands,
    },
    /// Supply, withdraw, and query borrow/lend reserves
    Borrowlend {
        #[command(subcommand)]
        subcommand: BorrowLendCommands,
    },
    /// Manage builder fee approvals
    Builder {
        #[command(subcommand)]
        subcommand: BuilderCommands,
    },
    /// Browse outcome markets
    Outcomes {
        #[command(subcommand)]
        subcommand: OutcomeCommands,
    },
    #[command(
        about = "Search markets across perps, HIP-3, spot, and outcomes",
        long_about = "Search markets across perps, HIP-3, spot, and outcomes.\n\n\
                      Retry: safe.\n\n\
                      Examples:\n  \
                      hyperliquid --format json search GOLD --select venue,coin\n  \
                      hyperliquid --format json search ETH --type perp --select coin"
    )]
    Search(hyperliquid_cli::commands::asset::AssetSearchArgs),
    #[command(
        about = "Buy at market (shortcut for orders create --side buy)",
        long_about = "Buy at market. Shortcut for orders create --side buy. Places a limit order if --price is set.\n\n\
                      Retry: check open orders first. Not idempotent. Inspect `orders open` and `positions list` before rerunning.\n\n\
                      Examples:\n  \
                      hyperliquid --format json --dry-run buy --coin BTC --size 0.001\n  \
                      hyperliquid --format json --dry-run buy --coin BTC --amount 20\n  \
                      hyperliquid --format json --dry-run buy --coin ETH --price 3000 --size 0.1\n  \
                      hyperliquid --format json buy --coin BTC --size 0.001 --yes"
    )]
    Buy(hyperliquid_cli::commands::orders::BuySellArgs),
    #[command(
        about = "Sell at market (shortcut for orders create --side sell)",
        long_about = "Sell at market. Shortcut for orders create --side sell. Places a limit order if --price is set.\n\n\
                      Retry: check open orders first. Not idempotent. Inspect `orders open` and `positions list` before rerunning.\n\n\
                      Examples:\n  \
                      hyperliquid --format json --dry-run sell --coin BTC --size 0.001\n  \
                      hyperliquid --format json --dry-run sell --coin ETH --price 3000 --size 0.1\n  \
                      hyperliquid --format json sell --coin BTC --size 0.001 --yes"
    )]
    Sell(hyperliquid_cli::commands::orders::BuySellArgs),
    #[command(
        about = "Buy a HIP-4 outcome token",
        long_about = "Buy a HIP-4 outcome token. --coin must be #N or +N notation.\n\n\
                      Retry: check open orders first. Not idempotent.\n\n\
                      Examples:\n  \
                      hyperliquid --format json --dry-run outcome-buy --coin #10 --size 1\n  \
                      hyperliquid --format json outcome-buy --coin #10 --size 1 --yes"
    )]
    OutcomeBuy(hyperliquid_cli::commands::orders::OutcomeOrderArgs),
    #[command(
        about = "Sell a HIP-4 outcome token",
        long_about = "Sell a HIP-4 outcome token. --coin must be #N or +N notation.\n\n\
                      Retry: check open orders first. Not idempotent.\n\n\
                      Examples:\n  \
                      hyperliquid --format json --dry-run outcome-sell --coin #10 --size 1\n  \
                      hyperliquid --format json outcome-sell --coin #10 --size 1 --yes"
    )]
    OutcomeSell(hyperliquid_cli::commands::orders::OutcomeOrderArgs),
    /// Watch accounts for risk alerts
    Watch {
        #[command(subcommand)]
        subcommand: WatchCommands,
    },
    /// Manage gossip priority bids
    Prio {
        #[command(subcommand)]
        subcommand: PrioCommands,
    },
    /// Manage referral codes
    Referral {
        #[command(subcommand)]
        subcommand: ReferralCommands,
    },
    #[command(
        about = "Show machine-readable command schemas",
        long_about = "Show machine-readable command schemas.\n\n\
                      Examples:\n  \
                      hyperliquid --format json schema\n  \
                      hyperliquid --format json schema orders create\n  \
                      hyperliquid --format json --select command,description schema orders"
    )]
    Schema {
        /// Optional command path, for example: orders create
        command_path: Vec<String>,
    },
    #[command(
        about = "Set up a wallet, network, and defaults",
        long_about = "Set up a wallet, network, and defaults.\n\n\
                      The wizard can create a new wallet or import an existing private key through \
                      a hidden prompt, choose the default network, save local config, store the \
                      wallet in the OWS vault as the default signing account, and verify the selected \
                      Hyperliquid API connection.\n\n\
                      Generated or imported private keys are never printed."
    )]
    Setup(hyperliquid_cli::commands::setup::SetupArgs),
    #[command(
        about = "Stream real-time market and account events",
        long_about = "Stream real-time market and account events over WebSocket.\n\n\
                      Automation callers should bound streams with --max-events and/or \
                      --idle-timeout-ms so commands return. Global JSON shaping flags such as \
                      --select and --max-results apply to JSONL stream lines."
    )]
    Subscribe {
        #[command(subcommand)]
        subcommand: SubscribeCommands,
    },
    /// Update this binary to the latest GitHub release
    Update,
}

#[derive(Subcommand, Debug)]
enum PerpsCommands {
    /// List all perpetual markets
    List(hyperliquid_cli::commands::perps::PerpsListArgs),
    /// Get details for a specific perpetual market
    Get(hyperliquid_cli::commands::perps::PerpsGetArgs),
}

#[derive(Subcommand, Debug)]
enum SpotCommands {
    /// List all spot markets
    List,
    /// Get details for a specific spot pair
    Get {
        /// Spot pair (e.g., PURR/USDC, HYPE/USDC)
        pair: String,
    },
}

#[derive(Subcommand, Debug)]
enum AssetCommands {
    /// Decode a raw protocol asset ID
    Decode(hyperliquid_cli::commands::asset::AssetDecodeArgs),
    /// Search assets by symbol, market title, slug, notation, or protocol ID
    Search(hyperliquid_cli::commands::asset::AssetSearchArgs),
}

#[derive(Subcommand, Debug)]
enum OutcomeCommands {
    /// List active outcome market sides
    List(hyperliquid_cli::commands::outcomes::OutcomeListArgs),
    /// Get outcome side metadata by #N or +N notation
    Get(hyperliquid_cli::commands::outcomes::OutcomeGetArgs),
}

#[derive(Subcommand, Debug)]
enum BuilderCommands {
    /// Query max builder fee approved by a user
    MaxFee(hyperliquid_cli::commands::builder::MaxFeeArgs),
    /// List all builders approved by a user
    Approved(hyperliquid_cli::commands::builder::ApprovedArgs),
    /// Approve or update a max builder fee for the selected signer
    Approve(hyperliquid_cli::commands::builder::ApproveArgs),
}

#[derive(Subcommand, Debug)]
enum AccountCommands {
    /// Show fill history for an address or wallet
    Fills(hyperliquid_cli::commands::account::FillsArgs),
    /// Show the fee schedule and volume context for an address or wallet
    Fees(hyperliquid_cli::commands::account::AddressArgs),
    /// Show rate-limit context for an address or wallet
    RateLimit(hyperliquid_cli::commands::account::AddressArgs),
    /// List open orders for an address or wallet
    Orders {
        /// Ethereum address, wallet name, or wallet id.
        /// Defaults to the selected/default signer when omitted.
        address: Option<String>,
    },
    /// Show the portfolio summary for an address or wallet
    Portfolio {
        /// Ethereum address, wallet name, or wallet id.
        /// Defaults to the selected/default signer when omitted.
        address: Option<String>,
    },
    /// List subaccounts for an address or wallet
    Subaccounts {
        /// Ethereum address, wallet name, or wallet id.
        /// Defaults to the selected/default signer when omitted.
        address: Option<String>,
    },
    /// Show portfolio history for an address or wallet
    PortfolioHistory(hyperliquid_cli::commands::account::AddressArgs),
    /// Show non-funding ledger updates: deposits, withdrawals, and transfers
    Ledger(hyperliquid_cli::commands::account::TimeRangeArgs),
    /// Show funding payment history
    Funding(hyperliquid_cli::commands::account::TimeRangeArgs),
    /// Show TWAP order history
    TwapHistory(hyperliquid_cli::commands::account::AddressArgs),
    /// Show TWAP slice fills
    TwapFills(hyperliquid_cli::commands::account::TwapFillsArgs),
    /// Show or set the account abstraction mode
    Abstraction(hyperliquid_cli::commands::account::AbstractionArgs),
    #[command(
        about = "Add an OWS wallet",
        long_about = "Add an OWS wallet.\n\n\
                      Recommended for humans: run `hyperliquid account add` without PRIVATE_KEY \
                      and paste the key at the hidden prompt.\n\n\
                      For controlled automation, pass PRIVATE_KEY with --alias, --type, and \
                      optionally --default. Passing a private key as an argument can expose it in \
                      OS process listings and shell history."
    )]
    Add {
        /// Private key to store (0x-prefixed hex). Omit to use the hidden prompt.
        private_key: Option<String>,
        /// Alias for the wallet. Defaults to the derived address in non-interactive mode.
        #[arg(long)]
        alias: Option<String>,
        /// Wallet type.
        #[arg(long = "type")]
        account_type: Option<String>,
        /// Make this wallet the default signer.
        #[arg(long)]
        default: bool,
    },
    /// List wallets
    Ls,
    /// Set the default wallet
    SetDefault {
        /// Wallet name or id. Omit to choose interactively.
        selector: Option<String>,
    },
    /// Delete a wallet and its stored key
    Remove {
        /// Wallet name or id. Omit to choose interactively.
        selector: Option<String>,
        /// Confirm removal without prompting.
        #[arg(long, short = 'y')]
        yes: bool,
    },
}

#[derive(Subcommand, Debug)]
enum ApiWalletCommands {
    #[command(
        about = "Generate or approve an API wallet",
        long_about = "Generate or approve an API wallet.\n\n\
                      By default this command generates a new local API wallet keypair, signs an \
                      approveAgent action for that address with the selected master account, and \
                      prints the generated private key exactly once unless command is running in dry-run."
    )]
    Create(hyperliquid_cli::commands::api_wallet::CreateArgs),
    /// Approve an existing or generated API wallet address
    Approve(hyperliquid_cli::commands::api_wallet::ApproveArgs),
    /// List API wallets approved by a master account
    List {
        /// Master address, wallet name, or wallet id. Defaults to the selected signer.
        account: Option<String>,
    },
    /// Revoke an API wallet by replacing it with a short-lived throwaway agent
    Revoke(hyperliquid_cli::commands::api_wallet::RevokeArgs),
}

#[derive(Subcommand, Debug)]
enum SubaccountCommands {
    /// List subaccounts for an address or wallet
    List {
        /// Master address, wallet name, or wallet id.
        /// Defaults to the selected/default signer when omitted.
        address: Option<String>,
    },
    /// Create a new subaccount signed by the master account
    Create(hyperliquid_cli::commands::subaccounts::CreateArgs),
    /// Transfer USDC between the master account and a subaccount
    Transfer(hyperliquid_cli::commands::subaccounts::TransferArgs),
    /// Transfer a spot token between the master account and a subaccount
    SpotTransfer(hyperliquid_cli::commands::subaccounts::SpotTransferArgs),
}

#[derive(Subcommand, Debug)]
enum OrderCommands {
    /// List open orders
    Open {
        #[command(flatten)]
        watch: SnapshotWatchArgs,
    },
    /// Show an order's status by OID or CLOID
    Status(hyperliquid_cli::commands::orders::StatusArgs),
    /// Show order history
    History,
    #[command(
        about = "Create a new order",
        long_about = "Create a new order.\n\n\
                      Examples:\n  \
                      hyperliquid --dry-run orders create --coin BTC --side buy --price 60000 --size 0.001\n  \
                      hyperliquid orders create --coin BTC --side buy --price 60000 --size 0.001 --yes"
    )]
    Create(hyperliquid_cli::commands::orders::CreateArgs),
    /// Create a deterministic batch of scaled limit orders
    Scale(hyperliquid_cli::commands::orders::ScaleArgs),
    /// Create a batch of limit orders from a JSON file
    BatchCreate(hyperliquid_cli::commands::orders::BatchCreateArgs),
    #[command(
        about = "Create position-attached TP/SL orders",
        long_about = "Create position-attached take-profit and/or stop-loss orders.\n\n\
                      Triggers accept an absolute price, a percent offset (`+10%`, `-5%`), or `entry`.\n\n\
                      Retry: check open orders first. A second tpsl can duplicate working triggers.\n\n\
                      Examples:\n  \
                      hyperliquid --format json --dry-run orders tpsl --coin ETH --take-profit +10% --stop-loss -5%\n  \
                      hyperliquid --format json --dry-run orders tpsl --coin ETH --stop-loss entry\n  \
                      hyperliquid --format json orders tpsl --coin ETH --take-profit +10% --stop-loss -5% --yes"
    )]
    Tpsl(hyperliquid_cli::commands::orders::TpslArgs),
    /// Cancel an order by ID
    Cancel(hyperliquid_cli::commands::orders::CancelArgs),
    /// Cancel all open orders
    CancelAll(hyperliquid_cli::commands::orders::CancelAllArgs),
    /// Modify an existing order
    Modify(hyperliquid_cli::commands::orders::ModifyArgs),
    /// Create a TWAP order
    TwapCreate(hyperliquid_cli::commands::orders::TwapCreateArgs),
    /// Cancel a TWAP order
    TwapCancel(hyperliquid_cli::commands::orders::TwapCancelArgs),
    /// Schedule a cancel-all (dead man's switch)
    ScheduleCancel(hyperliquid_cli::commands::orders::ScheduleCancelArgs),
    #[command(
        about = "Chase mid with ALO requotes",
        long_about = "Bounded ALO requote loop that chases mid until fill, timeout, or max chase distance.\n\n\
                      Agent/non-TTY mode requires --timeout. Retry: check open orders first. Not idempotent.\n\n\
                      Examples:\n  \
                      hyperliquid --format json --dry-run orders chase --coin ETH --side buy --size 0.5 --timeout 60s\n  \
                      hyperliquid --format json orders chase --coin ETH --side buy --size 0.5 --timeout 60s --yes"
    )]
    Chase(hyperliquid_cli::commands::orders::ChaseArgs),
    #[command(
        about = "Create an entry order with linked TP/SL",
        long_about = "Place an entry order, then arm position TP/SL. Limit entries wait for fill up to --entry-timeout.\n\n\
                      Retry: check open orders first. Not idempotent. On limit-entry timeout the remaining entry is cancelled and any filled portion is protected.\n\n\
                      Examples:\n  \
                      hyperliquid --format json --dry-run orders bracket --coin ETH --side buy --size 0.1 --take-profit +10% --stop-loss -5%\n  \
                      hyperliquid --format json --dry-run orders bracket --coin ETH --side buy --entry limit --price 3000 --size 0.1 --take-profit +10% --stop-loss -5%\n  \
                      hyperliquid --format json orders bracket --coin ETH --side buy --size 0.1 --take-profit +10% --stop-loss -5% --yes"
    )]
    Bracket(hyperliquid_cli::commands::orders::BracketArgs),
}

#[derive(Subcommand, Debug)]
enum WatchCommands {
    #[command(
        about = "Watch an address for liquidation and risk alerts",
        long_about = "Read-only liquidation and risk watcher. Emits JSON-line alerts and never places orders.\n\n\
                      Retry: safe. Agent/non-TTY mode requires --max-events or --idle-timeout-ms.\n\n\
                      Examples:\n  \
                      hyperliquid --format json watch risk --user 0x0000000000000000000000000000000000000001 --max-events 20\n  \
                      hyperliquid --format json watch risk --user 0x0000000000000000000000000000000000000001 --idle-timeout-ms 8000"
    )]
    Risk(hyperliquid_cli::commands::watch_risk::WatchRiskArgs),
}

#[derive(Subcommand, Debug)]
enum PositionCommands {
    /// List open positions
    List {
        #[command(flatten)]
        watch: SnapshotWatchArgs,
    },
    /// Update leverage for a position
    UpdateLeverage(hyperliquid_cli::commands::positions::UpdateLeverageArgs),
    /// Update margin for a position
    UpdateMargin(hyperliquid_cli::commands::positions::UpdateMarginArgs),
}

#[derive(Subcommand, Debug)]
enum TransferCommands {
    /// Transfer from spot to perp
    SpotToPerp(hyperliquid_cli::commands::transfers::ClassTransferArgs),
    /// Transfer from perp to spot
    PerpToSpot(hyperliquid_cli::commands::transfers::ClassTransferArgs),
    /// Send USDC to an address
    Send(hyperliquid_cli::commands::transfers::SendArgs),
    /// Send a spot token to an address
    SpotSend(hyperliquid_cli::commands::transfers::SpotSendArgs),
    /// Send an asset between spot, perp, or DEX contexts
    SendAsset(hyperliquid_cli::commands::transfers::SendAssetArgs),
    /// Withdraw to Arbitrum
    Withdraw(hyperliquid_cli::commands::transfers::WithdrawArgs),
}

#[derive(Subcommand, Debug)]
enum WalletCommands {
    /// Create a wallet
    Create,
    #[command(
        about = "Import an existing private key",
        long_about = "Import an existing private key.\n\n\
                      Recommended for humans: run `hyperliquid wallet import` without PRIVATE_KEY \
                      and paste the key at the hidden prompt.\n\n\
                      Warning: `hyperliquid wallet import <PRIVATE_KEY>` can expose the key in OS \
                      process listings and shell history. Use the argument form only for controlled \
                      automation."
    )]
    Import {
        /// Private key to import (0x-prefixed hex). Omit to use the hidden prompt.
        private_key: Option<String>,
    },
    /// Import an existing wallet from a BIP-39 mnemonic phrase
    #[command(
        about = "Import an existing wallet from a BIP-39 mnemonic phrase",
        long_about = "Import an existing wallet from a BIP-39 mnemonic phrase.\n\n\
                      Recommended for humans: run `hyperliquid wallet import-mnemonic` without MNEMONIC \
                      and paste the phrase at the hidden prompt.\n\n\
                      Warning: passing a mnemonic as an argument can expose it in OS \
                      process listings and shell history. Use the argument form only for controlled \
                      automation."
    )]
    ImportMnemonic {
        /// BIP-39 mnemonic phrase. Omit to use the hidden prompt.
        mnemonic: Option<String>,
        /// Alias/name for the imported wallet.
        #[arg(long)]
        alias: Option<String>,
    },
    /// Show the current wallet
    Show,
    /// Print the wallet address
    Address,
    /// List all wallets
    List,
    /// Rename a wallet
    Rename {
        /// Wallet name or id to rename.
        selector: String,
        /// New name for the wallet.
        new_name: String,
    },
    /// Delete a wallet and its stored key
    Delete {
        /// Wallet name or id to delete.
        selector: String,
        /// Confirm deletion without prompting.
        #[arg(long, short = 'y')]
        yes: bool,
    },
    /// Export a wallet's secret (mnemonic or private key)
    Export {
        /// Wallet name or id to export.
        selector: String,
        /// Skip confirmation prompt.
        #[arg(long, short = 'y')]
        yes: bool,
    },
    /// Delete the wallet config file and default wallet reference
    Reset {
        /// Confirm reset without prompting.
        #[arg(long, short = 'y')]
        yes: bool,
    },
}

#[derive(Subcommand, Debug)]
enum StakingCommands {
    /// Show the staking summary for an address or wallet
    Summary {
        /// Ethereum address, wallet name, or wallet id.
        /// Defaults to the selected/default signer when omitted.
        address: Option<String>,
    },
    /// List validators
    Validators,
    /// Show staking rewards for an address or wallet
    Rewards {
        /// Ethereum address, wallet name, or wallet id.
        /// Defaults to the selected/default signer when omitted.
        address: Option<String>,
    },
    /// Show staking delegation and withdrawal history for an address or wallet
    History {
        /// Ethereum address, wallet name, or wallet id.
        /// Defaults to the selected/default signer when omitted.
        address: Option<String>,
    },
    /// Delegate stake to a validator
    Delegate(hyperliquid_cli::commands::staking::DelegateArgs),
    /// Undelegate stake from a validator
    Undelegate(hyperliquid_cli::commands::staking::DelegateArgs),
    /// Deposit to staking
    Deposit(hyperliquid_cli::commands::staking::AmountArgs),
    /// Withdraw from staking
    Withdraw(hyperliquid_cli::commands::staking::AmountArgs),
    /// Claim staking rewards
    ClaimRewards,
    /// Link trading and staking accounts for fee discount attribution
    Link {
        #[command(subcommand)]
        subcommand: StakingLinkCommands,
    },
}

#[derive(Subcommand, Debug)]
enum StakingLinkCommands {
    /// Initiate staking-link from the trading account, targeting the staking user
    Initiate(hyperliquid_cli::commands::staking::LinkArgs),
    /// Finalize staking-link from the staking account, targeting the trading user
    Finalize(hyperliquid_cli::commands::staking::LinkArgs),
}

#[derive(Subcommand, Debug)]
enum VaultCommands {
    /// List vault summaries
    List(hyperliquid_cli::commands::vaults::VaultListArgs),
    /// Search vault summaries by name, leader, or address
    Search(hyperliquid_cli::commands::vaults::VaultSearchArgs),
    /// Get vault details
    Get {
        /// Vault address
        address: String,
    },
    /// List vault positions
    Positions {
        /// Vault address
        address: String,
    },
    /// Deposit to a vault
    Deposit(hyperliquid_cli::commands::vaults::VaultTransferArgs),
    /// Withdraw from a vault
    Withdraw(hyperliquid_cli::commands::vaults::VaultTransferArgs),
}

#[derive(Subcommand, Debug)]
enum BorrowLendCommands {
    /// Show borrow and lend rates
    Rates,
    /// Show a token reserve
    Get {
        /// Token symbol (e.g., USDC)
        token: String,
    },
    /// Show borrow and lend state for an address or wallet
    User {
        /// Ethereum address, wallet name, or wallet id.
        /// Defaults to the selected/default signer when omitted.
        address: Option<String>,
    },
    /// Supply a token to a borrow/lend reserve
    Supply(hyperliquid_cli::commands::borrowlend::ActionArgs),
    /// Withdraw a token from a borrow/lend reserve
    Withdraw(hyperliquid_cli::commands::borrowlend::ActionArgs),
}

#[derive(Subcommand, Debug)]
enum PrioCommands {
    /// Show priority auction status
    Status,
    /// Place a priority bid
    Bid(hyperliquid_cli::commands::prio::BidArgs),
}

#[derive(Subcommand, Debug)]
enum ReferralCommands {
    /// Set a referral code
    Set {
        /// Referral code to apply (defaults to TESTNET for testnet verification)
        code: Option<String>,
    },
    /// Register your own referral code
    Register {
        /// Referral code to create. Hyperliquid currently caps referrer codes at 20 characters.
        code: String,
    },
    /// Show referral status
    Status,
}

#[derive(Subcommand, Debug)]
enum SubscribeCommands {
    /// Stream real-time trades
    Trades {
        /// Asset name (e.g., BTC)
        #[arg(long)]
        asset: String,
        #[command(flatten)]
        stream: SubscribeStreamArgs,
    },
    /// Stream L2 order book updates
    Orderbook {
        /// Asset name (e.g., BTC)
        #[arg(long)]
        asset: String,
        #[command(flatten)]
        stream: SubscribeStreamArgs,
    },
    /// Stream candle updates
    Candles {
        /// Asset name (e.g., BTC)
        #[arg(long)]
        asset: String,
        /// Candle interval
        #[arg(long, default_value = "1m")]
        interval: String,
        #[command(flatten)]
        stream: SubscribeStreamArgs,
    },
    /// Stream all mid price updates
    AllMids {
        #[command(flatten)]
        stream: SubscribeStreamArgs,
    },
    /// Stream order status updates (authenticated)
    OrderUpdates {
        #[command(flatten)]
        stream: SubscribeStreamArgs,
    },
    /// Stream fill events (authenticated)
    Fills {
        #[command(flatten)]
        stream: SubscribeStreamArgs,
    },
}

#[derive(Args, Debug, Clone, Default)]
struct SnapshotWatchArgs {
    /// Watch live updates in-place
    #[arg(short = 'w', long)]
    watch: bool,
    /// Stop after rendering this many watch snapshots (useful for automation)
    #[arg(long, value_name = "TICKS", requires = "watch", value_parser = parse_positive_usize)]
    max_ticks: Option<usize>,
}

#[derive(Args, Debug, Clone, Default)]
struct SubscribeStreamArgs {
    /// Stop after emitting this many matching events (useful for automation)
    #[arg(long)]
    max_events: Option<usize>,
    /// Stop if no matching events are emitted within this many milliseconds
    #[arg(long, value_name = "MILLISECONDS", value_parser = clap::value_parser!(u64).range(1..))]
    idle_timeout_ms: Option<u64>,
}

impl SubscribeStreamArgs {
    fn idle_timeout(&self) -> Option<Duration> {
        self.idle_timeout_ms.map(Duration::from_millis)
    }
}

fn parse_positive_usize(raw: &str) -> Result<usize, String> {
    let value = raw
        .parse::<usize>()
        .map_err(|err| format!("invalid positive integer: {err}"))?;
    if value == 0 {
        return Err("value must be at least 1".to_string());
    }
    Ok(value)
}

const ENV_FORMAT: &str = "HYPERLIQUID_FORMAT";
const REPORT_URL: &str = "https://github.com/hypurrclaw/hyperliquid-cli/issues/new";

/// Top-level commands in help order: market data, trading, accounts and
/// wallets, funds, streaming, then tools.
const ROOT_ORDER: &[&str] = &[
    "perps",
    "spot",
    "asset",
    "outcomes",
    "book",
    "mids",
    "candles",
    "spread",
    "funding",
    "search",
    "status",
    "meta",
    "orders",
    "buy",
    "sell",
    "outcome-buy",
    "outcome-sell",
    "positions",
    "account",
    "wallet",
    "api-wallet",
    "subaccount",
    "builder",
    "referral",
    "staking",
    "vault",
    "borrowlend",
    "prio",
    "transfer",
    "subscribe",
    "watch",
    "schema",
    "setup",
    "feedback",
    "update",
];

/// Hidden alternate spellings for groups. A singular name behaves like its plural.
const GROUP_ALIASES: &[(&str, &str)] = &[
    ("perps", "perp"),
    ("asset", "assets"),
    ("outcomes", "outcome"),
    ("orders", "order"),
    ("positions", "position"),
    ("transfer", "transfers"),
    ("wallet", "wallets"),
    ("account", "accounts"),
    ("subaccount", "subaccounts"),
    ("api-wallet", "api-wallets"),
    ("vault", "vaults"),
    ("builder", "builders"),
    ("referral", "referrals"),
];

static PANIC_COMMAND: OnceLock<String> = OnceLock::new();

/// What the command line said about `--yes` for the matched command.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Confirmation {
    /// The command has a `--yes` option.
    pub supported: bool,
    /// `--yes` was passed.
    pub given: bool,
}

#[tokio::main]
async fn main() {
    install_panic_hook();
    let raw_args: Vec<OsString> = std::env::args_os().collect();
    let mut parsed = match try_parse_cli_from(&raw_args) {
        Ok(parsed) => parsed,
        Err(err) => exit_for_clap_error(err, &raw_args),
    };
    output::colors::init(parsed.cli.no_color);
    if let Some(path) = parsed.registry_path.as_deref() {
        let _ = PANIC_COMMAND.set(path.join(" "));
    }
    let format = match resolve_output_format(parsed.explicit_format, parsed.cli.format) {
        Ok(format) => format,
        Err(err) => errors::exit_with_error(err, output::OutputFormat::Json),
    };
    parsed.cli.format = format;
    if let Err(err) = hyperliquid_cli::mode::validate_env() {
        errors::exit_with_error(err, format);
    }
    hyperliquid_cli::mode::init(hyperliquid_cli::mode::ModeInputs {
        non_interactive_flag: parsed.cli.non_interactive,
        machine_format: format == output::OutputFormat::Json,
        stdin_is_payload: parsed.cli.payload_file.as_deref() == Some("-"),
    });
    output::set_json_options_with_limit(
        parsed.cli.select.as_deref(),
        parsed.cli.results_only,
        parsed.cli.max_results,
    );
    let is_update_command = matches!(&parsed.cli.command, Some(Commands::Update));
    let update_check_handle = hyperliquid_cli::update_check::maybe_start_update_check(
        format,
        parsed.cli.no_update_check || is_update_command,
    );

    if let Err(err) = cli_runtime::run(
        &parsed.cli,
        parsed.registry_path.as_deref(),
        parsed.confirmation,
    )
    .await
    {
        // This path handles CliError and anyhow errors from command handlers.
        // clap usage errors (exit 2) are handled by exit_for_clap_error before
        // the command runs, so they don't go through this path.
        hyperliquid_cli::update_check::flush_update_check(update_check_handle).await;
        let cli_err = errors::from_anyhow(err);
        errors::exit_with_error(cli_err, format);
    }

    hyperliquid_cli::update_check::flush_update_check(update_check_handle).await;
}

struct ParsedCli {
    cli: Cli,
    registry_path: Option<Vec<String>>,
    /// `--format` was given on the command line (not env or a default).
    explicit_format: bool,
    confirmation: Confirmation,
}

fn try_parse_cli_from<I, T>(args: I) -> Result<ParsedCli, clap::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let mut command = build_command();
    let matches = command.try_get_matches_from_mut(args)?;
    let registry_path = command_registry_path_from_matches(&matches);
    let explicit_format = matches.value_source("format") == Some(ValueSource::CommandLine);
    let confirmation = confirmation_from_matches(&matches);
    let cli = Cli::from_arg_matches(&matches)?;
    Ok(ParsedCli {
        cli,
        registry_path,
        explicit_format,
        confirmation,
    })
}

fn command_registry_path_from_matches(matches: &ArgMatches) -> Option<Vec<String>> {
    let mut path = Vec::new();
    let mut current = matches;
    while let Some((name, subcommand)) = current.subcommand() {
        path.push(name.to_string());
        current = subcommand;
    }
    (!path.is_empty()).then_some(path)
}

/// Read `--yes` from the matched command itself, so the answer can't drift
/// from the options the command actually defines.
fn confirmation_from_matches(matches: &ArgMatches) -> Confirmation {
    let mut leaf = matches;
    while let Some((_, subcommand)) = leaf.subcommand() {
        leaf = subcommand;
    }
    match leaf.try_get_one::<bool>("yes") {
        Ok(Some(given)) => Confirmation {
            supported: true,
            given: *given,
        },
        _ => Confirmation::default(),
    }
}

// ── Command tree ────────────────────────────────────────────────────────

/// The clap command tree with the pieces derive can't express: one help style
/// at every depth, `-v`, task-ordered top-level commands, aliases, and help
/// text filled from the command catalog.
fn build_command() -> clap::Command {
    let registry = CommandRegistry::load().ok();
    let mut command = Cli::command()
        .arg(
            clap::Arg::new("help")
                .short('h')
                .long("help")
                .global(true)
                .action(clap::ArgAction::HelpLong)
                .help("Print help")
                .help_heading(GLOBAL_OPTIONS),
        )
        .arg(
            clap::Arg::new("version")
                .short('v')
                .short_alias('V')
                .long("version")
                .global(true)
                .action(clap::ArgAction::Version)
                .help("Print version")
                .help_heading(GLOBAL_OPTIONS),
        );

    let names: Vec<String> = command
        .get_subcommands()
        .map(|sub| sub.get_name().to_string())
        .collect();
    for name in names {
        let order = ROOT_ORDER
            .iter()
            .position(|candidate| *candidate == name)
            .unwrap_or(ROOT_ORDER.len());
        let aliases: Vec<&'static str> = GROUP_ALIASES
            .iter()
            .filter(|(group, _)| *group == name)
            .map(|(_, alias)| *alias)
            .collect();
        let path = vec![name.clone()];
        command = command.mut_subcommand(&name, |sub| {
            let mut sub = sub.display_order(order);
            for alias in aliases {
                sub = sub.alias(alias);
            }
            decorate_command(sub, &path, registry.as_ref())
        });
    }
    let shared_help: Vec<(String, String)> = command
        .find_subcommand("orders")
        .and_then(|orders| orders.find_subcommand("create"))
        .map(|create| {
            create
                .get_arguments()
                .filter_map(|arg| {
                    arg.get_help()
                        .map(|help| (arg.get_id().to_string(), help.to_string()))
                })
                .collect()
        })
        .unwrap_or_default();
    inherit_option_help(command, &shared_help)
}

/// Order options shared by `buy`, `sell`, `orders chase`, and others have the
/// same meaning everywhere. Give each the help text `orders create` defines
/// when the catalog and the command itself have none.
fn inherit_option_help(mut command: clap::Command, shared: &[(String, String)]) -> clap::Command {
    let undocumented: Vec<String> = command
        .get_arguments()
        .filter(|arg| arg.get_help().is_none() && arg.get_long_help().is_none())
        .map(|arg| arg.get_id().to_string())
        .collect();
    for id in undocumented {
        if let Some((_, help)) = shared.iter().find(|(shared_id, _)| *shared_id == id) {
            let help = help.clone();
            command = command.mut_arg(&id, |arg| arg.help(help));
        }
    }
    let children: Vec<String> = command
        .get_subcommands()
        .map(|sub| sub.get_name().to_string())
        .collect();
    for child in children {
        command = command.mut_subcommand(&child, |sub| inherit_option_help(sub, shared));
    }
    command
}

/// Add `ls`/`list` aliases, fill missing option help from the catalog, and
/// append catalog-derived notes to commands that change state.
fn decorate_command(
    mut command: clap::Command,
    path: &[String],
    registry: Option<&CommandRegistry>,
) -> clap::Command {
    let children: Vec<String> = command
        .get_subcommands()
        .map(|sub| sub.get_name().to_string())
        .collect();
    for child in children {
        let mut child_path = path.to_vec();
        child_path.push(child.clone());
        command =
            command.mut_subcommand(&child, |sub| decorate_command(sub, &child_path, registry));
    }

    match command.get_name() {
        "list" => command = command.alias("ls"),
        "ls" => command = command.alias("list"),
        _ => {}
    }

    let path_refs: Vec<&str> = path.iter().map(String::as_str).collect();
    let Some(contract) = registry.and_then(|registry| registry.find_path(&path_refs)) else {
        return command;
    };

    let undocumented: Vec<String> = command
        .get_arguments()
        .filter(|arg| arg.get_help().is_none() && arg.get_long_help().is_none())
        .map(|arg| arg.get_id().to_string())
        .collect();
    for id in undocumented {
        let description = contract
            .inputs
            .iter()
            .find(|input| input.id == id)
            .and_then(|input| input.description.as_deref());
        if let Some(description) = description {
            let help = description.trim_end_matches('.').to_string();
            command = command.mut_arg(&id, |arg| arg.help(help));
        }
    }

    let has_yes = command.get_arguments().any(|arg| arg.get_id() == "yes");
    if let Some(notes) = command_notes(contract, has_yes) {
        let combined = match command.get_after_long_help() {
            Some(existing) => format!("{existing}\n\n{notes}"),
            None => notes,
        };
        command = command.after_long_help(combined);
    }
    command
}

/// Whether a command changes state, locally or on the exchange.
fn command_changes_state(contract: &CommandContract) -> bool {
    matches!(
        contract.lifecycle,
        Lifecycle::LiveMutating | Lifecycle::InteractiveLocal
    ) || matches!(contract.risk, Risk::LocalSecret | Risk::LocalState)
}

/// `Notes:` block for commands that change state: risk, dry-run support, and
/// how confirmation behaves.
fn command_notes(contract: &CommandContract, has_yes: bool) -> Option<String> {
    if !command_changes_state(contract) {
        return None;
    }
    let risk = match contract.risk {
        Risk::FundsMovement => "moves funds or changes live exchange state",
        Risk::AccountState => "changes account settings on the exchange",
        Risk::LocalSecret => "reads or writes secret key material on this machine",
        Risk::LocalState => "changes local configuration or wallet state",
        Risk::None => "changes live exchange state",
    };
    let mut lines = vec!["Notes:".to_string(), format!("  Risk: {risk}.")];
    lines.push(match contract.dry_run {
        DryRunPolicy::Optional => {
            "  Dry-run: supported. Add --dry-run to preview without side effects.".to_string()
        }
        DryRunPolicy::NotSupported => "  Dry-run: not supported.".to_string(),
    });
    lines.push(match (contract.confirmation, has_yes) {
        (ConfirmationPolicy::Prompt, true) => "  Confirmation: asks before it runs. Pass --yes to answer the prompt. Runs that can't prompt (JSON output, HYPERLIQUID_AGENT, CI, --non-interactive) fail without --yes. Use --yes there only after a human approves.".to_string(),
        (ConfirmationPolicy::Prompt, false) => "  Confirmation: asks before it runs. Runs that can't prompt (JSON output, HYPERLIQUID_AGENT, CI, --non-interactive) fail instead.".to_string(),
        (ConfirmationPolicy::None, _) => {
            "  Confirmation: none. It runs as soon as you invoke it.".to_string()
        }
    });
    if cli_runtime::TESTNET_UNPROMPTED_COMMANDS.contains(&contract.command_key().as_str()) {
        lines.push(
            "  Testnet: with --testnet this command doesn't prompt and runs as soon as you invoke it."
                .to_string(),
        );
    }
    Some(lines.join("\n"))
}

// ── Help, usage errors, and exit paths ──────────────────────────────────

fn styled_text(styled: &clap::builder::StyledStr, color: bool) -> String {
    if color {
        styled.ansi().to_string()
    } else {
        styled.to_string()
    }
}

/// Write to stdout. A closed pipe (`| head`) is a normal end, not an error.
fn write_stdout(text: &str) -> std::io::Result<()> {
    let mut out = std::io::stdout().lock();
    match out.write_all(text.as_bytes()).and_then(|()| out.flush()) {
        Err(err) if err.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
        other => other,
    }
}

fn exit_after_stdout(text: &str) -> ! {
    let code = match write_stdout(text) {
        Ok(()) => 0,
        Err(err) => {
            let _ = writeln!(std::io::stderr(), "Failed to write to stdout: {err}");
            1
        }
    };
    std::process::exit(code)
}

/// The deepest command along `path` that exists.
fn find_command<'a>(root: &'a clap::Command, path: &[String]) -> &'a clap::Command {
    let mut command = root;
    for name in path {
        match command.find_subcommand(name) {
            Some(sub) => command = sub,
            None => break,
        }
    }
    command
}

fn help_text_for_path(path: &[String]) -> String {
    let mut root = build_command();
    root.build();
    let mut target = find_command(&root, path).clone();
    styled_text(&target.render_long_help(), output::colors::stdout_enabled())
}

/// Print the long help for `path` (the root when empty) on stdout.
fn print_help_to_stdout(path: &[String]) -> Result<(), anyhow::Error> {
    write_stdout(&help_text_for_path(path))
        .map_err(|err| anyhow::anyhow!("failed to write help to stdout: {err}"))
}

fn help_command(path: &[String]) -> String {
    if path.is_empty() {
        "hyperliquid --help".to_string()
    } else {
        format!("hyperliquid {} --help", path.join(" "))
    }
}

fn is_help_or_version_token(arg: &OsString) -> bool {
    matches!(
        arg.to_str(),
        Some("-h" | "--help" | "-v" | "-V" | "--version")
    )
}

/// Parse as much of the command line as possible, skipping what clap rejects.
///
/// Used only after a real parse failed, to learn which command was reached and
/// which global options were given, so errors can be rendered in the right
/// format and point at the right help page.
fn lenient_matches(raw_args: &[OsString]) -> Option<ArgMatches> {
    let mut args: Vec<OsString> = raw_args
        .iter()
        .filter(|arg| !is_help_or_version_token(arg))
        .cloned()
        .collect();
    let mut command = build_command().ignore_errors(true);
    for _ in 0..8 {
        match command.try_get_matches_from_mut(&args) {
            Ok(matches) => return Some(matches),
            Err(err) => {
                let mut indexes = offending_arg_indexes(&err, &args)?;
                indexes.sort_unstable_by(|a, b| b.cmp(a));
                for index in indexes {
                    args.remove(index);
                }
            }
        }
    }
    None
}

/// Positions in `args` of the token clap rejected (and its value, if any).
fn offending_arg_indexes(err: &clap::Error, args: &[OsString]) -> Option<Vec<usize>> {
    use clap::error::{ContextKind, ContextValue, ErrorKind};

    let context_string = |kind: ContextKind| match err.get(kind) {
        Some(ContextValue::String(value)) => Some(value.clone()),
        _ => None,
    };
    let (name, has_value) = match err.kind() {
        ErrorKind::UnknownArgument => (context_string(ContextKind::InvalidArg)?, false),
        ErrorKind::InvalidSubcommand => (context_string(ContextKind::InvalidSubcommand)?, false),
        ErrorKind::InvalidValue | ErrorKind::ValueValidation => {
            (context_string(ContextKind::InvalidArg)?, true)
        }
        _ => return None,
    };
    let name = name.split([' ', '=']).next()?.to_string();
    let with_equals = format!("{name}=");
    let index = args.iter().position(|arg| {
        arg.to_str()
            .is_some_and(|arg| arg == name || arg.starts_with(&with_equals))
    })?;
    let mut indexes = vec![index];
    let attached = args[index].to_str().is_some_and(|arg| arg.contains('='));
    if has_value && !attached && index + 1 < args.len() {
        indexes.push(index + 1);
    }
    Some(indexes)
}

/// Output format for a failed parse: an explicit `--format` wins, then
/// `HYPERLIQUID_FORMAT`, then JSON for agents and non-terminal stdout.
fn error_format(matches: Option<&ArgMatches>) -> output::OutputFormat {
    if let Some(matches) = matches
        && matches.value_source("format") == Some(ValueSource::CommandLine)
        && let Some(format) = matches.get_one::<output::OutputFormat>("format")
    {
        return *format;
    }
    match env_output_format() {
        Some(Ok(format)) => return format,
        Some(Err(_)) => return output::OutputFormat::Json,
        None => {}
    }
    if hyperliquid_cli::mode::agent_env() || !std::io::stdout().is_terminal() {
        output::OutputFormat::Json
    } else {
        output::OutputFormat::Pretty
    }
}

/// Keep only clap's best suggestion. clap lists the closest match last.
fn narrow_suggestions(err: &mut clap::Error) {
    use clap::error::{ContextKind, ContextValue};

    for kind in [
        ContextKind::SuggestedSubcommand,
        ContextKind::SuggestedArg,
        ContextKind::SuggestedValue,
    ] {
        let best = match err.get(kind) {
            Some(ContextValue::Strings(values)) if values.len() > 1 => values.last().cloned(),
            _ => None,
        };
        if let Some(best) = best {
            err.insert(kind, ContextValue::Strings(vec![best]));
        }
    }
}

/// Render a usage error: one suggestion, the usage line of the command that
/// was actually reached, and a pointer to its help.
fn usage_error_text(mut err: clap::Error, path: &[String], color: bool) -> String {
    use clap::error::{ContextKind, ContextValue};

    narrow_suggestions(&mut err);
    let mut root = build_command();
    root.build();
    if err.get(ContextKind::Usage).is_some() {
        let usage = find_command(&root, path).clone().render_usage();
        err.insert(ContextKind::Usage, ContextValue::StyledStr(usage));
    }
    let text = styled_text(&err.render(), color);
    let body = match text.rfind("\n\nFor more information") {
        Some(index) => &text[..index],
        None => text.trim_end(),
    };
    format!("{body}\n\nSee all options:\n    {}\n", help_command(path))
}

/// Print what clap reported and exit. Help and version are successes on
/// stdout; everything else is a usage error (exit 2).
fn exit_for_clap_error(err: clap::Error, raw_args: &[OsString]) -> ! {
    use clap::error::ErrorKind;

    let lenient = lenient_matches(raw_args);
    output::colors::init(
        lenient
            .as_ref()
            .is_some_and(|matches| matches.get_flag("no_color")),
    );
    let path = lenient
        .as_ref()
        .and_then(command_registry_path_from_matches)
        .unwrap_or_default();
    match err.kind() {
        ErrorKind::DisplayHelp => exit_after_stdout(&styled_text(
            &err.render(),
            output::colors::stdout_enabled(),
        )),
        ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand => {
            exit_after_stdout(&help_text_for_path(&path))
        }
        ErrorKind::DisplayVersion => {
            let version = env!("CARGO_PKG_VERSION");
            let text = if error_format(lenient.as_ref()) == output::OutputFormat::Json {
                format!("{}\n", serde_json::json!({ "version": version }))
            } else {
                format!("hyperliquid {version}\n")
            };
            exit_after_stdout(&text)
        }
        _ => {
            let exit_code = err.exit_code();
            if error_format(lenient.as_ref()) == output::OutputFormat::Json {
                let text = usage_error_text(err, &path, false);
                exit_after_stdout_with_code(
                    &format!(
                        "{}\n",
                        errors::ErrorEnvelope::usage(text.trim_end()).to_json()
                    ),
                    exit_code,
                );
            }
            let text = usage_error_text(err, &path, output::colors::stderr_enabled());
            let _ = write!(std::io::stderr(), "{text}");
            std::process::exit(exit_code)
        }
    }
}

fn exit_after_stdout_with_code(text: &str, code: i32) -> ! {
    let _ = write_stdout(text);
    std::process::exit(code)
}

// ── Panics ──────────────────────────────────────────────────────────────

/// A panic from `println!`/`eprintln!` on a closed pipe.
fn is_broken_pipe_panic(message: &str) -> bool {
    message.contains("Broken pipe")
        && (message.contains("failed printing to stdout")
            || message.contains("failed printing to stderr"))
}

/// A closed pipe ends the process quietly. Any other panic is a bug: say so
/// with the command path (never argument values) and exit 1.
fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let payload = info.payload();
        let message = payload
            .downcast_ref::<&str>()
            .map(|message| (*message).to_string())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_default();
        if is_broken_pipe_panic(&message) {
            std::process::exit(0);
        }
        let command = match PANIC_COMMAND.get() {
            Some(path) if !path.is_empty() => format!("hyperliquid {path}"),
            _ => "hyperliquid".to_string(),
        };
        let _ = write!(
            std::io::stderr(),
            "Unexpected error in {command}. This is a bug in hyperliquid-cli.\nVersion: {}\nReport it at:\n    {REPORT_URL}\n",
            env!("CARGO_PKG_VERSION"),
        );
        std::process::exit(1);
    }));
}

// ── Output format resolution ────────────────────────────────────────────

/// `HYPERLIQUID_FORMAT`, when set to a non-empty value.
fn env_output_format() -> Option<Result<output::OutputFormat, errors::CliError>> {
    let raw = std::env::var(ENV_FORMAT).ok()?;
    if raw.trim().is_empty() {
        return None;
    }
    Some(parse_env_output_format(&raw))
}

/// An explicit `--format` wins over everything. Otherwise `HYPERLIQUID_FORMAT`,
/// then JSON for agents and non-terminal stdout, then the option's default.
fn resolve_output_format(
    explicit_format: bool,
    cli_format: output::OutputFormat,
) -> Result<output::OutputFormat, errors::CliError> {
    if explicit_format {
        return Ok(cli_format);
    }
    if let Some(format) = env_output_format() {
        return format;
    }
    if hyperliquid_cli::mode::agent_env() || !std::io::stdout().is_terminal() {
        return Ok(output::OutputFormat::Json);
    }
    Ok(cli_format)
}

fn parse_env_output_format(raw: &str) -> Result<output::OutputFormat, errors::CliError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "pretty" => Ok(output::OutputFormat::Pretty),
        "table" => Ok(output::OutputFormat::Table),
        "json" => Ok(output::OutputFormat::Json),
        other => Err(errors::CliError::Configuration(format!(
            "{ENV_FORMAT} must be one of pretty, table, or json; got '{other}'"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::cli_runtime::{
        command_accepts_mutating_preview, command_contract_for_path, command_uses_api,
    };
    use super::*;

    /// Split an example line into words, honoring single and double quotes.
    fn split_command_line(line: &str) -> Vec<String> {
        let mut words = Vec::new();
        let mut current = String::new();
        let mut quote: Option<char> = None;
        let mut started = false;
        for ch in line.chars() {
            match (quote, ch) {
                (Some(open), c) if c == open => quote = None,
                (Some(_), c) => current.push(c),
                (None, '\'' | '"') => {
                    quote = Some(ch);
                    started = true;
                }
                (None, c) if c.is_whitespace() => {
                    if started || !current.is_empty() {
                        words.push(std::mem::take(&mut current));
                        started = false;
                    }
                }
                (None, c) => current.push(c),
            }
        }
        if started || !current.is_empty() {
            words.push(current);
        }
        words
    }

    fn collect_examples(command: &clap::Command, out: &mut Vec<String>) {
        for text in [
            command.get_about(),
            command.get_long_about(),
            command.get_after_help(),
            command.get_after_long_help(),
        ]
        .into_iter()
        .flatten()
        {
            let text = text.to_string();
            let mut in_examples = false;
            for line in text.lines() {
                let line = line.trim();
                if line == "Examples:" {
                    in_examples = true;
                } else if in_examples {
                    if line.starts_with("hyperliquid ") {
                        out.push(line.to_string());
                    } else {
                        in_examples = false;
                    }
                }
            }
        }
        for sub in command.get_subcommands() {
            collect_examples(sub, out);
        }
    }

    #[test]
    fn every_help_example_parses() {
        let mut examples = Vec::new();
        collect_examples(&build_command(), &mut examples);
        assert!(
            examples.len() >= 25,
            "expected help examples, found {}",
            examples.len()
        );
        for example in examples {
            let words = split_command_line(&example);
            if let Err(err) = try_parse_cli_from(&words) {
                panic!("help example doesn't parse: {example}\n{err}");
            }
        }
    }

    fn visit_commands(
        command: &clap::Command,
        path: &mut Vec<String>,
        f: &mut dyn FnMut(&clap::Command, &[String]),
    ) {
        f(command, path);
        for sub in command.get_subcommands() {
            path.push(sub.get_name().to_string());
            visit_commands(sub, path, f);
            path.pop();
        }
    }

    #[test]
    fn every_state_changing_command_documents_its_risk() {
        let registry = CommandRegistry::load().unwrap();
        let root = build_command();
        let mut checked = 0;
        visit_commands(&root, &mut Vec::new(), &mut |command, path| {
            let refs: Vec<&str> = path.iter().map(String::as_str).collect();
            let Some(contract) = registry.find_path(&refs) else {
                return;
            };
            if !command_changes_state(contract) {
                return;
            }
            checked += 1;
            let help = command
                .get_after_long_help()
                .map(|help| help.to_string())
                .unwrap_or_default();
            assert!(
                help.contains("Notes:") && help.contains("Risk:") && help.contains("Dry-run:"),
                "{} is missing notes",
                path.join(" ")
            );
        });
        assert!(
            checked >= 40,
            "only {checked} state-changing commands checked"
        );
    }

    #[test]
    fn orders_create_notes_state_that_testnet_does_not_prompt() {
        let root = build_command();
        let create = find_command(&root, &["orders".to_string(), "create".to_string()]);
        let help = create.get_after_long_help().unwrap().to_string();
        assert!(
            help.contains("with --testnet this command doesn't prompt"),
            "{help}"
        );
        assert!(help.contains("Pass --yes"), "{help}");
    }

    #[test]
    fn every_option_has_help_text() {
        let root = build_command();
        let mut missing = Vec::new();
        visit_commands(&root, &mut Vec::new(), &mut |command, path| {
            for arg in command.get_arguments() {
                if arg.is_hide_set() {
                    continue;
                }
                if arg.get_help().is_none() && arg.get_long_help().is_none() {
                    missing.push(format!("{} {}", path.join(" "), arg.get_id()));
                }
            }
        });
        assert!(missing.is_empty(), "options without help: {missing:?}");
    }

    #[test]
    fn root_commands_are_all_ordered() {
        let root = build_command();
        for sub in root.get_subcommands() {
            assert!(
                ROOT_ORDER.contains(&sub.get_name()),
                "{} is missing from ROOT_ORDER",
                sub.get_name()
            );
        }
    }

    #[test]
    fn explicit_format_is_read_from_parsed_matches() {
        for (argv, explicit, format) in [
            (
                &["hyperliquid", "-fjson", "status"][..],
                true,
                output::OutputFormat::Json,
            ),
            (
                &["hyperliquid", "-ftable", "status"][..],
                true,
                output::OutputFormat::Table,
            ),
            (
                &["hyperliquid", "status", "--format=pretty"][..],
                true,
                output::OutputFormat::Pretty,
            ),
            (
                &["hyperliquid", "status"][..],
                false,
                output::OutputFormat::Pretty,
            ),
        ] {
            let parsed = try_parse_cli_from(argv.iter().copied()).unwrap();
            assert_eq!(parsed.explicit_format, explicit, "{argv:?}");
            assert_eq!(parsed.cli.format, format, "{argv:?}");
        }
    }

    #[test]
    fn failed_parse_still_reads_explicit_format() {
        let args = |items: &[&str]| items.iter().map(OsString::from).collect::<Vec<_>>();
        let lenient = lenient_matches(&args(&["hyperliquid", "--format=pretty", "--bogus"]));
        assert_eq!(error_format(lenient.as_ref()), output::OutputFormat::Pretty);
        let lenient = lenient_matches(&args(&["hyperliquid", "-f=table", "--bogus"]));
        assert_eq!(error_format(lenient.as_ref()), output::OutputFormat::Table);
        let lenient = lenient_matches(&args(&["hyperliquid", "-fjson", "--bogus"]));
        assert_eq!(error_format(lenient.as_ref()), output::OutputFormat::Json);
        let lenient = lenient_matches(&args(&["hyperliquid", "--format=json", "--bogus"]));
        assert_eq!(error_format(lenient.as_ref()), output::OutputFormat::Json);
    }

    #[test]
    fn lenient_parse_finds_the_command_that_was_reached() {
        let args = ["hyperliquid", "orders", "crete"].map(OsString::from);
        let lenient = lenient_matches(&args).unwrap();
        assert_eq!(
            command_registry_path_from_matches(&lenient),
            Some(vec!["orders".to_string()])
        );
    }

    #[test]
    fn unknown_subcommand_shows_one_suggestion_and_its_group_help() {
        let err = try_parse_cli_from(["hyperliquid", "--no-update-check", "orders", "cancl"])
            .err()
            .unwrap();
        let text = usage_error_text(err, &["orders".to_string()], false);
        assert_eq!(text.matches("tip:").count(), 1, "{text}");
        assert!(text.contains("'cancel'"), "{text}");
        assert!(!text.contains("--no-update-check"), "{text}");
        assert!(text.contains("Usage: hyperliquid orders"), "{text}");
        assert!(
            text.trim_end()
                .ends_with("See all options:\n    hyperliquid orders --help"),
            "{text}"
        );
        assert!(!text.contains("For more information"), "{text}");
    }

    #[test]
    fn usage_line_shows_the_command_path_not_consumed_globals() {
        let err =
            try_parse_cli_from(["hyperliquid", "--no-update-check", "--form", "json", "mids"])
                .err()
                .unwrap();
        let text = usage_error_text(err, &[], false);
        assert!(!text.contains("--no-update-check"), "{text}");
        assert!(text.contains("Usage: hyperliquid [OPTIONS]"), "{text}");
        assert!(text.trim_end().ends_with("hyperliquid --help"), "{text}");
    }

    #[test]
    fn short_and_long_help_are_identical() {
        let render = |flag: &str| {
            try_parse_cli_from(["hyperliquid", "orders", "create", flag])
                .err()
                .unwrap()
                .render()
                .to_string()
        };
        assert_eq!(render("-h"), render("--help"));
    }

    #[test]
    fn version_flags_work_at_every_depth() {
        use clap::error::ErrorKind;
        for argv in [
            &["hyperliquid", "-v"][..],
            &["hyperliquid", "-V"][..],
            &["hyperliquid", "--version"][..],
            &["hyperliquid", "orders", "--version"][..],
            &["hyperliquid", "orders", "create", "--version"][..],
        ] {
            let err = try_parse_cli_from(argv.iter().copied()).err().unwrap();
            assert_eq!(err.kind(), ErrorKind::DisplayVersion, "{argv:?}");
        }
    }

    #[test]
    fn group_without_action_asks_for_help() {
        use clap::error::ErrorKind;
        let err = try_parse_cli_from(["hyperliquid", "orders"]).err().unwrap();
        assert_eq!(
            err.kind(),
            ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
        );
    }

    #[test]
    fn list_has_ls_alias_and_ls_has_list_alias() {
        for argv in [
            &["hyperliquid", "perps", "ls"][..],
            &["hyperliquid", "positions", "ls"][..],
            &["hyperliquid", "account", "list"][..],
            &["hyperliquid", "account", "ls"][..],
            &["hyperliquid", "order", "open"][..],
            &["hyperliquid", "position", "list"][..],
            &["hyperliquid", "perp", "list"][..],
            &["hyperliquid", "outcome", "list"][..],
            &["hyperliquid", "wallets", "list"][..],
            &["hyperliquid", "accounts", "ls"][..],
        ] {
            try_parse_cli_from(argv.iter().copied())
                .unwrap_or_else(|err| panic!("{argv:?} should parse: {err}"));
        }
    }

    #[test]
    fn confirmation_comes_from_the_matched_command() {
        let parse = |argv: &[&str]| {
            try_parse_cli_from(argv.iter().copied())
                .unwrap()
                .confirmation
        };
        let with_yes = parse(&["hyperliquid", "wallet", "export", "main", "--yes"]);
        assert!(with_yes.supported && with_yes.given);
        let without = parse(&["hyperliquid", "wallet", "export", "main"]);
        assert!(without.supported && !without.given);
        let none = parse(&["hyperliquid", "wallet", "show"]);
        assert!(!none.supported && !none.given);
        let setup = parse(&["hyperliquid", "setup", "--yes"]);
        assert!(setup.supported && setup.given);
    }

    #[test]
    fn broken_pipe_panics_are_recognized() {
        assert!(is_broken_pipe_panic(
            "failed printing to stdout: Broken pipe (os error 32)"
        ));
        assert!(is_broken_pipe_panic(
            "failed printing to stderr: Broken pipe (os error 32)"
        ));
        assert!(!is_broken_pipe_panic("index out of bounds"));
        assert!(!is_broken_pipe_panic(
            "failed printing to stdout: Interrupted"
        ));
    }

    #[test]
    fn root_help_lists_environment_and_no_hype() {
        let help = help_text_for_path(&[]);
        assert!(help.contains("Environment:"), "{help}");
        for name in [
            "HYPERLIQUID_FORMAT",
            "HYPERLIQUID_KEYSTORE_PASSWORD",
            "OWS_PASSPHRASE",
            "CI",
        ] {
            assert!(help.contains(name), "{name} missing from root help");
        }
        assert!(!help.to_lowercase().contains("production-grade"), "{help}");
        assert!(help.contains("Global Options:"), "{help}");
    }

    #[test]
    fn cli_command_paths_resolve_to_registry_contracts() {
        for (argv, expected_path) in [
            (
                &["hyperliquid", "api-wallet", "list"][..],
                "api-wallet list",
            ),
            (&["hyperliquid", "orders", "history"][..], "orders history"),
            (
                &["hyperliquid", "orders", "cancel", "123"][..],
                "orders cancel",
            ),
            (&["hyperliquid", "wallet", "show"][..], "wallet show"),
            (
                &[
                    "hyperliquid",
                    "feedback",
                    "--scenario-json",
                    "{\"command\":\"mids\",\"actual\":\"ok\"}",
                ][..],
                "feedback",
            ),
            (
                &["hyperliquid", "subscribe", "fills"][..],
                "subscribe fills",
            ),
        ] {
            let parsed = try_parse_cli_from(argv.iter().copied()).unwrap();
            let contract = command_contract_for_path(parsed.registry_path.as_deref())
                .unwrap()
                .unwrap_or_else(|| panic!("missing registry contract for {argv:?}"));

            assert_eq!(contract.command_key(), expected_path);
        }
    }

    #[test]
    fn cli_policy_uses_registry_mutating_preview_and_api_contracts() {
        let api_wallet_list = command_contract_for_path(
            try_parse_cli_from(["hyperliquid", "api-wallet", "list"])
                .unwrap()
                .registry_path
                .as_deref(),
        )
        .unwrap();
        assert!(!command_accepts_mutating_preview(api_wallet_list.as_ref()));
        assert!(command_uses_api(api_wallet_list.as_ref()));

        let orders_cancel = command_contract_for_path(
            try_parse_cli_from(["hyperliquid", "orders", "cancel", "123"])
                .unwrap()
                .registry_path
                .as_deref(),
        )
        .unwrap();
        assert!(command_accepts_mutating_preview(orders_cancel.as_ref()));
        assert!(command_uses_api(orders_cancel.as_ref()));

        let wallet_show = command_contract_for_path(
            try_parse_cli_from(["hyperliquid", "wallet", "show"])
                .unwrap()
                .registry_path
                .as_deref(),
        )
        .unwrap();
        assert!(!command_accepts_mutating_preview(wallet_show.as_ref()));
        assert!(!command_uses_api(wallet_show.as_ref()));
    }
}
