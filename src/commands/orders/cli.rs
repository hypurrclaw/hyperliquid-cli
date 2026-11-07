//! The `orders ...` commands, `buy`, `sell`, `outcome-buy` and `outcome-sell`.

use clap::Args;
use hypersdk::hypercore::Subscription;

use super::*;
use crate::assets::{AssetQuery, AssetResolver, Market, parse_asset_query, qualify_dex_asset};
use crate::command_spec::{
    CliCommand, CommandSpec, GroupSpec, InputKind, NoArgs, OwsSupport, Risk, StreamBound, arg,
};
use crate::commands::support::*;
use crate::errors::CliError;
use crate::input_hardening::{FilePolicy, validate_file_path, validate_resource_id};
use crate::invocation::Invocation;
use crate::network::Network;
use crate::stream::SnapshotWatchArgs;

pub const GROUP: GroupSpec = GroupSpec::new(&["orders"], "Manage orders").aliases(&["order"]);

#[derive(Args, Debug, Clone)]
pub struct OpenOrdersArgs {
    #[command(flatten)]
    pub watch: SnapshotWatchArgs,
}

pub struct OrdersOpen;

impl CliCommand for OrdersOpen {
    type Args = OpenOrdersArgs;

    const SPEC: CommandSpec = CommandSpec::read(
        &["orders", "open"],
        "account",
        "List open orders",
        "List open orders for the configured signer.",
    )
    .risk(Risk::AccountState)
    .ows(OwsSupport::ExperimentalFeatureGated)
    .auth(true)
    .stream(StreamBound::SnapshotWatch)
    .args(&[
        arg("watch")
            .kind(InputKind::Boolean)
            .describe("Watch live snapshots. Use max_ticks to bound automation calls."),
        arg("max_ticks").describe("Stop after this many watch snapshots."),
    ]);

    async fn run(inv: &Invocation, args: &OpenOrdersArgs) -> Result<(), anyhow::Error> {
        if args.watch.watch {
            watch_open_orders(inv, args.watch.max_ticks).await
        } else {
            let resolver = inv.asset_resolver().await?;
            let user = inv.resolve_signer()?.query_address();
            open(inv, &resolver, user).await
        }
    }
}

pub struct OrdersStatus;

impl CliCommand for OrdersStatus {
    type Args = StatusArgs;

    const SPEC: CommandSpec = CommandSpec::read(
        &["orders", "status"],
        "account",
        "Show an order's status by OID or CLOID",
        "Query public order status by OID or CLOID.",
    )
    .one_of_required(&[&["oid"], &["cloid"]])
    .args(&[
        arg("user")
            .kind(InputKind::PublicUserSelector)
            .describe("Ethereum address, wallet name, or wallet id.")
            .resource_id("account selector"),
        arg("oid")
            .kind(InputKind::Identifier)
            .describe("Exchange-assigned order id."),
        arg("cloid")
            .kind(InputKind::Identifier)
            .describe("Client order id.")
            .resource_id("cloid"),
    ]);

    async fn run(inv: &Invocation, args: &StatusArgs) -> Result<(), anyhow::Error> {
        let user = inv.resolve_protocol_user(&args.user)?;
        status(inv, user, args).await
    }
}

pub struct OrdersHistory;

impl CliCommand for OrdersHistory {
    type Args = NoArgs;

    const SPEC: CommandSpec = CommandSpec::read(
        &["orders", "history"],
        "account",
        "Show order history",
        "List order history for the configured signer.",
    )
    .risk(Risk::AccountState)
    .ows(OwsSupport::ExperimentalFeatureGated)
    .auth(true);

    async fn run(inv: &Invocation, _args: &NoArgs) -> Result<(), anyhow::Error> {
        let resolver = inv.asset_resolver().await?;
        let user = inv.resolve_signer()?.query_address();
        history(inv, &resolver, user).await
    }
}

pub struct OrdersCreate;

impl CliCommand for OrdersCreate {
    type Args = CreateArgs;

    const SPEC: CommandSpec = CommandSpec::signed(
        &["orders", "create"],
        "trade",
        "Create a new order",
        "Create a live limit, market, stop-loss, take-profit, stop-limit, take-limit, or verified outcome limit order. Supports optional per-order builder fee params. Mainnet orders prompt for confirmation unless --yes is passed; --testnet orders run without a prompt.",
    )
    .long_about("Create a new order.\n\
\n\
Examples:\n  \
hyperliquid --dry-run orders create --coin BTC --side buy --price 60000 --size 0.001\n  \
hyperliquid orders create --coin BTC --side buy --price 60000 --size 0.001 --yes")
    .prompt()
    .testnet_unprompted()
    .args(&[
        arg("coin").kind(InputKind::MarketSymbol).describe("Perpetual coin such as BTC, supported spot pair, or outcome notation (#<encoding> or +<encoding>) for verified outcome limit orders.").asset_input("coin"),
        arg("dex").kind(InputKind::VenueSelector).describe("HIP-3 DEX name.").resource_id("dex"),
        arg("side"),
        arg("price").kind(InputKind::Price).describe("Limit price. For stop-loss and take-profit market trigger orders, also accepted as a legacy trigger price."),
        arg("trigger_price").kind(InputKind::Price),
        arg("size").kind(InputKind::Amount).describe("Base-asset size. Required for limit and trigger-limit orders."),
        arg("amount").kind(InputKind::Amount).describe("Quote-sized amount in USDC. Required for market orders."),
        arg("order_type").describe("Order type: limit, market, market-trigger, or trigger-limit."),
        arg("tif").describe("Time in force: gtc, ioc, or alo."),
        arg("reduce_only").kind(InputKind::Boolean).describe("Only reduce an existing perpetual position; spot markets are not supported."),
        arg("max_slippage_bps").describe("Maximum slippage bound in basis points for market orders. Default 500; allowed range 1-1000."),
        arg("take_profit").kind(InputKind::Price).describe("Take-profit trigger price for a normal-tpsl child order."),
        arg("stop_loss").kind(InputKind::Price).describe("Stop-loss trigger price for a normal-tpsl child order."),
        arg("grouping").describe("Parent-attached TP/SL grouping. orders create requires normal-tpsl when TP/SL children are present."),
        arg("on_behalf_of").kind(InputKind::ActingAccountSelector).describe("Acting-account selector for vaultAddress: subaccount/vault address, wallet name, or wallet id.").resource_id("on-behalf-of selector"),
        arg("margin_mode").describe("Margin mode intent for perpetual orders. Defaults to cross."),
        arg("builder").kind(InputKind::ProtocolObjectAddress).describe("Builder address to receive the additional order fee. Requires --builder-fee-rate.").resource_id("builder address"),
        arg("builder_fee_rate").describe("Builder fee rate percent string, e.g. 0.001%. Requires --builder.").resource_id("builder fee rate"),
        arg("cloid").kind(InputKind::Identifier).describe("0x-prefixed 16-byte client order id.").resource_id("cloid"),
        arg("yes").kind(InputKind::Boolean).describe("Skip the mainnet confirmation prompt for deliberate automation. Testnet orders never prompt."),
    ]);

