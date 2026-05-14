use clap::{ArgMatches, Args, CommandFactory, FromArgMatches, Parser, Subcommand};
use hypersdk::{
    Address,
    hypercore::{Chain, HttpClient, Subscription},
};
use std::ffi::OsString;
use std::io::IsTerminal;
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

mod cli_runtime;

static ASSET_METADATA_CACHE: OnceLock<MetadataCache> = OnceLock::new();

#[derive(Parser, Debug)]
#[command(
    name = "hyperliquid",
    version,
    about = "Production-grade CLI for Hyperliquid DEX",
    long_about = "A production-grade, standalone CLI for Hyperliquid DEX.\n\n\
                  Supports 45+ commands for market data, trading, wallet management,\n\
                  watch modes, and agent-first structured output.\n\n\
                  Use --format json for machine-readable output with stable contracts.\n\n\
                  Agent examples:\n  \
                  hyperliquid --format json --select coin,price --max-results 5 mids\n  \
                  hyperliquid --format json schema orders create\n  \
                  hyperliquid --dry-run orders create --coin BTC --side buy --price 1 --size 0.001"
)]
struct Cli {
    /// Output format. Effective default is pretty on a TTY, json for non-TTY stdout or HYPERLIQUID_AGENT=1; explicit --format wins over HYPERLIQUID_FORMAT and dynamic defaults.
    #[arg(long, short, global = true, value_enum, default_value = "pretty")]
    format: output::OutputFormat,

    /// Private key (overrides env var and config file)
    #[arg(long, global = true)]
    private_key: Option<String>,

    /// Foundry-compatible keystore file to decrypt for signing
    #[arg(long, global = true)]
    keystore: Option<PathBuf>,

    /// Password for --keystore (prefer an interactive shell-safe source in production)
    #[arg(long, global = true)]
    keystore_password: Option<String>,

