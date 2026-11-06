use clap::{ArgMatches, Args, CommandFactory, FromArgMatches, Parser, Subcommand};
use hypersdk::{
    Address,
    hypercore::{Chain, HttpClient, Subscription},
};
use std::ffi::OsString;
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
mod cli_shell;

use cli_shell::{
    Confirmation, PANIC_COMMAND, exit_for_clap_error, install_panic_hook, print_help_to_stdout,
    resolve_output_format, try_parse_cli_from,
};

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