    async fn run(inv: &Invocation, args: &CreateArgs) -> Result<(), anyhow::Error> {
        validate_create_args(args)?;
        let client = inv.remote()?.api().sdk();
        let resolver = inv.trading_resolver().await?;
        let coin_query = qualify_dex_asset(args.dex.as_deref(), &args.coin);
        let vault_address =
            resolve_optional_acting_account_target(inv, args.on_behalf_of.as_deref())?;
        let vault_address_string = vault_address.map(|address| address.to_string());
        if inv.dry_run() {
            let plan = create_dry_run_plan(client, &resolver, args).await?;
            let command = plan.command();
            let would_execute = plan.would_execute();
            let (signer, acting_as, vault_address_string) =
                dry_run_vault_signing_addresses(inv, vault_address_string);
            let mut details = plan.into_args();
            if let Ok(resolved) = inv.resolve_signer()
                && let Some(object) = details.as_object_mut()
            {
                let user = vault_address.unwrap_or_else(|| resolved.query_address());
                if let Some(helper) =
                    maybe_hip3_dry_run_side_effect(inv.remote()?.api(), user, args, object).await?
                {
                    object.insert("side_effects".to_string(), serde_json::json!([helper]));
                }
            }
            return print_dry_run(
                inv,
                command,
                dry_run_signed_details(
                    inv,
                    would_execute,
                    details,
                    signer,
                    acting_as,
                    vault_address_string,
                ),
            );
        }
        let margin_validation_target =
            if !matches!(parse_asset_query(&args.coin), AssetQuery::Outcome(_)) {
                let resolved = resolver
                    .resolve_or_fetch(client, &coin_query)
                    .await
                    .map_err(CliError::from)?;
                validate_create_resolved_asset(args, &resolved)?;
                match resolved {
                    Market::Perp(perp) => Some((perp.name, perp.dex)),
                    Market::Spot(_) | Market::Outcome(_) => None,
                }
            } else if args.margin_mode.is_some() {
                return Err(CliError::Configuration(
                    "orders create --margin-mode is only supported for perpetual orders"
                        .to_string(),
                )
                .into());
            } else {
                None
            };
        let resolved_signer = inv.resolve_signer()?;
        if let Some((coin, dex)) = margin_validation_target {
            let margin_mode_user = vault_address.unwrap_or_else(|| resolved_signer.query_address());
            validate_order_margin_mode(
                client,
                margin_mode_user,
                coin.as_str(),
                dex.as_deref(),
                args.margin_mode,
            )
            .await?;
        }
        let signer = resolved_signer.selected_signer();
        create(
            order_execution_context(inv, &resolver, &signer, &Self::SPEC)?,
            args,
            vault_address,
            true,
        )
        .await
    }
}

pub struct OrdersScale;

impl CliCommand for OrdersScale {
    type Args = ScaleArgs;

    const SPEC: CommandSpec = CommandSpec::signed(
        &["orders", "scale"],
        "trade",
        "Create a deterministic batch of scaled limit orders",
        "Create a deterministic batch of evenly spaced limit orders. Mainnet orders prompt for confirmation unless --yes is passed; --testnet orders run without a prompt.",
    )
    .prompt()
    .testnet_unprompted()
    .args(&[
        arg("coin").kind(InputKind::MarketSymbol).describe("Perpetual coin, for example BTC.").asset_input("coin"),
        arg("dex").kind(InputKind::VenueSelector).describe("HIP-3 DEX name.").resource_id("dex"),
        arg("side").describe("Order side for every generated leg."),
        arg("start_price"),
        arg("end_price"),
        arg("total_size").kind(InputKind::Amount).describe("Total base-asset size split across all legs."),
        arg("orders").describe("Number of limit orders to generate."),
        arg("tif").describe("Time in force for generated limit orders: gtc, ioc, or alo."),
        arg("reduce_only").kind(InputKind::Boolean),
        arg("on_behalf_of").kind(InputKind::ActingAccountSelector).describe("Acting-account selector for vaultAddress: subaccount/vault address, wallet name, or wallet id.").resource_id("on-behalf-of selector"),
        arg("margin_mode").describe("Margin mode intent for perpetual orders. Defaults to cross."),
        arg("yes").kind(InputKind::Boolean).describe("Skip the mainnet confirmation prompt for deliberate automation. Testnet orders never prompt."),
    ]);

    async fn run(inv: &Invocation, args: &ScaleArgs) -> Result<(), anyhow::Error> {
        validate_scale_args(args)?;
        let client = inv.remote()?.api().sdk();
        let resolver = inv.trading_resolver().await?;
        let vault_address =
            resolve_optional_acting_account_target(inv, args.on_behalf_of.as_deref())?;
        let vault_address_string = vault_address.map(|address| address.to_string());
        if inv.dry_run() {
            let plan = scale_dry_run_plan(client, &resolver, args).await?;
            let (signer, acting_as, vault_address_string) =
                dry_run_vault_signing_addresses(inv, vault_address_string);
            return print_dry_run(
                inv,
                plan.command(),
                dry_run_signed_details(
                    inv,
                    plan.would_execute(),
                    plan.into_args(),
                    signer,
                    acting_as,
                    vault_address_string,
                ),
            );
        }
        let coin_query = qualify_dex_asset(args.dex.as_deref(), &args.coin);
        let margin_validation_target =
            if !matches!(parse_asset_query(&args.coin), AssetQuery::Outcome(_)) {
                match resolver
                    .resolve_or_fetch(client, &coin_query)
                    .await
                    .map_err(CliError::from)?
                {
                    Market::Perp(perp) => Some((perp.name, perp.dex)),
                    Market::Spot(_) | Market::Outcome(_) => {
                        if args.margin_mode.is_some() {
                            return Err(CliError::Configuration(
                                "orders scale --margin-mode is only supported for perpetual orders"
                                    .to_string(),
                            )
                            .into());
                        }
                        None
                    }
                }
            } else if args.margin_mode.is_some() {
                return Err(CliError::Configuration(
                    "orders scale --margin-mode is only supported for perpetual orders".to_string(),
                )
                .into());
            } else {
                None
            };
        let resolved_signer = inv.resolve_signer()?;
        if let Some((coin, dex)) = margin_validation_target {
            let margin_mode_user = vault_address.unwrap_or_else(|| resolved_signer.query_address());
            validate_order_margin_mode(
                client,
                margin_mode_user,
                coin.as_str(),
                dex.as_deref(),
                args.margin_mode,
            )
            .await?;
        }
        let signer = resolved_signer.selected_signer();
        scale(
            order_execution_context(inv, &resolver, &signer, &Self::SPEC)?,
            args,
            vault_address,
        )
        .await
    }
}

pub struct OrdersBatchCreate;

impl CliCommand for OrdersBatchCreate {
    type Args = BatchCreateArgs;

    const SPEC: CommandSpec = CommandSpec::signed(
        &["orders", "batch-create"],
        "trade",
        "Create a batch of limit orders from a JSON file",
        "Create a batch of limit orders from a JSON order-leg file. Mainnet orders prompt for confirmation unless --yes is passed; --testnet orders run without a prompt.",
    )
    .prompt()
    .testnet_unprompted()
    .args(&[
        arg("orders_file").kind(InputKind::FilePath).describe("JSON file containing an array of order legs or an object with an orders array."),
        arg("on_behalf_of").kind(InputKind::ActingAccountSelector).describe("Acting-account selector for vaultAddress: subaccount/vault address, wallet name, or wallet id."),
        arg("yes").kind(InputKind::Boolean).describe("Skip the mainnet confirmation prompt for deliberate automation. Testnet orders never prompt."),
    ]);

    fn validate(args: &BatchCreateArgs) -> Result<(), CliError> {
        validate_file_path(
            &args.orders_file.to_string_lossy(),
            FilePolicy::json_artifact("orders file"),
        )?;
        if let Some(on_behalf_of) = args.on_behalf_of.as_deref() {
            validate_resource_id("on-behalf-of selector", on_behalf_of)?;
        }
        Ok(())
    }