    /// Wallet name, id, or address to use as the signer
    #[arg(
        long,
        global = true,
        conflicts_with_all = ["private_key", "keystore", "keystore_password", "ows_signer"]
    )]
    account: Option<String>,

    /// OWS signer selector (0x address, or wallet name/id from the OWS vault)
    #[arg(
        long,
        global = true,
        value_name = "SELECTOR",
        alias = "wallet",
        conflicts_with_all = ["private_key", "keystore", "keystore_password", "account"]
    )]
    ows_signer: Option<String>,

    /// Use testnet instead of mainnet
    #[arg(long, global = true)]
    testnet: bool,

    /// Field selection for JSON output (comma-separated). Unknown fields are omitted when the output shape is dynamic.
    #[arg(long, global = true)]
    select: Option<String>,

    /// Strip envelope/metadata, return only data
    #[arg(long, global = true)]
    results_only: bool,

    /// Limit top-level JSON/table/pretty result count for agent context control
    #[arg(long, global = true, value_name = "N", value_parser = parse_positive_usize)]
    max_results: Option<usize>,

    /// Validate and preview mutating commands without executing side effects
    #[arg(long, global = true)]
    dry_run: bool,

    /// Disable release update checks for this invocation
    #[arg(long, global = true)]
    no_update_check: bool,

    /// Raw JSON payload for mutating commands
    #[arg(long, global = true, conflicts_with = "payload_file")]
    payload_json: Option<String>,

    /// Raw JSON payload file for mutating commands, or '-' for stdin
    #[arg(long, global = true, conflicts_with = "payload_json")]
    payload_file: Option<String>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Perpetual market queries
    Perps {
        #[command(subcommand)]
        subcommand: PerpsCommands,
    },
    /// Spot market queries
    Spot {
        #[command(subcommand)]
        subcommand: SpotCommands,
    },
    /// L2 order book, candles, spread, funding
    Book {
        /// Asset name (e.g., BTC)
        coin: String,
        #[command(flatten)]
        watch: SnapshotWatchArgs,
    },
    /// All mid prices
    Mids {
        #[command(flatten)]
        watch: SnapshotWatchArgs,
    },
    /// Candle history
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
    /// Bid-ask spread
    Spread {
        /// Asset name (e.g., BTC)
        coin: String,
    },
    /// Current funding rate
    Funding {
        /// Asset name (e.g., BTC)
        coin: String,
    },
    #[command(
        about = "Public account reads and OWS wallet management",
        long_about = "Public account reads and OWS wallet management.\n\n\
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
        visible_alias = "api-wallets",
        about = "Approve and manage Hyperliquid API/agent wallets",
        long_about = "Approve and manage Hyperliquid API/agent wallets.\n\n\
                      API wallets, also called agent wallets, can sign trading actions on behalf \
                      of a master account, but cannot withdraw. Account data queries must use the \
                      master or subaccount address, not the API wallet address."
    )]
    ApiWallet {
        #[command(subcommand)]
        subcommand: ApiWalletCommands,
    },
    /// Subaccount reads and signed subaccount actions
    #[command(visible_alias = "subaccounts")]
    Subaccount {
        #[command(subcommand)]
        subcommand: SubaccountCommands,
    },
    /// API health and rate limit status
    Status,
    /// Raw exchange metadata
    Meta,
    /// Send structured CLI feedback as a scenario JSON object
    Feedback(hyperliquid_cli::commands::feedback::FeedbackArgs),
    /// Order management
    Orders {
        #[command(subcommand)]
        subcommand: OrderCommands,
    },
    /// Position management
    Positions {
        #[command(subcommand)]
        subcommand: PositionCommands,
    },
    /// Transfer funds (spot↔perp, send, withdraw)
    #[command(visible_alias = "transfers")]
    Transfer {
        #[command(subcommand)]
        subcommand: TransferCommands,
    },
    /// Wallet management (create, import, show, address, reset)
    Wallet {
        #[command(subcommand)]
        subcommand: WalletCommands,
    },
    /// Staking operations
    Staking {
        #[command(subcommand)]
        subcommand: StakingCommands,
    },
    /// Vault operations
    #[command(name = "vault", visible_alias = "vaults")]
    Vaults {
        #[command(subcommand)]
        subcommand: VaultCommands,
    },
    /// Borrow/lend operations
    Borrowlend {
        #[command(subcommand)]
        subcommand: BorrowLendCommands,
    },
    /// Builder fee approvals and status
    Builder {
        #[command(subcommand)]
        subcommand: BuilderCommands,
    },
    /// Outcome market discovery
    Outcomes {
        #[command(subcommand)]
        subcommand: OutcomeCommands,
    },
    /// Gossip priority auction
    Prio {
        #[command(subcommand)]
        subcommand: PrioCommands,
    },
    /// Referral system
    Referral {
        #[command(subcommand)]
        subcommand: ReferralCommands,
    },
    #[command(
        about = "Machine-readable command schemas for agents",
        long_about = "Machine-readable command schemas for agents.\n\n\
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
        about = "Guided first-time setup wizard",
        long_about = "Guided first-time setup wizard.\n\n\
                      The wizard can create a new wallet or import an existing private key through \
                      a hidden prompt, choose the default network, save local config, store the \
                      wallet in the OWS vault as the default signing account, and verify the selected \
                      Hyperliquid API connection.\n\n\
                      Generated or imported private keys are never printed."
    )]
    Setup(hyperliquid_cli::commands::setup::SetupArgs),
    #[command(
        about = "WebSocket subscriptions (real-time streaming)",
        long_about = "WebSocket subscriptions (real-time streaming).\n\n\
                      Automation callers should bound streams with --max-events and/or \
                      --idle-timeout-ms so commands return. Global JSON shaping flags such as \
                      --select and --max-results apply to JSONL stream lines."
    )]
    Subscribe {
        #[command(subcommand)]
        subcommand: SubscribeCommands,
    },
    /// Update this binary from the latest GitHub release
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
    /// Fill history for an address or wallet
    Fills(hyperliquid_cli::commands::account::FillsArgs),
    /// Fee schedule and volume context for an address or wallet
    Fees(hyperliquid_cli::commands::account::AddressArgs),
    /// User rate-limit context for an address or wallet
    RateLimit(hyperliquid_cli::commands::account::AddressArgs),
    /// Open orders for an address or wallet
    Orders {
        /// Ethereum address, wallet name, or wallet id.
        /// Defaults to the selected/default signer when omitted.
        address: Option<String>,
    },
    /// Portfolio summary for an address or wallet
    Portfolio {
        /// Ethereum address, wallet name, or wallet id.
        /// Defaults to the selected/default signer when omitted.
        address: Option<String>,
    },
    /// Subaccounts for an address or wallet
    Subaccounts {
        /// Ethereum address, wallet name, or wallet id.
        /// Defaults to the selected/default signer when omitted.
        address: Option<String>,
    },
    /// Frontend portfolio graph/history data for an address or wallet
    PortfolioHistory(hyperliquid_cli::commands::account::AddressArgs),
    /// Non-funding ledger updates including deposits, withdrawals, and transfers
    Ledger(hyperliquid_cli::commands::account::TimeRangeArgs),
    /// User funding payment history
    Funding(hyperliquid_cli::commands::account::TimeRangeArgs),
    /// User TWAP order history
    TwapHistory(hyperliquid_cli::commands::account::AddressArgs),
    /// User TWAP slice fills
    TwapFills(hyperliquid_cli::commands::account::TwapFillsArgs),
    /// User account abstraction mode
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
    /// Set default wallet
    SetDefault {
        /// Wallet name or id. Omit to choose interactively.
        selector: Option<String>,
    },
    /// Remove a wallet
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
    /// Public order status by OID or CLOID
    Status(hyperliquid_cli::commands::orders::StatusArgs),
    /// Order history
    History,
    #[command(
        about = "Create a new order",
        long_about = "Create a new order.\n\n\
                      Risk: funds_movement. Dry-run: supported with --dry-run. \
                      Confirmation: prompts for live mainnet orders unless --yes is passed for deliberate automation."
    )]
    Create(hyperliquid_cli::commands::orders::CreateArgs),
    /// Create a deterministic batch of scaled limit orders
    Scale(hyperliquid_cli::commands::orders::ScaleArgs),
    /// Create a batch of limit orders from a JSON file
    BatchCreate(hyperliquid_cli::commands::orders::BatchCreateArgs),
    /// Create position-attached TP/SL orders
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
    /// Schedule cancel (dead man's switch)
    ScheduleCancel(hyperliquid_cli::commands::orders::ScheduleCancelArgs),
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
    /// Create a new wallet
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
    /// Show current wallet info
    Show,
    /// Print only the wallet address
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
    /// Delete a wallet
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
    /// Reset wallet configuration
    Reset {
        /// Confirm reset without prompting.
        #[arg(long, short = 'y')]
        yes: bool,
    },
}