    async fn run(inv: &Invocation, args: &BatchCreateArgs) -> Result<(), anyhow::Error> {
        validate_batch_create_args(args)?;
        let orders = read_validated_batch_create_orders(args)?;
        let client = inv.remote()?.api().sdk();
        let resolver = inv.trading_resolver().await?;
        let vault_address =
            resolve_optional_acting_account_target(inv, args.on_behalf_of.as_deref())?;
        let vault_address_string = vault_address.map(|address| address.to_string());
        if inv.dry_run() {
            let plan = batch_create_dry_run_plan(client, &resolver, args, orders).await?;
            let (signer, acting_as, vault_address_string) =
                dry_run_vault_signing_addresses(inv, vault_address_string);
            return print_dry_run(
                inv,
                plan.command(),
                dry_run_signed_details(
                    inv,
                    plan.would_execute(),
                    plan.into_args(),
                    signer,
                    acting_as,
                    vault_address_string,
                ),
            );
        }
        let resolved_signer = inv.resolve_signer()?;
        let signer = resolved_signer.selected_signer();
        batch_create(
            order_execution_context(inv, &resolver, &signer, &Self::SPEC)?,
            args,
            orders,
            vault_address,
        )
        .await
    }
}

pub struct OrdersTpsl;

impl CliCommand for OrdersTpsl {
    type Args = TpslArgs;

    const SPEC: CommandSpec = CommandSpec::signed(
        &["orders", "tpsl"],
        "trade",
        "Create position-attached TP/SL orders",
        "Create position-attached take-profit and/or stop-loss orders. Dry-run first. Retry: check_open_orders — a second tpsl can duplicate working triggers. Mainnet orders prompt for confirmation unless --yes is passed; --testnet orders run without a prompt.",
    )
    .long_about("Create position-attached take-profit and/or stop-loss orders.\n\
\n\
Triggers accept an absolute price, a percent offset (`+10%`, `-5%`), or `entry`.\n\
\n\
Retry: check open orders first. A second tpsl can duplicate working triggers.\n\
\n\
Examples:\n  \
hyperliquid --format json --dry-run orders tpsl --coin ETH --take-profit +10% --stop-loss -5%\n  \
hyperliquid --format json --dry-run orders tpsl --coin ETH --stop-loss entry\n  \
hyperliquid --format json orders tpsl --coin ETH --take-profit +10% --stop-loss -5% --yes")
    .prompt()
    .testnet_unprompted()
    .args(&[
        arg("coin").kind(InputKind::MarketSymbol).describe("Perpetual coin, for example BTC.").asset_input("coin"),
        arg("dex").kind(InputKind::VenueSelector).describe("HIP-3 DEX name.").resource_id("dex"),
        arg("on_behalf_of").kind(InputKind::ActingAccountSelector).resource_id("on-behalf-of selector"),
        arg("take_profit").kind(InputKind::Price).describe("Take-profit trigger: absolute price, percent offset such as +10%, or entry."),
        arg("stop_loss").kind(InputKind::Price).describe("Stop-loss trigger: absolute price, percent offset such as -5%, or entry."),
        arg("grouping"),
        arg("side").describe("Fixed close side. Omit live to infer from the current position."),
        arg("size").kind(InputKind::Amount).describe("Fixed base-asset size. Omit live to use the current position size."),
        arg("margin_mode").describe("Margin mode intent for perpetual TP/SL orders. Defaults to cross."),
        arg("cloid").kind(InputKind::Identifier).describe("0x-prefixed 16-byte client order id.").resource_id("cloid"),
        arg("yes").kind(InputKind::Boolean).describe("Skip the mainnet confirmation prompt for deliberate automation. Testnet orders never prompt."),
    ]);

    async fn run(inv: &Invocation, args: &TpslArgs) -> Result<(), anyhow::Error> {
        validate_tpsl_args(args)?;
        let client = inv.remote()?.api().sdk();
        let resolver = inv.trading_resolver().await?;
        let coin_query = qualify_dex_asset(args.dex.as_deref(), &args.coin);
        let (resolver, resolved_tpsl_perp) = resolver
            .with_resolved_perp(client, &coin_query)
            .await
            .map_err(CliError::from)?;
        let vault_address =
            resolve_optional_acting_account_target(inv, args.on_behalf_of.as_deref())?;
        let vault_address_string = vault_address.map(|address| address.to_string());
        if inv.dry_run() {
            let plan = tpsl_dry_run_plan(&resolver, args)?;
            let (signer, acting_as, vault_address_string) =
                dry_run_vault_signing_addresses(inv, vault_address_string);
            return print_dry_run(
                inv,
                plan.command(),
                dry_run_signed_details(
                    inv,
                    plan.would_execute(),
                    plan.into_args(),
                    signer,
                    acting_as,
                    vault_address_string,
                ),
            );
        }
        let resolved_signer = inv.resolve_signer()?;
        let margin_mode_user = vault_address.unwrap_or_else(|| resolved_signer.query_address());
        validate_order_margin_mode(
            client,
            margin_mode_user,
            resolved_tpsl_perp.name.as_str(),
            resolved_tpsl_perp.dex.as_deref(),
            args.margin_mode,
        )
        .await?;
        let signer = resolved_signer.selected_signer();
        tpsl(
            order_execution_context(inv, &resolver, &signer, &Self::SPEC)?,
            args,
            vault_address,
            true,
        )
        .await
    }
}

pub struct OrdersTrailingStop;

impl CliCommand for OrdersTrailingStop {
    type Args = TrailingStopArgs;

    const SPEC: CommandSpec = CommandSpec::signed(
        &["orders", "trailing-stop"],
        "trade",
        "Place a native trailing stop on a perp market",
        "Place a native trailing stop on a perp market. It tracks the mark price and fires a market order once the mark retraces from its best level since activation. Dry-run first. Retry: check_open_orders — a second trailing stop can duplicate a working one. Mainnet orders prompt for confirmation unless --yes is passed; --testnet orders run without a prompt.",
    )
    .long_about("Place a native trailing stop on a perp market.\n\
\n\
The stop tracks the mark price and fires a market order once the mark retraces from its best level since activation by --retracement-pct or --retracement-px. A sell trails the highest mark (closes a long); a buy trails the lowest (closes a short). The trigger never moves back. --activation-price delays tracking until the mark reaches it.\n\
\n\
Retry: check open orders first. A second trailing stop can duplicate a working one.\n\
\n\
Examples:\n  \
hyperliquid --format json --dry-run orders trailing-stop --coin BTC --side sell --size 0.01 --retracement-pct 5 --reduce-only\n  \
hyperliquid --format json orders trailing-stop --coin ETH --side sell --size 0.5 --retracement-px 50 --activation-price 3500 --yes")
    .prompt()
    .testnet_unprompted()
    .one_of_required(&[&["retracement_pct"], &["retracement_px"]])
    .args(&[
        arg("coin").kind(InputKind::MarketSymbol).describe("Perpetual coin, for example BTC. Spot and outcome markets are not supported.").asset_input("coin"),
        arg("dex").kind(InputKind::VenueSelector).describe("HIP-3 DEX name.").resource_id("dex"),
        arg("side"),
        arg("size").kind(InputKind::Amount),
        arg("retracement_pct").describe("Retracement from the best mark price as a percent, above 0 and below 100. Use this or --retracement-px."),
        arg("retracement_px").kind(InputKind::Price),
        arg("activation_price").kind(InputKind::Price),
        arg("reduce_only").kind(InputKind::Boolean),
        arg("on_behalf_of").kind(InputKind::ActingAccountSelector).resource_id("on-behalf-of selector"),
        arg("yes").kind(InputKind::Boolean).describe("Skip the mainnet confirmation prompt for deliberate automation. Testnet orders never prompt."),
    ]);

    async fn run(inv: &Invocation, args: &TrailingStopArgs) -> Result<(), anyhow::Error> {
        validate_trailing_stop_args(args)?;
        let client = inv.remote()?.api().sdk();
        let resolver = inv.trading_resolver().await?;
        let coin_query = qualify_dex_asset(args.dex.as_deref(), &args.coin);
        let (resolver, _) = resolver
            .with_resolved_perp(client, &coin_query)
            .await
            .map_err(CliError::from)?;
        let dry_run_plan = trailing_stop_dry_run_plan(&resolver, args)?;
        let vault_address =
            resolve_optional_acting_account_target(inv, args.on_behalf_of.as_deref())?;
        let vault_address_string = vault_address.map(|address| address.to_string());
        if inv.dry_run() {
            let (signer, acting_as, vault_address_string) =
                dry_run_vault_signing_addresses(inv, vault_address_string);
            return print_dry_run(
                inv,
                dry_run_plan.command(),
                dry_run_signed_details(
                    inv,
                    dry_run_plan.would_execute(),
                    dry_run_plan.into_args(),
                    signer,
                    acting_as,
                    vault_address_string,
                ),
            );
        }
        let resolved = inv.resolve_signer()?;
        let signer = resolved.selected_signer();
        trailing_stop(
            order_submission_context(inv, &signer, &Self::SPEC)?,
            &resolver,
            args,
            vault_address,
        )
        .await
    }
}

pub struct OrdersCancel;

impl CliCommand for OrdersCancel {
    type Args = CancelArgs;

    const SPEC: CommandSpec = CommandSpec::signed(
        &["orders", "cancel"],
        "trade",
        "Cancel an order by ID",
        "Cancel an order.",
    )
    .args(&[
        arg("order_id")
            .schema_string()
            .kind(InputKind::Identifier)
            .describe("Exchange-assigned order id."),
        arg("cloid")
            .kind(InputKind::Identifier)
            .describe("Client order id.")
            .resource_id("cloid"),
        arg("on_behalf_of")
            .kind(InputKind::ActingAccountSelector)
            .resource_id("on-behalf-of selector"),
    ]);

    async fn run(inv: &Invocation, args: &CancelArgs) -> Result<(), anyhow::Error> {
        let vault_address =
            resolve_optional_acting_account_target(inv, args.on_behalf_of.as_deref())?;
        let vault_address_string = vault_address.map(|address| address.to_string());
        if inv.dry_run() {
            let plan = cancel_dry_run_plan(args)?;
            let (signer, acting_as, vault_address_string) =
                dry_run_vault_signing_addresses(inv, vault_address_string);
            return print_dry_run(
                inv,
                plan.command(),
                dry_run_signed_details(
                    inv,
                    plan.would_execute(),
                    plan.into_args(),
                    signer,
                    acting_as,
                    vault_address_string,
                ),
            );
        }
        let resolver = inv.trading_resolver().await?;
        let resolved = inv.resolve_signer()?;
        let user = vault_address.unwrap_or_else(|| resolved.query_address());
        let signer = resolved.selected_signer();
        cancel(
            order_execution_context(inv, &resolver, &signer, &Self::SPEC)?,
            user,
            args,
            vault_address,
        )
        .await
    }
}

pub struct OrdersCancelAll;

impl CliCommand for OrdersCancelAll {
    type Args = CancelAllArgs;

    const SPEC: CommandSpec = CommandSpec::signed(
        &["orders", "cancel-all"],
        "trade",
        "Cancel all open orders",
        "Cancel all open orders, optionally filtered by coin.",
    )
    .prompt()
    .args(&[
        arg("coin")
            .kind(InputKind::MarketSymbol)
            .describe("Optional coin filter.")
            .asset_input("coin"),
        arg("dex")
            .kind(InputKind::VenueSelector)
            .describe("HIP-3 DEX name for the coin filter.")
            .resource_id("dex"),
        arg("on_behalf_of")
            .kind(InputKind::ActingAccountSelector)
            .resource_id("on-behalf-of selector"),
        arg("yes")
            .kind(InputKind::Boolean)
            .describe("Skip confirmation prompt for deliberate automation."),
    ]);

    async fn run(inv: &Invocation, args: &CancelAllArgs) -> Result<(), anyhow::Error> {
        let vault_address =
            resolve_optional_acting_account_target(inv, args.on_behalf_of.as_deref())?;
        let vault_address_string = vault_address.map(|address| address.to_string());
        if inv.dry_run() {
            let resolver = inv.trading_resolver().await?;
            let plan = cancel_all_dry_run_plan(&resolver, args)?;
            let (signer, acting_as, vault_address_string) =
                dry_run_vault_signing_addresses(inv, vault_address_string);
            return print_dry_run(
                inv,
                plan.command(),
                dry_run_signed_details(
                    inv,
                    plan.would_execute(),
                    plan.into_args(),
                    signer,
                    acting_as,
                    vault_address_string,
                ),
            );
        }
        let resolver = inv.trading_resolver().await?;
        let resolved = inv.resolve_signer()?;
        let user = vault_address.unwrap_or_else(|| resolved.query_address());
        let signer = resolved.selected_signer();
        cancel_all(
            order_execution_context(inv, &resolver, &signer, &Self::SPEC)?,
            user,
            args,
            vault_address,
        )
        .await
    }
}

pub struct OrdersModify;

impl CliCommand for OrdersModify {
    type Args = ModifyArgs;

    const SPEC: CommandSpec = CommandSpec::signed(
        &["orders", "modify"],
        "trade",
        "Modify an existing order",
        "Modify an existing order.",
    )
    .one_of_required(&[&["order_id"], &["cloid"]])
    .args(&[
        arg("order_id")
            .kind(InputKind::Identifier)
            .describe("Exchange-assigned order id."),
        arg("cloid")
            .kind(InputKind::Identifier)
            .describe("0x-prefixed 16-byte client order id.")
            .resource_id("cloid"),
        arg("price")
            .kind(InputKind::Price)
            .describe("Replacement price."),
        arg("trigger_price")
            .kind(InputKind::Price)
            .describe("Replacement trigger price for trigger orders."),
        arg("size")
            .kind(InputKind::Amount)
            .describe("Replacement size."),
        arg("always_place").kind(InputKind::Boolean),
        arg("on_behalf_of")
            .kind(InputKind::ActingAccountSelector)
            .resource_id("on-behalf-of selector"),
    ]);

    async fn run(inv: &Invocation, args: &ModifyArgs) -> Result<(), anyhow::Error> {
        validate_modify_args(args)?;
        let vault_address =
            resolve_optional_acting_account_target(inv, args.on_behalf_of.as_deref())?;
        let vault_address_string = vault_address.map(|address| address.to_string());
        if inv.dry_run() {
            let plan = modify_dry_run_plan(args)?;
            let (signer, acting_as, vault_address_string) =
                dry_run_vault_signing_addresses(inv, vault_address_string);
            return print_dry_run(
                inv,
                plan.command(),
                dry_run_signed_details(
                    inv,
                    plan.would_execute(),
                    plan.into_args(),
                    signer,
                    acting_as,
                    vault_address_string,
                ),
            );
        }
        let resolver = inv.trading_resolver().await?;
        let resolved = inv.resolve_signer()?;
        let user = vault_address.unwrap_or_else(|| resolved.query_address());
        let signer = resolved.selected_signer();
        modify(
            order_execution_context(inv, &resolver, &signer, &Self::SPEC)?,
            user,
            args,
            vault_address,
        )
        .await
    }
}

pub struct OrdersTwapCreate;

impl CliCommand for OrdersTwapCreate {
    type Args = TwapCreateArgs;