#[derive(Subcommand, Debug)]
enum StakingCommands {
    /// Staking summary for an address or wallet
    Summary {
        /// Ethereum address, wallet name, or wallet id.
        /// Defaults to the selected/default signer when omitted.
        address: Option<String>,
    },
    /// List validators
    Validators,
    /// Staking rewards for an address or wallet
    Rewards {
        /// Ethereum address, wallet name, or wallet id.
        /// Defaults to the selected/default signer when omitted.
        address: Option<String>,
    },
    /// Staking delegation and withdrawal history for an address or wallet
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
    /// All borrow/lend rates
    Rates,
    /// Single token reserve info
    Get {
        /// Token symbol (e.g., USDC)
        token: String,
    },
    /// User borrow/lend state
    User {
        /// Ethereum address, wallet name, or wallet id.
        /// Defaults to the selected/default signer when omitted.
        address: Option<String>,
    },
    /// Preview borrow/lend supply action shape
    Supply(hyperliquid_cli::commands::borrowlend::ActionArgs),
    /// Preview borrow/lend withdraw action shape
    Withdraw(hyperliquid_cli::commands::borrowlend::ActionArgs),
}

#[derive(Subcommand, Debug)]
enum PrioCommands {
    /// Priority auction status
    Status,
    /// Place a priority bid
    Bid(hyperliquid_cli::commands::prio::BidArgs),
}