    const SPEC: CommandSpec = CommandSpec::signed(
        &["orders", "twap-create"],
        "trade",
        "Create a TWAP order",
        "Create a TWAP order. Mainnet orders prompt for confirmation unless --yes is passed; --testnet orders run without a prompt.",
    )
    .prompt()
    .testnet_unprompted()
    .args(&[
        arg("coin").kind(InputKind::MarketSymbol).describe("Perpetual coin, for example BTC.").asset_input("coin"),
        arg("dex").kind(InputKind::VenueSelector).describe("HIP-3 DEX name.").resource_id("dex"),
        arg("on_behalf_of").kind(InputKind::ActingAccountSelector).resource_id("on-behalf-of selector"),
        arg("side"),
        arg("size").kind(InputKind::Amount).describe("Total base-asset size."),
        arg("duration").kind(InputKind::Duration).describe("Whole-minute duration in seconds. Minimum 300."),
        arg("margin_mode").describe("Require the target perpetual position to already use this margin mode."),
        arg("randomize").kind(InputKind::Boolean).describe("Randomize execution timing and suborder sizes up to ±20%."),
        arg("yes").kind(InputKind::Boolean).describe("Skip the mainnet confirmation prompt for deliberate automation. Testnet orders never prompt."),
    ]);

    async fn run(inv: &Invocation, args: &TwapCreateArgs) -> Result<(), anyhow::Error> {
        let client = inv.remote()?.api().sdk();
        let resolver = inv.trading_resolver().await?;
        let coin_query = qualify_dex_asset(args.dex.as_deref(), &args.coin);
        let (resolver, resolved_twap_perp) = resolver
            .with_resolved_perp(client, &coin_query)
            .await
            .map_err(CliError::from)?;
        let dry_run_plan = twap_create_dry_run_plan(&resolver, args)?;
        let vault_address =
            resolve_optional_acting_account_target(inv, args.on_behalf_of.as_deref())?;
        let vault_address_string = vault_address.map(|address| address.to_string());
        if inv.dry_run() {
            let (signer, acting_as, vault_address_string) =
                dry_run_vault_signing_addresses(inv, vault_address_string);
            return print_dry_run(
                inv,
                dry_run_plan.command(),
                dry_run_signed_details(
                    inv,
                    dry_run_plan.would_execute(),
                    dry_run_plan.into_args(),
                    signer,
                    acting_as,
                    vault_address_string,
                ),
            );
        }
        let resolved = inv.resolve_signer()?;
        let margin_mode_user = vault_address.unwrap_or_else(|| resolved.query_address());
        validate_order_margin_mode(
            client,
            margin_mode_user,
            resolved_twap_perp.name.as_str(),
            resolved_twap_perp.dex.as_deref(),
            args.margin_mode,
        )
        .await?;
        let signer = resolved.selected_signer();
        twap_create(
            order_submission_context(inv, &signer, &Self::SPEC)?,
            &resolver,
            args,
            vault_address,
        )
        .await
    }
}

pub struct OrdersTwapCancel;

impl CliCommand for OrdersTwapCancel {
    type Args = TwapCancelArgs;

    const SPEC: CommandSpec = CommandSpec::signed(
        &["orders", "twap-cancel"],
        "trade",
        "Cancel a TWAP order",
        "Cancel a TWAP order.",
    )
    .args(&[
        arg("twap_id")
            .kind(InputKind::Identifier)
            .describe("TWAP id."),
        arg("coin")
            .kind(InputKind::MarketSymbol)
            .describe("Perpetual coin.")
            .asset_input("coin"),
        arg("dex")
            .kind(InputKind::VenueSelector)
            .describe("HIP-3 DEX name.")
            .resource_id("dex"),
        arg("on_behalf_of")
            .kind(InputKind::ActingAccountSelector)
            .resource_id("on-behalf-of selector"),
    ]);

    async fn run(inv: &Invocation, args: &TwapCancelArgs) -> Result<(), anyhow::Error> {
        let client = inv.remote()?.api().sdk();
        let resolver = inv.trading_resolver().await?;
        let coin_query = qualify_dex_asset(args.dex.as_deref(), &args.coin);
        let (resolver, _) = resolver
            .with_resolved_perp(client, &coin_query)
            .await
            .map_err(CliError::from)?;
        let dry_run_plan = twap_cancel_dry_run_plan(&resolver, args)?;
        let vault_address =
            resolve_optional_acting_account_target(inv, args.on_behalf_of.as_deref())?;
        let vault_address_string = vault_address.map(|address| address.to_string());
        if inv.dry_run() {
            let (signer, acting_as, vault_address_string) =
                dry_run_vault_signing_addresses(inv, vault_address_string);
            return print_dry_run(
                inv,
                dry_run_plan.command(),
                dry_run_signed_details(
                    inv,
                    dry_run_plan.would_execute(),
                    dry_run_plan.into_args(),
                    signer,
                    acting_as,
                    vault_address_string,
                ),
            );
        }
        let resolved = inv.resolve_signer()?;
        let signer = resolved.selected_signer();
        twap_cancel(
            order_submission_context(inv, &signer, &Self::SPEC)?,
            &resolver,
            args,
            vault_address,
        )
        .await
    }
}

pub struct OrdersScheduleCancel;

impl CliCommand for OrdersScheduleCancel {
    type Args = ScheduleCancelArgs;

    const SPEC: CommandSpec = CommandSpec::signed(
        &["orders", "schedule-cancel"],
        "trade",
        "Schedule a cancel-all (dead man's switch)",
        "Configure a dead man's switch. Mainnet orders prompt for confirmation unless --yes is passed; --testnet orders run without a prompt.",
    )
    .prompt()
    .testnet_unprompted()
    .args(&[
        arg("in_duration").kind(InputKind::Duration).describe("Relative duration, for example 30s or 5m."),
        arg("clear").kind(InputKind::Boolean).schema_string(),
        arg("on_behalf_of").kind(InputKind::ActingAccountSelector).resource_id("on-behalf-of selector"),
        arg("yes").kind(InputKind::Boolean).describe("Skip the mainnet confirmation prompt for deliberate automation. Testnet orders never prompt."),
    ]);

    async fn run(inv: &Invocation, args: &ScheduleCancelArgs) -> Result<(), anyhow::Error> {
        let vault_address =
            resolve_optional_acting_account_target(inv, args.on_behalf_of.as_deref())?;
        let vault_address_string = vault_address.map(|address| address.to_string());
        if inv.dry_run() {
            let plan = schedule_cancel_dry_run_plan(args)?;
            let (signer, acting_as, vault_address_string) =
                dry_run_vault_signing_addresses(inv, vault_address_string);
            return print_dry_run(
                inv,
                plan.command(),
                dry_run_signed_details(
                    inv,
                    plan.would_execute(),
                    plan.into_args(),
                    signer,
                    acting_as,
                    vault_address_string,
                ),
            );
        }
        let resolved = inv.resolve_signer()?;
        let signer = resolved.selected_signer();
        schedule_cancel(
            order_submission_context(inv, &signer, &Self::SPEC)?,
            args,
            vault_address,
        )
        .await
    }
}

pub struct OrdersChase;

impl CliCommand for OrdersChase {
    type Args = ChaseArgs;

    const SPEC: CommandSpec = CommandSpec::signed(
        &["orders", "chase"],
        "trade",
        "Chase mid with ALO requotes",
        "Bounded ALO requote loop that chases mid by a basis-point offset until fill, timeout, or max chase distance. Dry-run first. Retry: check_open_orders — not idempotent. Agent mode requires --timeout. Mainnet orders prompt for confirmation unless --yes is passed; --testnet orders run without a prompt.",
    )
    .long_about("Bounded ALO requote loop that chases mid until fill, timeout, or max chase distance.\n\
\n\
Agent/non-TTY mode requires --timeout. Retry: check open orders first. Not idempotent.\n\
\n\
Examples:\n  \
hyperliquid --format json --dry-run orders chase --coin ETH --side buy --size 0.5 --timeout 60s\n  \
hyperliquid --format json orders chase --coin ETH --side buy --size 0.5 --timeout 60s --yes")
    .prompt()
    .testnet_unprompted()
    .args(&[
        arg("coin").kind(InputKind::MarketSymbol).describe("Perpetual coin, for example ETH.").asset_input("coin"),
        arg("dex").kind(InputKind::VenueSelector).describe("HIP-3 DEX name."),
        arg("side"),
        arg("size").kind(InputKind::Amount),
        arg("offset").describe("Offset from mid in basis points."),
        arg("timeout").kind(InputKind::Duration).describe("Max chase duration, for example 60s. Required in agent mode."),
        arg("interval").kind(InputKind::Duration),
        arg("max_chase").describe("Cancel and stop if mid travels this many basis points from the first mid."),
        arg("reduce_only").kind(InputKind::Boolean).describe("Only reduce an existing perpetual position; spot markets are not supported."),
        arg("on_behalf_of").kind(InputKind::ActingAccountSelector).describe("Acting-account selector for vaultAddress: subaccount/vault address, wallet name, or wallet id."),
        arg("yes").kind(InputKind::Boolean).describe("Skip the mainnet confirmation prompt for deliberate automation. Testnet orders never prompt."),
    ]);

    async fn run(inv: &Invocation, args: &ChaseArgs) -> Result<(), anyhow::Error> {
        chase_dry_run_plan(args, None, chase::requires_explicit_timeout(inv))?;
        let client = inv.remote()?.api().sdk();
        let resolver = inv.trading_resolver().await?;
        let vault_address =
            resolve_optional_acting_account_target(inv, args.on_behalf_of.as_deref())?;
        let vault_address_string = vault_address.map(|address| address.to_string());
        if inv.dry_run() {
            let coin_key = qualify_dex_asset(args.dex.as_deref(), &args.coin);
            let mid = client
                .all_mids(None)
                .await
                .ok()
                .and_then(|mids| mids.get(&coin_key).copied());
            let plan = chase_dry_run_plan(args, mid, chase::requires_explicit_timeout(inv))?;
            let (signer, acting_as, vault_address_string) =
                dry_run_vault_signing_addresses(inv, vault_address_string);
            return print_dry_run(
                inv,
                "orders chase",
                dry_run_signed_details(inv, "chase", plan, signer, acting_as, vault_address_string),
            );
        }
        let resolved_signer = inv.resolve_signer()?;
        let signer = resolved_signer.selected_signer();
        chase(
            order_execution_context(inv, &resolver, &signer, &Self::SPEC)?,
            args,
            vault_address,
        )
        .await
    }
}

pub struct OrdersBracket;

impl CliCommand for OrdersBracket {
    type Args = BracketArgs;

    const SPEC: CommandSpec = CommandSpec::signed(
        &["orders", "bracket"],
        "trade",
        "Create an entry order with linked TP/SL",
        "Place an entry order, then arm position TP/SL. Limit entries wait for fill up to --entry-timeout. Dry-run first. Retry: check_open_orders — not idempotent. Emits one result object. Mainnet orders prompt for confirmation unless --yes is passed; --testnet orders run without a prompt.",
    )
    .long_about("Place an entry order, then arm position TP/SL. Limit entries wait for fill up to --entry-timeout.\n\
\n\
Retry: check open orders first. Not idempotent. On limit-entry timeout the remaining entry is cancelled and any filled portion is protected.\n\
\n\
Examples:\n  \
hyperliquid --format json --dry-run orders bracket --coin ETH --side buy --size 0.1 --take-profit +10% --stop-loss -5%\n  \
hyperliquid --format json --dry-run orders bracket --coin ETH --side buy --entry limit --price 3000 --size 0.1 --take-profit +10% --stop-loss -5%\n  \
hyperliquid --format json orders bracket --coin ETH --side buy --size 0.1 --take-profit +10% --stop-loss -5% --yes")
    .prompt()
    .testnet_unprompted()
    .args(&[
        arg("coin").kind(InputKind::MarketSymbol).describe("Perpetual coin, for example ETH.").asset_input("coin"),
        arg("side"),
        arg("size").kind(InputKind::Amount).describe("Base-asset size."),
        arg("amount").kind(InputKind::Amount).describe("Quote-sized amount for market entry."),
        arg("entry").describe("Entry order type."),
        arg("price").kind(InputKind::Price).describe("Limit entry price."),
        arg("take_profit").kind(InputKind::Price).describe("Take-profit trigger: absolute price, +10%, or entry."),
        arg("stop_loss").kind(InputKind::Price).describe("Stop-loss trigger: absolute price, -5%, or entry."),
        arg("entry_timeout").kind(InputKind::Duration).describe("How long to wait for a limit entry fill before the remaining entry is cancelled and any filled portion is protected."),
        arg("dex").kind(InputKind::VenueSelector).describe("HIP-3 DEX name."),
        arg("max_slippage_bps").describe("Maximum slippage bound in basis points for market orders. Default 500; allowed range 1-1000."),
        arg("on_behalf_of").kind(InputKind::ActingAccountSelector).describe("Acting-account selector for vaultAddress: subaccount/vault address, wallet name, or wallet id."),
        arg("yes").kind(InputKind::Boolean).describe("Skip the mainnet confirmation prompt for deliberate automation. Testnet orders never prompt."),
    ]);

    async fn run(inv: &Invocation, args: &BracketArgs) -> Result<(), anyhow::Error> {
        validate_bracket_args(args)?;
        let client = inv.remote()?.api().sdk();
        let resolver = inv.trading_resolver().await?;
        let vault_address =
            resolve_optional_acting_account_target(inv, args.on_behalf_of.as_deref())?;
        let vault_address_string = vault_address.map(|address| address.to_string());
        if inv.dry_run() {
            let plan = bracket_dry_run_plan(client, &resolver, args).await?;
            let (signer, acting_as, vault_address_string) =
                dry_run_vault_signing_addresses(inv, vault_address_string);
            return print_dry_run(
                inv,
                "orders bracket",
                dry_run_signed_details(
                    inv,
                    "bracket",
                    plan,
                    signer,
                    acting_as,
                    vault_address_string,
                ),
            );
        }
        let resolved_signer = inv.resolve_signer()?;
        let signer = resolved_signer.selected_signer();
        bracket(
            order_execution_context(inv, &resolver, &signer, &Self::SPEC)?,
            args,
            vault_address,
        )
        .await
    }
}

pub struct Buy;

impl CliCommand for Buy {
    type Args = BuySellArgs;