#[derive(Subcommand, Debug)]
enum ReferralCommands {
    /// Set referral code
    Set {
        /// Referral code to apply (defaults to TESTNET for testnet verification)
        code: Option<String>,
    },
    /// Register/create your own referral code
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
const ENV_AGENT: &str = "HYPERLIQUID_AGENT";

#[tokio::main]
async fn main() {
    let raw_args = std::env::args().collect::<Vec<_>>();
    let explicit_format = args_request_format(raw_args.iter().map(String::as_str));
    let mut parsed = match try_parse_cli_from(&raw_args) {
        Ok(parsed) => parsed,
        Err(err) => {
            if err.exit_code() == 0 {
                if let Err(print_err) = err.print() {
                    eprintln!("Error: failed to print command error: {print_err}");
                }
                std::process::exit(0);
            }
            if usage_error_should_be_json(raw_args.iter().map(String::as_str)) {
                println!(
                    "{}",
                    errors::ErrorEnvelope::usage(err.to_string()).to_json()
                );
                std::process::exit(2);
            }
            if let Err(print_err) = err.print() {
                eprintln!("Error: failed to print command error: {print_err}");
            }
            std::process::exit(err.exit_code());
        }
    };
    let format =
        match resolve_output_format(&parsed.cli.command, parsed.cli.format, explicit_format) {
            Ok(format) => format,
            Err(err) => errors::exit_with_error(err, output::OutputFormat::Json),
        };
    parsed.cli.format = format;
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

    if let Err(err) = cli_runtime::run(&parsed.cli, parsed.registry_path.as_deref()).await {
        // This path handles CliError and anyhow errors from command handlers.
        // clap usage errors (exit 2) are handled by clap itself before main()
        // is reached, so they don't go through this path.
        hyperliquid_cli::update_check::flush_update_check(update_check_handle).await;
        let cli_err = errors::from_anyhow(err);
        errors::exit_with_error(cli_err, format);
    }

    hyperliquid_cli::update_check::flush_update_check(update_check_handle).await;
}

struct ParsedCli {
    cli: Cli,
    registry_path: Option<Vec<String>>,
}

fn try_parse_cli_from<I, T>(args: I) -> Result<ParsedCli, clap::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let mut command = Cli::command();
    let matches = command.try_get_matches_from_mut(args)?;
    let registry_path = command_registry_path_from_matches(&matches);
    let cli = Cli::from_arg_matches(&matches)?;
    Ok(ParsedCli { cli, registry_path })
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

fn args_request_format<'a>(args: impl IntoIterator<Item = &'a str>) -> bool {
    let mut expect_format_value = false;
    for arg in args {
        if expect_format_value {
            return true;
        }
        if arg == "--format" || arg == "-f" {
            expect_format_value = true;
            continue;
        }
        if arg.starts_with("--format=") || arg.starts_with("-f=") {
            return true;
        }
    }
    false
}

fn usage_error_should_be_json<'a>(args: impl IntoIterator<Item = &'a str>) -> bool {
    let mut expect_format_value = false;
    let mut saw_format_flag = false;
    for arg in args {
        if expect_format_value {
            return usage_error_format_value_should_be_json(arg);
        }
        if arg == "--format" || arg == "-f" {
            saw_format_flag = true;
            expect_format_value = true;
            continue;
        }
        if let Some(value) = arg
            .strip_prefix("--format=")
            .or_else(|| arg.strip_prefix("-f="))
        {
            return usage_error_format_value_should_be_json(value);
        }
    }
    if saw_format_flag {
        return true;
    }
    if let Some(format) = std::env::var_os(ENV_FORMAT) {
        return format.to_string_lossy().trim().eq_ignore_ascii_case("json");
    }
    env_truthy(ENV_AGENT) || !std::io::stdout().is_terminal()
}

fn usage_error_format_value_should_be_json(value: &str) -> bool {
    !matches!(value, "pretty" | "table")
}

fn resolve_output_format(
    _command: &Option<Commands>,
    cli_format: output::OutputFormat,
    explicit_format: bool,
) -> Result<output::OutputFormat, errors::CliError> {
    if explicit_format {
        return Ok(cli_format);
    }
    if let Some(format) = std::env::var_os(ENV_FORMAT) {
        return parse_env_output_format(&format.to_string_lossy());
    }
    if env_truthy(ENV_AGENT) || !std::io::stdout().is_terminal() {
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

fn env_truthy(name: &str) -> bool {
    std::env::var(name).is_ok_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

/// Dispatch commands and return Ok(()) on success or Err on failure.
///
/// clap handles argument parsing and usage errors (exit 2) before
/// this function is called.

#[cfg(test)]
mod tests {
    use super::cli_runtime::{
        command_accepts_mutating_preview, command_contract_for_path, command_uses_api,
    };
    use super::*;
    use clap::Parser;

    #[test]
    fn usage_error_respects_equals_form_human_formats() {
        assert!(!usage_error_should_be_json([
            "hyperliquid",
            "--format=pretty",
            "--bogus"
        ]));
        assert!(!usage_error_should_be_json([
            "hyperliquid",
            "-f=table",
            "--bogus"
        ]));
        assert!(usage_error_should_be_json([
            "hyperliquid",
            "--format=json",
            "--bogus"
        ]));
        assert!(usage_error_should_be_json([
            "hyperliquid",
            "--format=xml",
            "--bogus"
        ]));
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