    const SPEC: CommandSpec = CommandSpec::signed(
        &["buy"],
        "trade",
        "Buy at market (shortcut for orders create --side buy)",
        "Market buy alias for orders create --side buy. Limit if --price is set. Accepts --size or --amount. Dry-run first. Retry: check_open_orders — not idempotent. Mainnet orders prompt for confirmation unless --yes is passed; --testnet orders run without a prompt.",
    )
    .long_about("Buy at market. Shortcut for orders create --side buy. Places a limit order if --price is set.\n\
\n\
Retry: check open orders first. Not idempotent. Inspect `orders open` and `positions list` before rerunning.\n\
\n\
Examples:\n  \
hyperliquid --format json --dry-run buy --coin BTC --size 0.001\n  \
hyperliquid --format json --dry-run buy --coin BTC --amount 20\n  \
hyperliquid --format json --dry-run buy --coin ETH --price 3000 --size 0.1\n  \
hyperliquid --format json buy --coin BTC --size 0.001 --yes")
    .prompt()
    .testnet_unprompted()
    .args(&[
        arg("coin").kind(InputKind::MarketSymbol).describe("Perpetual coin, spot pair, or outcome notation.").asset_input("coin"),
        arg("dex").kind(InputKind::VenueSelector).describe("HIP-3 DEX name.").resource_id("dex"),
        arg("price").kind(InputKind::Price),
        arg("size").kind(InputKind::Amount).describe("Base-asset size. Accepted for market and required for limit."),
        arg("amount").kind(InputKind::Amount),
        arg("order_type").describe("Order type. Defaults to limit when --price is set, otherwise market."),
        arg("tif").describe("Time in force: gtc, ioc, or alo."),
        arg("reduce_only").kind(InputKind::Boolean).describe("Only reduce an existing perpetual position; spot markets are not supported."),
        arg("max_slippage_bps").describe("Maximum slippage bound in basis points for market orders. Default 500; allowed range 1-1000."),
        arg("on_behalf_of").kind(InputKind::ActingAccountSelector).describe("Acting-account selector for vaultAddress: subaccount/vault address, wallet name, or wallet id."),
        arg("margin_mode").describe("Margin mode intent for perpetual orders. Defaults to cross."),
        arg("builder").kind(InputKind::ProtocolObjectAddress).describe("Builder address to receive the additional order fee. Requires --builder-fee-rate."),
        arg("builder_fee_rate").describe("Builder fee rate percent string, e.g. 0.001%. Requires --builder."),
        arg("cloid").kind(InputKind::Identifier).describe("0x-prefixed 16-byte client order id."),
        arg("yes").kind(InputKind::Boolean).describe("Skip the mainnet confirmation prompt for deliberate automation. Testnet orders never prompt."),
    ]);

    async fn run(inv: &Invocation, args: &BuySellArgs) -> Result<(), anyhow::Error> {
        let create_args = args.clone().into_create(OrderSide::Buy);
        OrdersCreate::run(inv, &create_args).await
    }
}

pub struct Sell;

impl CliCommand for Sell {
    type Args = BuySellArgs;

    const SPEC: CommandSpec = CommandSpec::signed(
        &["sell"],
        "trade",
        "Sell at market (shortcut for orders create --side sell)",
        "Market sell alias for orders create --side sell. Limit if --price is set. Accepts --size or --amount. Dry-run first. Retry: check_open_orders — not idempotent. Mainnet orders prompt for confirmation unless --yes is passed; --testnet orders run without a prompt.",
    )
    .long_about("Sell at market. Shortcut for orders create --side sell. Places a limit order if --price is set.\n\
\n\
Retry: check open orders first. Not idempotent. Inspect `orders open` and `positions list` before rerunning.\n\
\n\
Examples:\n  \
hyperliquid --format json --dry-run sell --coin BTC --size 0.001\n  \
hyperliquid --format json --dry-run sell --coin ETH --price 3000 --size 0.1\n  \
hyperliquid --format json sell --coin BTC --size 0.001 --yes")
    .prompt()
    .testnet_unprompted()
    .args(&[
        arg("coin").kind(InputKind::MarketSymbol).describe("Perpetual coin, spot pair, or outcome notation.").asset_input("coin"),
        arg("dex").kind(InputKind::VenueSelector).describe("HIP-3 DEX name.").resource_id("dex"),
        arg("price").kind(InputKind::Price),
        arg("size").kind(InputKind::Amount).describe("Base-asset size. Accepted for market and required for limit."),
        arg("amount").kind(InputKind::Amount),
        arg("order_type").describe("Order type. Defaults to limit when --price is set, otherwise market."),
        arg("tif").describe("Time in force: gtc, ioc, or alo."),
        arg("reduce_only").kind(InputKind::Boolean).describe("Only reduce an existing perpetual position; spot markets are not supported."),
        arg("max_slippage_bps").describe("Maximum slippage bound in basis points for market orders. Default 500; allowed range 1-1000."),
        arg("on_behalf_of").kind(InputKind::ActingAccountSelector).describe("Acting-account selector for vaultAddress: subaccount/vault address, wallet name, or wallet id."),
        arg("margin_mode").describe("Margin mode intent for perpetual orders. Defaults to cross."),
        arg("builder").kind(InputKind::ProtocolObjectAddress).describe("Builder address to receive the additional order fee. Requires --builder-fee-rate."),
        arg("builder_fee_rate").describe("Builder fee rate percent string, e.g. 0.001%. Requires --builder."),
        arg("cloid").kind(InputKind::Identifier).describe("0x-prefixed 16-byte client order id."),
        arg("yes").kind(InputKind::Boolean).describe("Skip the mainnet confirmation prompt for deliberate automation. Testnet orders never prompt."),
    ]);

    async fn run(inv: &Invocation, args: &BuySellArgs) -> Result<(), anyhow::Error> {
        let create_args = args.clone().into_create(OrderSide::Sell);
        OrdersCreate::run(inv, &create_args).await
    }
}

pub struct OutcomeBuy;

impl CliCommand for OutcomeBuy {
    type Args = OutcomeOrderArgs;

    const SPEC: CommandSpec = CommandSpec::signed(
        &["outcome-buy"],
        "trade",
        "Buy a HIP-4 outcome token",
        "Buy a HIP-4 outcome token. Coin must be #N or +N notation. Dry-run first. Retry: check_open_orders — not idempotent. Mainnet orders prompt for confirmation unless --yes is passed; --testnet orders run without a prompt.",
    )
    .long_about("Buy a HIP-4 outcome token. --coin must be #N or +N notation.\n\
\n\
Retry: check open orders first. Not idempotent.\n\
\n\
Examples:\n  \
hyperliquid --format json --dry-run outcome-buy --coin #10 --size 1\n  \
hyperliquid --format json outcome-buy --coin #10 --size 1 --yes")
    .prompt()
    .testnet_unprompted()
    .args(&[
        arg("coin").kind(InputKind::MarketSymbol).describe("Outcome notation such as #10 or +10.").asset_input("coin"),
        arg("price").kind(InputKind::Price).describe("Limit price. When omitted, attempts a market order."),
        arg("size").kind(InputKind::Amount).describe("Outcome token size."),
        arg("dex").kind(InputKind::VenueSelector).describe("HIP-3 DEX name."),
        arg("amount").kind(InputKind::Amount),
        arg("order_type").describe("Order type. Defaults to limit when --price is set, otherwise market."),
        arg("tif").describe("Time in force: gtc, ioc, or alo."),
        arg("reduce_only").kind(InputKind::Boolean).describe("Only reduce an existing perpetual position; spot markets are not supported."),
        arg("max_slippage_bps").describe("Maximum slippage bound in basis points for market orders. Default 500; allowed range 1-1000."),
        arg("on_behalf_of").kind(InputKind::ActingAccountSelector).describe("Acting-account selector for vaultAddress: subaccount/vault address, wallet name, or wallet id."),
        arg("margin_mode").describe("Margin mode intent for perpetual orders. Defaults to cross."),
        arg("builder").kind(InputKind::ProtocolObjectAddress).describe("Builder address to receive the additional order fee. Requires --builder-fee-rate."),
        arg("builder_fee_rate").describe("Builder fee rate percent string, e.g. 0.001%. Requires --builder."),
        arg("cloid").kind(InputKind::Identifier).describe("0x-prefixed 16-byte client order id."),
        arg("yes").kind(InputKind::Boolean).describe("Skip the mainnet confirmation prompt for deliberate automation. Testnet orders never prompt."),
    ]);

    async fn run(inv: &Invocation, args: &OutcomeOrderArgs) -> Result<(), anyhow::Error> {
        ensure_outcome_coin(&args.inner.coin)?;
        let create_args = args.inner.clone().into_create(OrderSide::Buy);
        OrdersCreate::run(inv, &create_args).await
    }
}

pub struct OutcomeSell;

impl CliCommand for OutcomeSell {
    type Args = OutcomeOrderArgs;

    const SPEC: CommandSpec = CommandSpec::signed(
        &["outcome-sell"],
        "trade",
        "Sell a HIP-4 outcome token",
        "Sell a HIP-4 outcome token. Coin must be #N or +N notation. Dry-run first. Retry: check_open_orders — not idempotent. Mainnet orders prompt for confirmation unless --yes is passed; --testnet orders run without a prompt.",
    )
    .long_about("Sell a HIP-4 outcome token. --coin must be #N or +N notation.\n\
\n\
Retry: check open orders first. Not idempotent.\n\
\n\
Examples:\n  \
hyperliquid --format json --dry-run outcome-sell --coin #10 --size 1\n  \
hyperliquid --format json outcome-sell --coin #10 --size 1 --yes")
    .prompt()
    .testnet_unprompted()
    .args(&[
        arg("coin").kind(InputKind::MarketSymbol).describe("Outcome notation such as #10 or +10.").asset_input("coin"),
        arg("price").kind(InputKind::Price).describe("Limit price. When omitted, attempts a market order."),
        arg("size").kind(InputKind::Amount).describe("Outcome token size."),
        arg("dex").kind(InputKind::VenueSelector).describe("HIP-3 DEX name."),
        arg("amount").kind(InputKind::Amount),
        arg("order_type").describe("Order type. Defaults to limit when --price is set, otherwise market."),
        arg("tif").describe("Time in force: gtc, ioc, or alo."),
        arg("reduce_only").kind(InputKind::Boolean).describe("Only reduce an existing perpetual position; spot markets are not supported."),
        arg("max_slippage_bps").describe("Maximum slippage bound in basis points for market orders. Default 500; allowed range 1-1000."),
        arg("on_behalf_of").kind(InputKind::ActingAccountSelector).describe("Acting-account selector for vaultAddress: subaccount/vault address, wallet name, or wallet id."),
        arg("margin_mode").describe("Margin mode intent for perpetual orders. Defaults to cross."),
        arg("builder").kind(InputKind::ProtocolObjectAddress).describe("Builder address to receive the additional order fee. Requires --builder-fee-rate."),
        arg("builder_fee_rate").describe("Builder fee rate percent string, e.g. 0.001%. Requires --builder."),
        arg("cloid").kind(InputKind::Identifier).describe("0x-prefixed 16-byte client order id."),
        arg("yes").kind(InputKind::Boolean).describe("Skip the mainnet confirmation prompt for deliberate automation. Testnet orders never prompt."),
    ]);

    async fn run(inv: &Invocation, args: &OutcomeOrderArgs) -> Result<(), anyhow::Error> {
        ensure_outcome_coin(&args.inner.coin)?;
        let create_args = args.inner.clone().into_create(OrderSide::Sell);
        OrdersCreate::run(inv, &create_args).await
    }
}

async fn watch_open_orders(
    inv: &Invocation,
    max_ticks: Option<usize>,
) -> Result<(), anyhow::Error> {
    let remote = inv.remote()?;
    let client = remote.api().sdk();
    let resolver = std::sync::Arc::new(inv.asset_resolver().await?);
    let resolved = inv.resolve_signer()?;
    let user = resolved.query_address();

    crate::watch::run_snapshot_watch(
        inv.session(),
        "Live open orders",
        remote.chain(),
        vec![Subscription::OrderUpdates { user }],
        max_ticks,
        move || {
            let resolver = std::sync::Arc::clone(&resolver);
            async move { open_snapshot(client, &resolver, user).await }
        },
    )
    .await
}

async fn validate_order_margin_mode(
    client: &hypersdk::hypercore::HttpClient,
    user: hypersdk::Address,
    coin: &str,
    dex: Option<&str>,
    margin_mode: Option<MarginModeArg>,
) -> Result<(), anyhow::Error> {
    let Some(margin_mode) = margin_mode else {
        return Ok(());
    };

    let state = client
        .clearinghouse_state(user, dex.map(ToOwned::to_owned))
        .await?;
    let Some(position) = state
        .asset_positions
        .into_iter()
        .find(|position| position.position.coin.eq_ignore_ascii_case(coin))
    else {
        if matches!(margin_mode, MarginModeArg::Cross) {
            return Ok(());
        }
        return Err(CliError::Configuration(format!(
            "orders with --margin-mode {} require an open {} perp position so the mode can be verified; establish it first with `positions update-leverage --coin {} --leverage <N>{}`",
            margin_mode,
            coin,
            coin,
            if matches!(margin_mode, MarginModeArg::Isolated) {
                " --isolated"
            } else {
                ""
            }
        ))
        .into());
    };

    let actual_margin_mode = match position.position.leverage.leverage_type {
        hypersdk::hypercore::types::LeverageType::Cross => MarginModeArg::Cross,
        hypersdk::hypercore::types::LeverageType::Isolated => MarginModeArg::Isolated,
    };
    if actual_margin_mode != margin_mode {
        return Err(CliError::Configuration(format!(
            "orders requested --margin-mode {} but the current {} perp position is {}; use `positions update-leverage --coin {} --leverage <N>{}` first",
            margin_mode,
            coin,
            actual_margin_mode,
            coin,
            if matches!(margin_mode, MarginModeArg::Isolated) {
                " --isolated"
            } else {
                ""
            }
        ))
        .into());
    }

    Ok(())
}

async fn maybe_hip3_dry_run_side_effect(
    api: &crate::http_api::ApiClient,
    user: hypersdk::Address,
    args: &CreateArgs,
    preview: &serde_json::Map<String, serde_json::Value>,
) -> Result<Option<serde_json::Value>, anyhow::Error> {
    let Some(dex) = hip3_dex_from_coin(&args.coin, args.dex.as_deref()) else {
        return Ok(None);
    };
    let notional = preview_notional(preview).unwrap_or(hypersdk::Decimal::ZERO);
    let plan = plan_hip3_margin_transfer(api, user, Some(dex.as_str()), notional).await?;
    Ok(plan.map(|plan| plan.to_json()))
}

fn preview_notional(
    preview: &serde_json::Map<String, serde_json::Value>,
) -> Option<hypersdk::Decimal> {
    let size = preview
        .get("size")?
        .as_str()?
        .parse::<hypersdk::Decimal>()
        .ok()?;
    let price = preview
        .get("limit_px")
        .or_else(|| preview.get("price"))
        .and_then(serde_json::Value::as_str)?
        .parse::<hypersdk::Decimal>()
        .ok()?;
    Some(size * price)
}

fn ensure_outcome_coin(coin: &str) -> Result<(), CliError> {
    crate::assets::asset_id::parse_outcome_notation(coin)?;
    Ok(())
}

fn order_submission_context<'a>(
    inv: &'a Invocation,
    signer: &'a crate::signing::SelectedSigner,
    spec: &CommandSpec,
) -> Result<OrderSubmissionContext<'a>, CliError> {
    let remote = inv.remote()?;
    Ok(OrderSubmissionContext {
        inv,
        api: remote.api(),
        signer,
        require_mainnet_confirmation: spec.testnet_unprompted
            && remote.network() == Network::Mainnet,
    })
}

fn order_execution_context<'a>(
    inv: &'a Invocation,
    resolver: &'a AssetResolver,
    signer: &'a crate::signing::SelectedSigner,
    spec: &CommandSpec,
) -> Result<OrderExecutionContext<'a>, CliError> {
    Ok(OrderExecutionContext {
        submission: order_submission_context(inv, signer, spec)?,
        resolver,
    })
}
