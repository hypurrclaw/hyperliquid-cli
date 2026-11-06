use super::*;

use hyperliquid_cli::commands::{
    account, asset, borrowlend, meta, orderbook, outcomes, perps, positions, prio, referral,
    schema, setup, spot, staking, status, vaults, wallet,
};

pub(super) async fn run(
    cli: &Cli,
    registry_path: Option<&[String]>,
    confirmation: Confirmation,
    mut inv: Invocation,
) -> Result<(), anyhow::Error> {
    if cli.command.is_none() {
        return print_help_to_stdout(&[]);
    }
    validate_cli_inputs(cli)?;
    validate_stream_bounds(cli, &inv)?;

    if matches!(cli.command, Some(Commands::Update)) {
        if cli.payload_json.is_some() || cli.payload_file.is_some() {
            return Err(errors::CliError::Unsupported(
                "payload input is not supported for update".to_string(),
            )
            .into());
        }
        return hyperliquid_cli::update::update(&inv).await;
    }

    let command_contract = command_contract_for_path(registry_path)?;
    if cli.payload_json.is_some() || cli.payload_file.is_some() {
        validate_raw_payload_supported(command_contract.as_ref())?;
    }
    validate_prompt_policy(cli, &inv, command_contract.as_ref(), confirmation)?;

    if let Some(result) = run_local_command_without_context(cli, &inv).await {
        return result;
    }

    inv.set_payload(read_payload_json(cli)?);
    if inv.dry_run() && !command_accepts_mutating_preview(command_contract.as_ref()) {
        return Err(errors::CliError::Unsupported(
            "--dry-run is only supported for mutating commands with preview support".to_string(),
        )
        .into());
    }
    if inv.payload().is_some() && !inv.dry_run() {
        return Err(errors::CliError::Unsupported(
            "payload input currently requires --dry-run so raw payloads can be validated without side effects"
                .to_string(),
        )
        .into());
    }

    let has_api_override = inv.remote()?.api().has_override();
    if inv.dry_run() {
        // Signed exchange actions name the signer in their preview. Resolution
        // never prompts; a failure is reported inside the preview instead.
        inv.attach_dry_run_context(command_contract.as_ref().is_some_and(|command| {
            command.auth_required && command.lifecycle == Lifecycle::LiveMutating
        }));
    }
    if has_api_override && command_uses_api(command_contract.as_ref()) {
        inv.remote()?.api().probe_override().await?;
    }

    dispatch(cli, &inv).await
}

async fn dispatch(cli: &Cli, inv: &Invocation) -> Result<(), anyhow::Error> {
    match &cli.command {
        Some(Commands::Wallet {
            subcommand: WalletCommands::Address,
        }) => {
            let signer = inv.resolve_signer()?;
            wallet::address(inv, &signer);
            Ok(())
        }
        Some(Commands::Wallet {
            subcommand: WalletCommands::Show,
        }) => {
            let signer = inv.resolve_signer()?;
            let remote = inv.remote()?;
            wallet::show(inv, remote.settings(), remote.selection(), &signer);
            Ok(())
        }
        Some(Commands::Perps {
            subcommand: PerpsCommands::List(args),
        }) => perps::list(inv, args).await,
        Some(Commands::Perps {
            subcommand: PerpsCommands::Get(args),
        }) => perps::get(inv, args).await,
        Some(Commands::Spot {
            subcommand: SpotCommands::List,
        }) => spot::list(inv).await,
        Some(Commands::Spot {
            subcommand: SpotCommands::Get { pair },
        }) => spot::get(inv, pair).await,
        Some(Commands::Asset {
            subcommand: AssetCommands::Decode(args),
        }) => asset::decode(inv, args).await,
        Some(Commands::Asset {
            subcommand: AssetCommands::Search(args),
        })
        | Some(Commands::Search(args)) => asset::search(inv, args).await,
        Some(Commands::Book { coin, watch }) => {
            if watch.watch {
                watch_book(inv, coin, watch.max_ticks).await
            } else {
                orderbook::book(inv, coin).await
            }
        }
        Some(Commands::Candles {
            coin,
            interval,
            limit,
            watch,
        }) => {
            if watch.watch {
                watch_candles(inv, coin, *interval, *limit, watch.max_ticks).await
            } else {
                orderbook::candles(inv, coin, *interval, *limit).await
            }
        }
        Some(Commands::Spread { coin }) => orderbook::spread(inv, coin).await,
        Some(Commands::Funding { coin }) => orderbook::funding(inv, coin).await,
        Some(Commands::Builder {
            subcommand: BuilderCommands::MaxFee(args),
        }) => {
            let user = inv.resolve_protocol_user(&args.user)?;
            let builder_address =
                hyperliquid_cli::commands::builder::parse_builder_address(&args.builder)?;
            hyperliquid_cli::commands::builder::max_fee(inv, user, builder_address).await
        }
        Some(Commands::Builder {
            subcommand: BuilderCommands::Approved(args),
        }) => {
            let user = inv.resolve_protocol_user(&args.user)?;
            hyperliquid_cli::commands::builder::approved(inv, user).await
        }
        Some(Commands::Builder {
            subcommand: BuilderCommands::Approve(args),
        }) => builder_approve(inv, args).await,
        Some(Commands::Outcomes {
            subcommand: OutcomeCommands::List(args),
        }) => outcomes::list(inv, args).await,
        Some(Commands::Outcomes {
            subcommand: OutcomeCommands::Get(args),
        }) => outcomes::get(inv, args).await,
        Some(Commands::Mids { watch }) => {
            if watch.watch {
                watch_mids(inv, watch.max_ticks).await
            } else {
                orderbook::mids(inv).await
            }
        }
        Some(Commands::Account {
            subcommand: AccountCommands::Fills(args),
        }) => {
            let address = resolve_user_string(inv, args.address.as_deref())?;
            account::fills(inv, &address, args).await
        }
        Some(Commands::Account {
            subcommand: AccountCommands::Fees(args),
        }) => {
            let address = resolve_user_string(inv, args.address.as_deref())?;
            account::fees(inv, &address).await
        }
        Some(Commands::Account {
            subcommand: AccountCommands::RateLimit(args),
        }) => {
            let address = resolve_user_string(inv, args.address.as_deref())?;
            account::rate_limit(inv, &address).await
        }
        Some(Commands::Account {
            subcommand: AccountCommands::Orders { address },
        }) => {
            let address = resolve_user_string(inv, address.as_deref())?;
            account::orders(inv, &address).await
        }
        Some(Commands::Account {
            subcommand: AccountCommands::Portfolio { address },
        }) => {
            let address = resolve_user_string(inv, address.as_deref())?;
            account::portfolio(inv, &address).await
        }
        Some(Commands::Account {
            subcommand: AccountCommands::Subaccounts { address },
        })
        | Some(Commands::Subaccount {
            subcommand: SubaccountCommands::List { address },
        }) => {
            let address = resolve_user_string(inv, address.as_deref())?;
            account::subaccounts(inv, &address).await
        }
        Some(Commands::ApiWallet {
            subcommand: ApiWalletCommands::Create(args),
        }) => api_wallet_create(inv, args).await,
        Some(Commands::ApiWallet {
            subcommand: ApiWalletCommands::Approve(args),
        }) => api_wallet_approve(inv, args).await,
        Some(Commands::ApiWallet {
            subcommand: ApiWalletCommands::List { account },
        }) => api_wallet_list(inv, account.as_deref()).await,
        Some(Commands::ApiWallet {
            subcommand: ApiWalletCommands::Revoke(args),
        }) => api_wallet_revoke(inv, args).await,
        Some(Commands::Account {
            subcommand: AccountCommands::PortfolioHistory(args),
        }) => {
            let address = resolve_user_string(inv, args.address.as_deref())?;
            account::portfolio_history(inv, &address).await
        }
        Some(Commands::Account {
            subcommand: AccountCommands::Ledger(args),
        }) => {
            let address = resolve_user_string(inv, args.address.as_deref())?;
            account::ledger(inv, &address, args).await
        }
        Some(Commands::Account {
            subcommand: AccountCommands::Funding(args),
        }) => {
            let address = resolve_user_string(inv, args.address.as_deref())?;
            account::funding(inv, &address, args).await
        }
        Some(Commands::Account {
            subcommand: AccountCommands::TwapHistory(args),
        }) => {
            let address = resolve_user_string(inv, args.address.as_deref())?;
            account::twap_history(inv, &address).await
        }
        Some(Commands::Account {
            subcommand: AccountCommands::TwapFills(args),
        }) => {
            let address = resolve_user_string(inv, args.address.as_deref())?;
            account::twap_fills(inv, &address, args).await
        }
        Some(Commands::Account {
            subcommand: AccountCommands::Abstraction(args),
        }) => account_abstraction(inv, args).await,
        Some(Commands::Subaccount {
            subcommand: SubaccountCommands::Create(args),
        }) => subaccount_create(inv, args).await,
        Some(Commands::Subaccount {
            subcommand: SubaccountCommands::Transfer(args),
        }) => subaccount_transfer(inv, args).await,
        Some(Commands::Subaccount {
            subcommand: SubaccountCommands::SpotTransfer(args),
        }) => subaccount_spot_transfer(inv, args).await,
        Some(Commands::Status) => status::show(inv).await,
        Some(Commands::Meta) => meta::show(inv).await,
        Some(Commands::Orders {
            subcommand: OrderCommands::Create(args),
        }) => create_order(inv, args).await,
        Some(Commands::Buy(args)) => {
            let create_args = args
                .clone()
                .into_create(hyperliquid_cli::commands::orders::OrderSide::Buy);
            create_order(inv, &create_args).await
        }
        Some(Commands::Sell(args)) => {
            let create_args = args
                .clone()
                .into_create(hyperliquid_cli::commands::orders::OrderSide::Sell);
            create_order(inv, &create_args).await
        }
        Some(Commands::OutcomeBuy(args)) => {
            ensure_outcome_coin(&args.inner.coin)?;
            let create_args = args
                .inner
                .clone()
                .into_create(hyperliquid_cli::commands::orders::OrderSide::Buy);
            create_order(inv, &create_args).await
        }
        Some(Commands::OutcomeSell(args)) => {
            ensure_outcome_coin(&args.inner.coin)?;
            let create_args = args
                .inner
                .clone()
                .into_create(hyperliquid_cli::commands::orders::OrderSide::Sell);
            create_order(inv, &create_args).await
        }
        Some(Commands::Orders {
            subcommand: OrderCommands::Scale(args),
        }) => scale_orders(inv, args).await,
        Some(Commands::Orders {
            subcommand: OrderCommands::BatchCreate(args),
        }) => batch_create_orders(inv, args).await,
        Some(Commands::Orders {
            subcommand: OrderCommands::Tpsl(args),
        }) => create_tpsl(inv, args).await,
        Some(Commands::Orders {
            subcommand: OrderCommands::TrailingStop(args),
        }) => trailing_stop_order(inv, args).await,
        Some(Commands::Orders {
            subcommand: OrderCommands::Cancel(args),
        }) => cancel_order(inv, args).await,
        Some(Commands::Orders {
            subcommand: OrderCommands::CancelAll(args),
        }) => cancel_all_orders(inv, args).await,
        Some(Commands::Orders {
            subcommand: OrderCommands::Modify(args),
        }) => modify_order(inv, args).await,
        Some(Commands::Orders {
            subcommand: OrderCommands::Open { watch },
        }) => {
            if watch.watch {
                watch_open_orders(inv, watch.max_ticks).await
            } else {
                open_orders(inv).await
            }
        }
        Some(Commands::Orders {
            subcommand: OrderCommands::History,
        }) => order_history(inv).await,
        Some(Commands::Orders {
            subcommand: OrderCommands::Status(args),
        }) => {
            let user = inv.resolve_protocol_user(&args.user)?;
            hyperliquid_cli::commands::orders::status(inv, user, args).await
        }
        Some(Commands::Orders {
            subcommand: OrderCommands::TwapCreate(args),
        }) => create_twap(inv, args).await,
        Some(Commands::Orders {
            subcommand: OrderCommands::TwapCancel(args),
        }) => cancel_twap(inv, args).await,
        Some(Commands::Orders {
            subcommand: OrderCommands::ScheduleCancel(args),
        }) => schedule_cancel(inv, args).await,
        Some(Commands::Orders {
            subcommand: OrderCommands::Chase(args),
        }) => chase_order(inv, args).await,
        Some(Commands::Orders {
            subcommand: OrderCommands::Bracket(args),
        }) => bracket_order(inv, args).await,
        Some(Commands::Watch {
            subcommand: WatchCommands::Risk(args),
        }) => watch_risk_command(inv, args).await,
        Some(Commands::Positions {
            subcommand: PositionCommands::List { watch },
        }) => {
            if watch.watch {
                watch_positions(inv, watch.max_ticks).await
            } else {
                let user = inv.resolve_signer()?.query_address();
                positions::list(inv, user).await
            }
        }
        Some(Commands::Positions {
            subcommand: PositionCommands::UpdateLeverage(args),
        }) => update_position_leverage(inv, args).await,
        Some(Commands::Positions {
            subcommand: PositionCommands::UpdateMargin(args),
        }) => update_position_margin(inv, args).await,
        Some(Commands::Transfer {
            subcommand: TransferCommands::SpotToPerp(args),
        }) => transfer_spot_to_perp(inv, args).await,
        Some(Commands::Transfer {
            subcommand: TransferCommands::PerpToSpot(args),
        }) => transfer_perp_to_spot(inv, args).await,
        Some(Commands::Transfer {
            subcommand: TransferCommands::Send(args),
        }) => transfer_send(inv, args).await,
        Some(Commands::Transfer {
            subcommand: TransferCommands::SpotSend(args),
        }) => transfer_spot_send(inv, args).await,
        Some(Commands::Transfer {
            subcommand: TransferCommands::SendAsset(args),
        }) => transfer_send_asset(inv, args).await,
        Some(Commands::Transfer {
            subcommand: TransferCommands::Withdraw(args),
        }) => transfer_withdraw(inv, args).await,
        Some(Commands::Setup(args)) => setup::run(inv, args).await,
        Some(Commands::Staking {
            subcommand: StakingCommands::Summary { address },
        }) => {
            let address = resolve_user_string(inv, address.as_deref())?;
            staking::summary(inv, &address).await
        }
        Some(Commands::Staking {
            subcommand: StakingCommands::Validators,
        }) => staking::validators(inv).await,
        Some(Commands::Staking {
            subcommand: StakingCommands::Rewards { address },
        }) => {
            let address = resolve_user_string(inv, address.as_deref())?;
            staking::rewards(inv, &address).await
        }
        Some(Commands::Staking {
            subcommand: StakingCommands::History { address },
        }) => {
            let address = resolve_user_string(inv, address.as_deref())?;
            staking::history(inv, &address).await
        }
        Some(Commands::Staking {
            subcommand: StakingCommands::Delegate(args),
        }) => staking_delegate(inv, args).await,
        Some(Commands::Staking {
            subcommand: StakingCommands::Undelegate(args),
        }) => staking_undelegate(inv, args).await,
        Some(Commands::Staking {
            subcommand: StakingCommands::Deposit(args),
        }) => staking_deposit(inv, args).await,
        Some(Commands::Staking {
            subcommand: StakingCommands::Withdraw(args),
        }) => staking_withdraw(inv, args).await,
        Some(Commands::Staking {
            subcommand: StakingCommands::ClaimRewards,
        }) => staking_claim_rewards(inv).await,
        Some(Commands::Staking {
            subcommand:
                StakingCommands::Link {
                    subcommand: StakingLinkCommands::Initiate(args),
                },
        }) => staking_link(inv, args, false).await,
        Some(Commands::Staking {
            subcommand:
                StakingCommands::Link {
                    subcommand: StakingLinkCommands::Finalize(args),
                },
        }) => staking_link(inv, args, true).await,
        Some(Commands::Vaults {
            subcommand: VaultCommands::List(args),
        }) => vaults::list(inv, args).await,
        Some(Commands::Vaults {
            subcommand: VaultCommands::Search(args),
        }) => vaults::search(inv, args).await,
        Some(Commands::Vaults {
            subcommand: VaultCommands::Get { address },
        }) => vaults::get(inv, address).await,
        Some(Commands::Vaults {
            subcommand: VaultCommands::Positions { address },
        }) => vaults::positions(inv, address).await,
        Some(Commands::Vaults {
            subcommand: VaultCommands::Deposit(args),
        }) => vault_deposit(inv, args).await,
        Some(Commands::Vaults {
            subcommand: VaultCommands::Withdraw(args),
        }) => vault_withdraw(inv, args).await,
        Some(Commands::Borrowlend {
            subcommand: BorrowLendCommands::Rates,
        }) => borrowlend::rates(inv).await,
        Some(Commands::Borrowlend {
            subcommand: BorrowLendCommands::Get { token },
        }) => borrowlend::get(inv, token).await,
        Some(Commands::Borrowlend {
            subcommand: BorrowLendCommands::User { address },
        }) => {
            let address = resolve_user_string(inv, address.as_deref())?;
            borrowlend::user(inv, &address).await
        }
        Some(Commands::Borrowlend {
            subcommand: BorrowLendCommands::Supply(args),
        }) => borrowlend_action(inv, borrowlend::BorrowLendOperation::Supply, args).await,
        Some(Commands::Borrowlend {
            subcommand: BorrowLendCommands::Withdraw(args),
        }) => borrowlend_action(inv, borrowlend::BorrowLendOperation::Withdraw, args).await,
        Some(Commands::Prio {
            subcommand: PrioCommands::Status,
        }) => prio::status(inv).await,
        Some(Commands::Prio {
            subcommand: PrioCommands::Bid(args),
        }) => prio_bid(inv, args).await,
        Some(Commands::Referral {
            subcommand: ReferralCommands::Set { code },
        }) => set_referral(inv, code.as_deref()).await,
        Some(Commands::Referral {
            subcommand: ReferralCommands::Register { code },
        }) => register_referrer(inv, code).await,
        Some(Commands::Referral {
            subcommand: ReferralCommands::Status,
        }) => {
            let user = inv.resolve_signer()?.query_address();
            referral::status(inv, user).await
        }
        Some(Commands::Subscribe { subcommand }) => subscribe(inv, subcommand).await,
        _ => {
            // Placeholder for all other unimplemented commands
            Err(unimplemented_command_error())
        }
    }
}

async fn run_local_command_without_context(
    cli: &Cli,
    inv: &Invocation,
) -> Option<Result<(), anyhow::Error>> {
    match &cli.command {
        Some(Commands::Schema { command_path }) => Some(schema::show(inv, command_path)),
        Some(Commands::Feedback(_)) if inv.dry_run() => Some(Err(errors::CliError::Unsupported(
            "--dry-run is not supported for feedback".to_string(),
        )
        .into())),
        Some(Commands::Feedback(args)) => {
            Some(hyperliquid_cli::commands::feedback::submit(inv, args).await)
        }
        Some(Commands::Wallet {
            subcommand: WalletCommands::Create,
        }) if inv.dry_run() => Some(print_dry_run(
            inv,
            "wallet create",
            serde_json::json!({"would_execute": "create_and_store_encrypted_wallet"}),
        )),
        Some(Commands::Wallet {
            subcommand: WalletCommands::Create,
        }) => Some(wallet::create(inv)),
        Some(Commands::Wallet {
            subcommand: WalletCommands::Import { private_key },
        }) if inv.dry_run() => Some(print_dry_run(
            inv,
            "wallet import",
            serde_json::json!({
                "would_execute": "validate_and_store_encrypted_wallet",
                "args": {"private_key_supplied": private_key.is_some()}
            }),
        )),
        Some(Commands::Wallet {
            subcommand: WalletCommands::Import { private_key },
        }) => Some(wallet::import(inv, private_key.as_deref())),
        Some(Commands::Wallet {
            subcommand: WalletCommands::Reset { yes },
        }) if inv.dry_run() => Some(print_dry_run(
            inv,
            "wallet reset",
            serde_json::json!({
                "would_execute": "remove_wallet_configuration",
                "args": {"yes": yes}
            }),
        )),
        Some(Commands::Wallet {
            subcommand: WalletCommands::Reset { yes },
        }) => Some(wallet::reset(inv, *yes)),
        Some(Commands::Wallet {
            subcommand: WalletCommands::ImportMnemonic { mnemonic, alias },
        }) if inv.dry_run() => Some(print_dry_run(
            inv,
            "wallet import-mnemonic",
            serde_json::json!({
                "would_execute": "import_mnemonic_to_ows_vault",
                "args": {"mnemonic_supplied": mnemonic.is_some(), "alias": alias}
            }),
        )),
        Some(Commands::Wallet {
            subcommand: WalletCommands::ImportMnemonic { mnemonic, alias },
        }) => Some(wallet::import_mnemonic(
            inv,
            mnemonic.as_deref(),
            alias.as_deref(),
        )),
        Some(Commands::Wallet {
            subcommand: WalletCommands::List,
        }) => Some(wallet::list(inv)),
        Some(Commands::Wallet {
            subcommand: WalletCommands::Rename { .. },
        }) if inv.dry_run() => Some(Err(errors::CliError::Unsupported(
            "--dry-run is not supported for wallet rename".to_string(),
        )
        .into())),
        Some(Commands::Wallet {
            subcommand: WalletCommands::Rename { selector, new_name },
        }) => Some(wallet::rename(inv, selector, new_name)),
        Some(Commands::Wallet {
            subcommand: WalletCommands::Delete { selector, yes },
        }) if inv.dry_run() => Some(print_dry_run(
            inv,
            "wallet delete",
            serde_json::json!({
                "would_execute": "delete_ows_wallet",
                "args": {"selector": selector, "yes": yes}
            }),
        )),
        Some(Commands::Wallet {
            subcommand: WalletCommands::Delete { selector, yes },
        }) => Some(wallet::delete(inv, selector, *yes)),
        Some(Commands::Wallet {
            subcommand: WalletCommands::Export { .. },
        }) if inv.dry_run() => Some(Err(errors::CliError::Unsupported(
            "--dry-run is not supported for wallet export".to_string(),
        )
        .into())),
        Some(Commands::Wallet {
            subcommand: WalletCommands::Export { selector, yes },
        }) => Some(wallet::export(inv, selector, *yes)),
        Some(Commands::Account {
            subcommand:
                AccountCommands::Add {
                    private_key,
                    alias,
                    account_type,
                    default,
                },
        }) if inv.dry_run() => Some(print_dry_run(
            inv,
            "account add",
            serde_json::json!({
                "would_execute": "store_encrypted_signing_account",
                "args": {
                    "private_key_supplied": private_key.is_some(),
                    "alias": alias,
                    "type": account_type,
                    "default": default
                }
            }),
        )),
        Some(Commands::Account {
            subcommand:
                AccountCommands::Add {
                    private_key,
                    alias,
                    account_type,
                    default,
                },
        }) => Some(wallet::account_add(
            inv,
            private_key.as_deref(),
            alias.as_deref(),
            account_type.as_deref(),
            *default,
        )),
        Some(Commands::Account {
            subcommand: AccountCommands::Ls,
        }) => Some(wallet::account_ls(inv)),
        Some(Commands::Account {
            subcommand: AccountCommands::SetDefault { selector },
        }) if inv.dry_run() => Some(print_dry_run(
            inv,
            "account set-default",
            serde_json::json!({
                "would_execute": "set_default_account",
                "args": {"selector": selector}
            }),
        )),
        Some(Commands::Account {
            subcommand: AccountCommands::SetDefault { selector },
        }) => Some(wallet::account_set_default(inv, selector.as_deref())),
        Some(Commands::Account {
            subcommand: AccountCommands::Remove { selector, yes },
        }) if inv.dry_run() => Some(print_dry_run(
            inv,
            "account remove",
            serde_json::json!({
                "would_execute": "remove_stored_account",
                "args": {"selector": selector, "yes": yes}
            }),
        )),
        Some(Commands::Account {
            subcommand: AccountCommands::Remove { selector, yes },
        }) => Some(wallet::account_remove(inv, selector.as_deref(), *yes)),
        _ => None,
    }
}

pub(super) fn command_contract_for_path(
    path: Option<&[String]>,
) -> Result<Option<CommandContract>, errors::CliError> {
    let Some(path) = path else {
        return Ok(None);
    };
    let registry = CommandRegistry::load().map_err(|err| {
        errors::CliError::Internal(anyhow::anyhow!("invalid command registry: {err}"))
    })?;
    let path_segments = path.iter().map(String::as_str).collect::<Vec<_>>();
    let command = registry.find_path(&path_segments).cloned().ok_or_else(|| {
        errors::CliError::Internal(anyhow::anyhow!(
            "command registry missing CLI path '{}'",
            path.join(" ")
        ))
    })?;
    Ok(Some(command))
}

pub(super) fn command_accepts_mutating_preview(command: Option<&CommandContract>) -> bool {
    command.is_some_and(|command| command.dry_run == DryRunPolicy::Optional)
}

fn validate_raw_payload_supported(
    command: Option<&CommandContract>,
) -> Result<(), errors::CliError> {
    let Some(command) = command else {
        return Err(errors::CliError::Unsupported(
            "payload input is not supported without a command".to_string(),
        ));
    };
    if command.raw_payload == RawPayloadPolicy::DryRunOnly {
        return Ok(());
    }
    Err(errors::CliError::Unsupported(format!(
        "payload input is not supported for {}",
        command.command_key()
    )))
}

fn validate_prompt_policy(
    cli: &Cli,
    inv: &Invocation,
    command: Option<&CommandContract>,
    confirmation: Confirmation,
) -> Result<(), errors::CliError> {
    let Some(command) = command else {
        return Ok(());
    };
    if machine_context(inv) && argv_secret_supplied(&cli.command) {
        return Err(errors::CliError::Unsupported(
            "argv secret input is not supported in machine-readable contexts; use an interactive prompt or a future safe stdin/file secret input"
                .to_string(),
        ));
    }
    if inv.dry_run()
        || inv.mode().can_prompt()
        || command.confirmation != ConfirmationPolicy::Prompt
        || !prompt_required_for_invocation(cli, command)
        || confirmation.given
        || confirmation_not_required(&cli.command)
    {
        return Ok(());
    }

    Err(confirmation_required_error(command, confirmation))
}

/// Name the blocked command, then give the safe next steps: a runnable
/// `--dry-run` preview when supported, and the `--yes` rerun only for use
/// after a human approves.
fn confirmation_required_error(
    command: &CommandContract,
    confirmation: Confirmation,
) -> errors::CliError {
    let args = replay_args(std::env::args().skip(1));
    let preview = (command.dry_run == DryRunPolicy::Optional).then(|| {
        let mut argv = vec!["--dry-run".to_string()];
        argv.extend(args.iter().cloned());
        replay_command(&argv)
    });
    let approve = confirmation.supported.then(|| {
        let mut argv = args.clone();
        argv.push("--yes".to_string());
        replay_command(&argv)
    });
    errors::CliError::ConfirmationRequired {
        headline: format!(
            "Can't run `{}` without confirmation in non-interactive mode.",
            command.command_key()
        ),
        preview,
        approve,
    }
}

/// Options whose values are secrets and never appear in suggested commands.
const SECRET_OPTIONS: &[&str] = &["--private-key", "--keystore-password"];

/// Invocation arguments (without the program name) minus secret options.
fn replay_args(args: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut args = args.into_iter();
    let mut kept = Vec::new();
    while let Some(arg) = args.next() {
        if SECRET_OPTIONS.contains(&arg.as_str()) {
            args.next();
            continue;
        }
        if SECRET_OPTIONS
            .iter()
            .any(|option| arg.starts_with(&format!("{option}=")))
        {
            continue;
        }
        kept.push(arg);
    }
    kept
}

/// `hyperliquid` plus `args`, single-quoted where a POSIX shell needs it.
fn replay_command(args: &[String]) -> String {
    let mut command = String::from("hyperliquid");
    for arg in args {
        command.push(' ');
        let plain = !arg.is_empty()
            && arg
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || "-_./:=@%+,#".contains(ch));
        if plain {
            command.push_str(arg);
        } else {
            command.push('\'');
            command.push_str(&arg.replace('\'', r"'\''"));
            command.push('\'');
        }
    }
    command
}

/// Commands that skip the mainnet confirmation when `--testnet` is set.
pub(super) const TESTNET_UNPROMPTED_COMMANDS: &[&str] = &[
    "orders create",
    "orders scale",
    "orders batch-create",
    "orders tpsl",
    "orders trailing-stop",
    "orders twap-create",
    "orders schedule-cancel",
    "orders chase",
    "orders bracket",
    "buy",
    "sell",
    "outcome-buy",
    "outcome-sell",
];

fn prompt_required_for_invocation(cli: &Cli, command: &CommandContract) -> bool {
    !(cli.testnet && TESTNET_UNPROMPTED_COMMANDS.contains(&command.command_key().as_str()))
}

fn argv_secret_supplied(command: &Option<Commands>) -> bool {
    match command {
        Some(Commands::Account {
            subcommand: AccountCommands::Add { private_key, .. },
        }) => private_key.is_some(),
        Some(Commands::Wallet {
            subcommand: WalletCommands::Import { private_key },
        }) => private_key.is_some(),
        Some(Commands::Wallet {
            subcommand: WalletCommands::ImportMnemonic { mnemonic, .. },
        }) => mnemonic.is_some(),
        _ => false,
    }
}

/// Commands whose confirmation prompt doesn't apply.
///
/// `account set-default` only switches a pointer. `account remove` without a
/// selector has nothing to confirm yet: its handler fails on the missing
/// selector (missing input) instead of prompting for one.
fn confirmation_not_required(command: &Option<Commands>) -> bool {
    matches!(
        command,
        Some(Commands::Account {
            subcommand: AccountCommands::SetDefault { .. }
                | AccountCommands::Remove { selector: None, .. },
        })
    )
}

pub(super) fn command_uses_api(command: Option<&CommandContract>) -> bool {
    command.is_some_and(|command| {
        command.group != "system"
            && !matches!(
                command.lifecycle,
                Lifecycle::InteractiveLocal | Lifecycle::BlockedUnsupported
            )
            && !matches!(command.risk, Risk::LocalSecret | Risk::LocalState)
    })
}

fn validate_cli_inputs(cli: &Cli) -> Result<(), errors::CliError> {
    use hyperliquid_cli::input_hardening::validate_resource_id;

    if let Some(account) = cli.account.as_deref() {
        validate_resource_id("account selector", account)?;
    }
    if let Some(path) = cli.payload_file.as_deref() {
        hyperliquid_cli::input_hardening::validate_input_path(path)?;
    }

    match &cli.command {
        Some(Commands::Schema { command_path }) => {
            for part in command_path {
                validate_resource_id("schema command path", part)?;
            }
        }
        Some(Commands::Perps { subcommand }) => match subcommand {
            PerpsCommands::List(args) => {
                if let Some(dex) = args.dex.as_deref() {
                    validate_resource_id("dex", dex)?;
                }
            }
            PerpsCommands::Get(args) => {
                validate_resource_id("coin", &args.coin)?;
                if let Some(dex) = args.dex.as_deref() {
                    validate_resource_id("dex", dex)?;
                }
            }
        },
        Some(Commands::Spot {
            subcommand: SpotCommands::Get { pair },
        }) => validate_resource_id("spot pair", pair)?,
        Some(Commands::Asset { subcommand }) => match subcommand {
            AssetCommands::Decode(_) => {}
            AssetCommands::Search(args) => {
                validate_asset_input("asset search query", &args.query)?;
            }
        },
        Some(Commands::Search(args)) => {
            validate_asset_input("asset search query", &args.query)?;
        }
        Some(Commands::Buy(args) | Commands::Sell(args)) => {
            validate_asset_input("coin", &args.coin)?;
            if let Some(dex) = args.dex.as_deref() {
                validate_resource_id("dex", dex)?;
            }
        }
        Some(Commands::OutcomeBuy(args) | Commands::OutcomeSell(args)) => {
            validate_asset_input("coin", &args.inner.coin)?;
        }
        Some(Commands::Watch {
            subcommand: WatchCommands::Risk(args),
        }) => {
            if let Some(user) = args.user.as_deref() {
                validate_resource_id("user", user)?;
            }
        }
        Some(Commands::Feedback(args)) => {
            if let Some(path) = args.scenario_file.as_deref()
                && path != "-"
            {
                hyperliquid_cli::input_hardening::validate_input_path(path)?;
            }
        }
        Some(Commands::Book { coin, .. })
        | Some(Commands::Candles { coin, .. })
        | Some(Commands::Spread { coin })
        | Some(Commands::Funding { coin }) => validate_resource_id("coin", coin)?,
        Some(Commands::Builder { subcommand }) => match subcommand {
            BuilderCommands::MaxFee(args) => {
                validate_resource_id("account selector", &args.user)?;
                validate_resource_id("builder address", &args.builder)?;
                hyperliquid_cli::commands::builder::parse_builder_address(&args.builder)?;
            }
            BuilderCommands::Approved(args) => {
                validate_resource_id("account selector", &args.user)?;
            }
            BuilderCommands::Approve(args) => {
                validate_resource_id("builder address", &args.builder)?;
                hyperliquid_cli::commands::builder::parse_builder_address(&args.builder)?;
                hyperliquid_cli::commands::builder::validate_max_fee_rate(&args.max_fee_rate)?;
            }
        },
        Some(Commands::Outcomes { subcommand }) => match subcommand {
            OutcomeCommands::List(_) => {}
            OutcomeCommands::Get(args) => {
                hyperliquid_cli::assets::asset_id::parse_outcome_notation(&args.notation)?;
            }
        },
        Some(Commands::Account { subcommand }) => match subcommand {
            AccountCommands::Fills(args) => {
                validate_optional_account_selector(args.address.as_deref())?
            }
            AccountCommands::Fees(args)
            | AccountCommands::RateLimit(args)
            | AccountCommands::PortfolioHistory(args)
            | AccountCommands::TwapHistory(args) => {
                validate_optional_account_selector(args.address.as_deref())?;
            }
            AccountCommands::Ledger(args) | AccountCommands::Funding(args) => {
                validate_optional_account_selector(args.address.as_deref())?;
            }
            AccountCommands::TwapFills(args) => {
                validate_optional_account_selector(args.address.as_deref())?
            }
            AccountCommands::Abstraction(args) => {
                validate_optional_account_selector(args.address.as_deref())?
            }
            AccountCommands::Orders { address } | AccountCommands::Subaccounts { address } => {
                validate_optional_account_selector(address.as_deref())?;
            }
            AccountCommands::Portfolio { address } => {
                validate_optional_account_selector(address.as_deref())?;
            }
            AccountCommands::Add { alias, .. } => {
                if let Some(alias) = alias.as_deref() {
                    validate_resource_id("account alias", alias)?;
                }
            }
            AccountCommands::SetDefault { selector } => {
                if let Some(selector) = selector.as_deref() {
                    validate_resource_id("account selector", selector)?;
                }
            }
            AccountCommands::Remove { selector, .. } => {
                if let Some(selector) = selector.as_deref() {
                    validate_resource_id("account selector", selector)?;
                }
            }
            AccountCommands::Ls => {}
        },
        Some(Commands::ApiWallet { subcommand }) => match subcommand {
            ApiWalletCommands::Create(args) => {
                validate_api_wallet_args(args.name.as_deref(), args.agent_address.as_deref())?;
            }
            ApiWalletCommands::Approve(args) => {
                validate_api_wallet_args(args.name.as_deref(), args.agent_address.as_deref())?;
            }
            ApiWalletCommands::List { account } => {
                if let Some(account) = account.as_deref() {
                    validate_resource_id("account selector", account)?;
                }
            }
            ApiWalletCommands::Revoke(args) => {
                validate_resource_id("API wallet name", &args.name)?;
            }
        },
        Some(Commands::Subaccount { subcommand }) => match subcommand {
            SubaccountCommands::List { address } => {
                validate_optional_account_selector(address.as_deref())?;
            }
            SubaccountCommands::Create(args) => {
                validate_resource_id("subaccount name", &args.name)?;
            }
            SubaccountCommands::Transfer(args) => {
                validate_resource_id("subaccount selector", &args.subaccount)?;
            }
            SubaccountCommands::SpotTransfer(args) => {
                validate_resource_id("subaccount selector", &args.subaccount)?;
                validate_resource_id("token", &args.token)?;
            }
        },
        Some(Commands::Orders { subcommand }) => match subcommand {
            OrderCommands::Create(args) => {
                validate_asset_input("coin", &args.coin)?;
                if let Some(dex) = args.dex.as_deref() {
                    validate_resource_id("dex", dex)?;
                }
                if let Some(on_behalf_of) = args.on_behalf_of.as_deref() {
                    validate_resource_id("on-behalf-of selector", on_behalf_of)?;
                }
                if let Some(builder) = args.builder.as_deref() {
                    validate_resource_id("builder address", builder)?;
                }
                if let Some(fee) = args.builder_fee_rate.as_deref() {
                    validate_resource_id("builder fee rate", fee)?;
                }
                if let Some(cloid) = args.cloid.as_deref() {
                    validate_resource_id("cloid", cloid)?;
                }
            }
            OrderCommands::Scale(args) => {
                validate_asset_input("coin", &args.coin)?;
                if let Some(dex) = args.dex.as_deref() {
                    validate_resource_id("dex", dex)?;
                }
                if let Some(on_behalf_of) = args.on_behalf_of.as_deref() {
                    validate_resource_id("on-behalf-of selector", on_behalf_of)?;
                }
            }
            OrderCommands::BatchCreate(args) => {
                hyperliquid_cli::input_hardening::validate_file_path(
                    &args.orders_file.to_string_lossy(),
                    hyperliquid_cli::input_hardening::FilePolicy::json_artifact("orders file"),
                )?;
                if let Some(on_behalf_of) = args.on_behalf_of.as_deref() {
                    validate_resource_id("on-behalf-of selector", on_behalf_of)?;
                }
            }
            OrderCommands::Tpsl(args) => {
                validate_asset_input("coin", &args.coin)?;
                if let Some(dex) = args.dex.as_deref() {
                    validate_resource_id("dex", dex)?;
                }
                if let Some(on_behalf_of) = args.on_behalf_of.as_deref() {
                    validate_resource_id("on-behalf-of selector", on_behalf_of)?;
                }
                if let Some(cloid) = args.cloid.as_deref() {
                    validate_resource_id("cloid", cloid)?;
                }
            }
            OrderCommands::TrailingStop(args) => {
                validate_asset_input("coin", &args.coin)?;
                if let Some(dex) = args.dex.as_deref() {
                    validate_resource_id("dex", dex)?;
                }
                if let Some(on_behalf_of) = args.on_behalf_of.as_deref() {
                    validate_resource_id("on-behalf-of selector", on_behalf_of)?;
                }
            }
            OrderCommands::Cancel(args) => {
                if let Some(on_behalf_of) = args.on_behalf_of.as_deref() {
                    validate_resource_id("on-behalf-of selector", on_behalf_of)?;
                }
                if let Some(cloid) = args.cloid.as_deref() {
                    validate_resource_id("cloid", cloid)?;
                }
            }
            OrderCommands::Modify(args) => {
                if let Some(on_behalf_of) = args.on_behalf_of.as_deref() {
                    validate_resource_id("on-behalf-of selector", on_behalf_of)?;
                }
                if let Some(cloid) = args.cloid.as_deref() {
                    validate_resource_id("cloid", cloid)?;
                }
            }
            OrderCommands::CancelAll(args) => {
                if let Some(coin) = args.coin.as_deref() {
                    validate_asset_input("coin", coin)?;
                }
                if let Some(dex) = args.dex.as_deref() {
                    validate_resource_id("dex", dex)?;
                }
                if let Some(on_behalf_of) = args.on_behalf_of.as_deref() {
                    validate_resource_id("on-behalf-of selector", on_behalf_of)?;
                }
            }
            OrderCommands::TwapCreate(args) => {
                validate_asset_input("coin", &args.coin)?;
                if let Some(dex) = args.dex.as_deref() {
                    validate_resource_id("dex", dex)?;
                }
                if let Some(on_behalf_of) = args.on_behalf_of.as_deref() {
                    validate_resource_id("on-behalf-of selector", on_behalf_of)?;
                }
            }
            OrderCommands::TwapCancel(args) => {
                validate_asset_input("coin", &args.coin)?;
                if let Some(dex) = args.dex.as_deref() {
                    validate_resource_id("dex", dex)?;
                }
                if let Some(on_behalf_of) = args.on_behalf_of.as_deref() {
                    validate_resource_id("on-behalf-of selector", on_behalf_of)?;
                }
            }
            OrderCommands::ScheduleCancel(args) => {
                if let Some(on_behalf_of) = args.on_behalf_of.as_deref() {
                    validate_resource_id("on-behalf-of selector", on_behalf_of)?;
                }
            }
            OrderCommands::Chase(args) => {
                validate_asset_input("coin", &args.coin)?;
            }
            OrderCommands::Bracket(args) => {
                validate_asset_input("coin", &args.coin)?;
            }
            OrderCommands::Status(args) => {
                validate_resource_id("account selector", &args.user)?;
                if let Some(cloid) = args.cloid.as_deref() {
                    validate_resource_id("cloid", cloid)?;
                }
            }
            _ => {}
        },
        Some(Commands::Positions { subcommand }) => match subcommand {
            PositionCommands::UpdateLeverage(args) => validate_resource_id("coin", &args.coin)?,
            PositionCommands::UpdateMargin(args) => validate_resource_id("coin", &args.coin)?,
            PositionCommands::List { .. } => {}
        },
        Some(Commands::Transfer { subcommand }) => match subcommand {
            TransferCommands::Send(args) => validate_resource_id("destination address", &args.to)?,
            TransferCommands::SpotSend(args) => {
                validate_resource_id("destination address", &args.to)?;
                validate_resource_id("token", &args.token)?;
            }
            TransferCommands::SendAsset(args) => {
                validate_resource_id("destination address", &args.to)?;
                validate_resource_id("token", &args.token)?;
                validate_resource_id("source asset target", &args.source)?;
                validate_resource_id("destination asset target", &args.dest)?;
                if let Some(from_subaccount) = args.from_subaccount.as_deref() {
                    validate_resource_id("from subaccount address", from_subaccount)?;
                }
            }
            TransferCommands::Withdraw(args) => {
                validate_resource_id("destination address", &args.to)?
            }
            _ => {}
        },
        Some(Commands::Staking { subcommand }) => match subcommand {
            StakingCommands::Summary { address }
            | StakingCommands::Rewards { address }
            | StakingCommands::History { address } => {
                validate_optional_account_selector(address.as_deref())?;
            }
            StakingCommands::Delegate(args) | StakingCommands::Undelegate(args) => {
                validate_resource_id("validator address", &args.validator)?
            }
            StakingCommands::Link { subcommand } => match subcommand {
                StakingLinkCommands::Initiate(args) | StakingLinkCommands::Finalize(args) => {
                    validate_resource_id("staking-link user address", &args.user)?
                }
            },
            _ => {}
        },
        Some(Commands::Vaults { subcommand }) => match subcommand {
            VaultCommands::List(_) => {}
            VaultCommands::Search(_) => {}
            VaultCommands::Get { address } | VaultCommands::Positions { address } => {
                validate_resource_id("vault address", address)?
            }
            VaultCommands::Deposit(args) | VaultCommands::Withdraw(args) => {
                validate_resource_id("vault address", &args.vault)?
            }
        },
        Some(Commands::Borrowlend {
            subcommand: BorrowLendCommands::Get { token },
        }) => validate_resource_id("token", token)?,
        Some(Commands::Borrowlend {
            subcommand: BorrowLendCommands::User { address },
        }) => validate_optional_account_selector(address.as_deref())?,
        Some(Commands::Borrowlend {
            subcommand: BorrowLendCommands::Supply(args) | BorrowLendCommands::Withdraw(args),
        }) => validate_resource_id("token", &args.token)?,
        Some(Commands::Prio {
            subcommand: PrioCommands::Bid(args),
        }) => validate_resource_id("validator ip", &args.ip)?,
        Some(Commands::Referral {
            subcommand: ReferralCommands::Set { code: Some(code) },
        }) => validate_resource_id("referral code", code)?,
        Some(Commands::Referral {
            subcommand: ReferralCommands::Register { code },
        }) => validate_resource_id("referrer code", code)?,
        Some(Commands::Subscribe {
            subcommand:
                SubscribeCommands::Trades { asset, .. }
                | SubscribeCommands::Orderbook { asset, .. }
                | SubscribeCommands::Candles { asset, .. },
        }) => validate_resource_id("asset", asset)?,
        _ => {}
    }
    Ok(())
}

fn validate_stream_bounds(cli: &Cli, inv: &Invocation) -> Result<(), errors::CliError> {
    if !machine_context(inv) {
        return Ok(());
    }
    if let Some(watch) = snapshot_watch_args(&cli.command).filter(|watch| watch.watch) {
        hyperliquid_cli::stream::require_tick_bound(watch.max_ticks)?;
    }
    if let Some(stream) = subscribe_stream_args(&cli.command) {
        hyperliquid_cli::stream::require_event_bound(stream)?;
    }
    Ok(())
}

fn machine_context(inv: &Invocation) -> bool {
    inv.format() == output::OutputFormat::Json || inv.session().env().agent
}

fn snapshot_watch_args(command: &Option<Commands>) -> Option<&SnapshotWatchArgs> {
    match command {
        Some(Commands::Book { watch, .. })
        | Some(Commands::Mids { watch })
        | Some(Commands::Candles { watch, .. })
        | Some(Commands::Orders {
            subcommand: OrderCommands::Open { watch },
        })
        | Some(Commands::Positions {
            subcommand: PositionCommands::List { watch },
        }) => Some(watch),
        _ => None,
    }
}

fn subscribe_stream_args(command: &Option<Commands>) -> Option<&StreamBoundArgs> {
    match command {
        Some(Commands::Subscribe { subcommand }) => match subcommand {
            SubscribeCommands::Trades { stream, .. }
            | SubscribeCommands::Orderbook { stream, .. }
            | SubscribeCommands::Candles { stream, .. }
            | SubscribeCommands::AllMids { stream }
            | SubscribeCommands::OrderUpdates { stream }
            | SubscribeCommands::Fills { stream } => Some(stream),
        },
        _ => None,
    }
}

fn validate_asset_input(label: &str, value: &str) -> Result<(), errors::CliError> {
    use hyperliquid_cli::input_hardening::validate_resource_id;

    if matches!(parse_asset_query(value), AssetQuery::Outcome(_)) {
        return Ok(());
    }
    validate_resource_id(label, value)
}

fn validate_optional_account_selector(value: Option<&str>) -> Result<(), errors::CliError> {
    use hyperliquid_cli::input_hardening::validate_resource_id;

    if let Some(value) = value {
        validate_resource_id("account selector", value)?;
    }
    Ok(())
}

fn validate_api_wallet_args(
    name: Option<&str>,
    agent_address: Option<&str>,
) -> Result<(), errors::CliError> {
    use hyperliquid_cli::commands::api_wallet::MAX_AGENT_NAME_LEN;
    use hyperliquid_cli::input_hardening::validate_resource_id;

    if let Some(name) = name {
        validate_resource_id("API wallet name", name)?;
        let trimmed = name.trim();
        if !trimmed.is_empty() && trimmed.chars().count() > MAX_AGENT_NAME_LEN {
            return Err(errors::CliError::Configuration(format!(
                "--name must be between 1 and {MAX_AGENT_NAME_LEN} characters"
            )));
        }
    }
    if let Some(agent_address) = agent_address {
        validate_resource_id("agent address", agent_address)?;
    }
    Ok(())
}

fn print_dry_run(
    inv: &Invocation,
    command: impl Into<String>,
    details: serde_json::Value,
) -> Result<(), anyhow::Error> {
    print_dry_run_envelope(inv, DryRunEnvelope::from_details(command, details))
}

fn print_dry_run_envelope(inv: &Invocation, envelope: DryRunEnvelope) -> Result<(), anyhow::Error> {
    inv.print_dry_run(envelope);
    Ok(())
}

fn dry_run_details(
    inv: &Invocation,
    would_execute: &str,
    args: serde_json::Value,
) -> serde_json::Value {
    let mut details = serde_json::Map::new();
    details.insert(
        "would_execute".to_string(),
        serde_json::json!(would_execute),
    );
    details.insert("args".to_string(), args);
    if let Some(payload) = inv.payload() {
        details.insert("payload".to_string(), redact_payload(payload.clone()));
    }
    serde_json::Value::Object(details)
}

fn dry_run_signed_details(
    inv: &Invocation,
    would_execute: &str,
    args: serde_json::Value,
    signer: Option<String>,
    acting_as: Option<String>,
    vault_address: Option<String>,
) -> serde_json::Value {
    let mut details = match dry_run_details(inv, would_execute, args) {
        serde_json::Value::Object(details) => details,
        _ => serde_json::Map::new(),
    };
    details.insert("signer".to_string(), serde_json::json!(signer));
    details.insert("acting_as".to_string(), serde_json::json!(acting_as));
    details.insert(
        "vault_address".to_string(),
        serde_json::json!(vault_address),
    );
    serde_json::Value::Object(details)
}

fn transfer_dry_run_envelope(
    inv: &Invocation,
    command: &str,
    action: hyperliquid_cli::commands::transfers::TransferActionKind,
    would_execute: &str,
    args: serde_json::Value,
) -> DryRunEnvelope {
    let plan = ActionPlan::signed_exchange_action(
        would_execute,
        action.reversibility(),
        LiveSubmissionPolicy::ValidateConfirmSignSubmit,
    );
    DryRunEnvelope::from_action_plan(command, &plan, args)
        .with_payload(inv.payload().map(|payload| redact_payload(payload.clone())))
}

fn signed_action_dry_run_envelope(
    inv: &Invocation,
    command: &str,
    would_execute: &str,
    reversibility: ActionReversibility,
    args: serde_json::Value,
    signing_context: DryRunSigningContext,
) -> DryRunEnvelope {
    let plan = ActionPlan::signed_exchange_action(
        would_execute,
        reversibility,
        LiveSubmissionPolicy::ValidateConfirmSignSubmit,
    );
    DryRunEnvelope::from_action_plan(command, &plan, args)
        .with_payload(inv.payload().map(|payload| redact_payload(payload.clone())))
        .with_signing_context(signing_context)
}

fn dry_run_signer_address(inv: &Invocation) -> Option<String> {
    dry_run_signing_addresses(inv).0
}

fn dry_run_signing_addresses(inv: &Invocation) -> (Option<String>, Option<String>) {
    inv.resolve_signer()
        .ok()
        .map(|resolved| {
            (
                Some(resolved.address().to_string()),
                Some(resolved.query_address().to_string()),
            )
        })
        .unwrap_or((None, None))
}

fn dry_run_vault_signing_addresses(
    inv: &Invocation,
    vault_address: Option<String>,
) -> (Option<String>, Option<String>, Option<String>) {
    let (signer, query_address) = dry_run_signing_addresses(inv);
    let acting_as = vault_address.clone().or(query_address);
    (signer, acting_as, vault_address)
}

fn default_dry_run_signing_context(inv: &Invocation) -> DryRunSigningContext {
    let (signer, acting_as) = dry_run_signing_addresses(inv);
    DryRunSigningContext::new(signer, acting_as, None)
}

fn redact_payload(value: serde_json::Value) -> serde_json::Value {
    hyperliquid_cli::input_hardening::redact_sensitive_json(value)
}

fn read_payload_json(cli: &Cli) -> Result<Option<serde_json::Value>, anyhow::Error> {
    let Some(raw) = cli.payload_json.as_deref() else {
        let Some(path) = cli.payload_file.as_deref() else {
            return Ok(None);
        };
        return hyperliquid_cli::input_hardening::read_json_file(
            std::path::Path::new(path),
            hyperliquid_cli::input_hardening::FilePolicy::payload(),
        )
        .map(Some)
        .map_err(Into::into);
    };
    parse_payload_json(raw).map(Some)
}

fn parse_payload_json(raw: &str) -> Result<serde_json::Value, anyhow::Error> {
    hyperliquid_cli::input_hardening::parse_json_text(raw, "payload").map_err(Into::into)
}

fn order_submission_context<'a>(
    inv: &'a Invocation,
    signer: &'a hyperliquid_cli::signing::SelectedSigner,
) -> Result<hyperliquid_cli::commands::orders::OrderSubmissionContext<'a>, errors::CliError> {
    let remote = inv.remote()?;
    Ok(hyperliquid_cli::commands::orders::OrderSubmissionContext {
        inv,
        api: remote.api(),
        signer,
        require_mainnet_confirmation: remote.network() == Network::Mainnet,
    })
}

fn order_execution_context<'a>(
    inv: &'a Invocation,
    resolver: &'a AssetResolver,
    signer: &'a hyperliquid_cli::signing::SelectedSigner,
) -> Result<hyperliquid_cli::commands::orders::OrderExecutionContext<'a>, errors::CliError> {
    Ok(hyperliquid_cli::commands::orders::OrderExecutionContext {
        submission: order_submission_context(inv, signer)?,
        resolver,
    })
}

async fn watch_book(
    inv: &Invocation,
    coin: &str,
    max_ticks: Option<usize>,
) -> Result<(), anyhow::Error> {
    let resolver = std::sync::Arc::new(inv.asset_resolver().await?);
    let subscription_coin = resolve_subscription_coin(&resolver, coin)?;
    let chain = inv.remote()?.chain();

    hyperliquid_cli::watch::run_snapshot_watch(
        inv.session(),
        &format!("Live order book: {subscription_coin}"),
        chain,
        vec![Subscription::L2Book {
            coin: subscription_coin,
            n_sig_figs: None,
            mantissa: None,
            fast: false,
        }],
        max_ticks,
        move || {
            let resolver = std::sync::Arc::clone(&resolver);
            async move {
                hyperliquid_cli::commands::orderbook::book_snapshot(chain, &resolver, coin).await
            }
        },
    )
    .await
}

async fn watch_mids(inv: &Invocation, max_ticks: Option<usize>) -> Result<(), anyhow::Error> {
    let remote = inv.remote()?;
    let client = remote.api().sdk();
    let selected = inv.session().json_options().selected_fields();

    hyperliquid_cli::watch::run_snapshot_watch(
        inv.session(),
        "Live mid prices",
        remote.chain(),
        vec![Subscription::AllMids { dex: None }],
        max_ticks,
        move || async move {
            hyperliquid_cli::commands::orderbook::mids_snapshot(client, selected).await
        },
    )
    .await
}

async fn watch_candles(
    inv: &Invocation,
    coin: &str,
    interval: hypersdk::hypercore::CandleInterval,
    limit: usize,
    max_ticks: Option<usize>,
) -> Result<(), anyhow::Error> {
    let remote = inv.remote()?;
    let client = remote.api().sdk();
    let resolver = std::sync::Arc::new(inv.asset_resolver().await?);
    let subscription_coin = resolve_subscription_coin(&resolver, coin)?;

    hyperliquid_cli::watch::run_snapshot_watch(
        inv.session(),
        &format!("Live candles: {subscription_coin} {interval}"),
        remote.chain(),
        vec![Subscription::Candle {
            coin: subscription_coin,
            interval: interval.to_string(),
        }],
        max_ticks,
        move || {
            let resolver = std::sync::Arc::clone(&resolver);
            async move {
                hyperliquid_cli::commands::orderbook::candles_snapshot(
                    client, &resolver, coin, interval, limit,
                )
                .await
            }
        },
    )
    .await
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

    hyperliquid_cli::watch::run_snapshot_watch(
        inv.session(),
        "Live open orders",
        remote.chain(),
        vec![Subscription::OrderUpdates { user }],
        max_ticks,
        move || {
            let resolver = std::sync::Arc::clone(&resolver);
            async move {
                hyperliquid_cli::commands::orders::open_snapshot(client, &resolver, user).await
            }
        },
    )
    .await
}

async fn watch_positions(inv: &Invocation, max_ticks: Option<usize>) -> Result<(), anyhow::Error> {
    let remote = inv.remote()?;
    let client = remote.api().sdk();
    let resolved = inv.resolve_signer()?;
    let user = resolved.query_address();

    hyperliquid_cli::watch::run_snapshot_watch(
        inv.session(),
        "Live positions",
        remote.chain(),
        vec![Subscription::WebData3 { user }],
        max_ticks,
        move || async move { hyperliquid_cli::commands::positions::list_snapshot(client, user).await },
    )
    .await
}

async fn subscribe(inv: &Invocation, command: &SubscribeCommands) -> Result<(), anyhow::Error> {
    let (target, stream) = match command {
        SubscribeCommands::Trades { asset, stream } => {
            let resolver = inv.asset_resolver().await?;
            let coin = resolve_subscription_coin(&resolver, asset)?;
            (SubscribeTarget::trades(coin), stream)
        }
        SubscribeCommands::Orderbook { asset, stream } => {
            let resolver = inv.asset_resolver().await?;
            let coin = resolve_subscription_coin(&resolver, asset)?;
            (SubscribeTarget::orderbook(coin), stream)
        }
        SubscribeCommands::Candles {
            asset,
            interval,
            stream,
        } => {
            hyperliquid_cli::commands::orderbook::parse_candle_interval(interval)
                .map_err(errors::CliError::Configuration)?;
            let resolver = inv.asset_resolver().await?;
            let coin = resolve_subscription_coin(&resolver, asset)?;
            (SubscribeTarget::candles(coin, interval.clone()), stream)
        }
        SubscribeCommands::AllMids { stream } => (SubscribeTarget::all_mids(), stream),
        SubscribeCommands::OrderUpdates { stream } => {
            let user = inv.resolve_signer()?.query_address();
            (SubscribeTarget::order_updates(user), stream)
        }
        SubscribeCommands::Fills { stream } => {
            let user = inv.resolve_signer()?.query_address();
            (SubscribeTarget::fills(user), stream)
        }
    };
    hyperliquid_cli::stream::stream_subscription(
        inv.session(),
        inv.remote()?.chain(),
        target,
        StreamBounds::subscribe(stream),
    )
    .await
}

fn resolve_subscription_coin(
    resolver: &AssetResolver,
    asset: &str,
) -> Result<String, errors::CliError> {
    hyperliquid_cli::commands::orderbook::resolve_subscription_info_coin(resolver, asset)
}

/// The account a read command queries: the explicit selector, else the signer's.
fn resolve_user_string(inv: &Invocation, selector: Option<&str>) -> Result<String, anyhow::Error> {
    Ok(inv.resolve_user(selector)?.to_string())
}

fn resolve_optional_acting_account_target(
    inv: &Invocation,
    selector: Option<&str>,
) -> Result<Option<Address>, anyhow::Error> {
    selector
        .map(|selector| inv.resolve_acting_account(selector))
        .transpose()
}

async fn api_wallet_create(
    inv: &Invocation,
    args: &hyperliquid_cli::commands::api_wallet::CreateArgs,
) -> Result<(), anyhow::Error> {
    let signer = api_wallet_approval_signer(inv)?;
    hyperliquid_cli::commands::api_wallet::create(inv, signer.as_ref(), args).await
}

async fn api_wallet_approve(
    inv: &Invocation,
    args: &hyperliquid_cli::commands::api_wallet::ApproveArgs,
) -> Result<(), anyhow::Error> {
    let signer = api_wallet_approval_signer(inv)?;
    hyperliquid_cli::commands::api_wallet::approve(inv, signer.as_ref(), args).await
}

async fn api_wallet_list(inv: &Invocation, account: Option<&str>) -> Result<(), anyhow::Error> {
    let master_address = match account {
        Some(account) => inv.resolve_protocol_user(account)?,
        None => inv.resolve_signer()?.query_address(),
    };
    hyperliquid_cli::commands::api_wallet::list(inv, master_address).await
}

async fn api_wallet_revoke(
    inv: &Invocation,
    args: &hyperliquid_cli::commands::api_wallet::RevokeArgs,
) -> Result<(), anyhow::Error> {
    if args.name.chars().count() > hyperliquid_cli::commands::api_wallet::MAX_AGENT_NAME_LEN {
        return Err(errors::CliError::Configuration(format!(
            "--name must be between 1 and {} characters",
            hyperliquid_cli::commands::api_wallet::MAX_AGENT_NAME_LEN
        ))
        .into());
    }
    let signer = api_wallet_approval_signer(inv)?;
    hyperliquid_cli::commands::api_wallet::revoke(inv, signer.as_ref(), args).await
}

fn api_wallet_approval_signer(
    inv: &Invocation,
) -> Result<Option<hyperliquid_cli::signing::SelectedSigner>, anyhow::Error> {
    if inv.dry_run() {
        return Ok(None);
    }
    let resolved = inv.resolve_signer()?;
    if resolved.query_address() != resolved.address() {
        return Err(errors::CliError::Unsupported(
            "approveAgent must be signed by the master account; select the master wallet, not a stored API wallet"
                .to_string(),
        )
        .into());
    }
    Ok(Some(resolved.selected_signer()))
}

async fn builder_approve(
    inv: &Invocation,
    args: &hyperliquid_cli::commands::builder::ApproveArgs,
) -> Result<(), anyhow::Error> {
    let builder = hyperliquid_cli::commands::builder::parse_builder_address(&args.builder)?;
    hyperliquid_cli::commands::builder::validate_max_fee_rate(&args.max_fee_rate)?;
    if inv.dry_run() {
        let signer_context = inv.resolve_signer().ok();
        let preview_args = hyperliquid_cli::commands::builder::approve_dry_run_value(
            inv.remote()?.chain(),
            signer_context.as_ref().map(|resolved| resolved.address()),
            signer_context
                .as_ref()
                .map(|resolved| resolved.query_address()),
            builder,
            args,
        )?;
        let envelope = signed_action_dry_run_envelope(
            inv,
            "builder approve",
            "approve_builder_fee",
            hyperliquid_cli::commands::builder::BuilderActionKind::ApproveFee.reversibility(),
            preview_args,
            default_dry_run_signing_context(inv),
        );
        return print_dry_run_envelope(inv, envelope);
    }
    let resolved = inv.resolve_signer()?;
    if resolved.query_address() != resolved.address() {
        return Err(errors::CliError::Unsupported(
            "approveBuilderFee must be signed by the master account; select the master wallet, not a stored API wallet"
                .to_string(),
        )
        .into());
    }
    hyperliquid_cli::commands::builder::approve(
        inv,
        &resolved.selected_signer(),
        builder,
        args,
        true,
    )
    .await
}

async fn account_abstraction(
    inv: &Invocation,
    args: &hyperliquid_cli::commands::account::AbstractionArgs,
) -> Result<(), anyhow::Error> {
    use hyperliquid_cli::commands::account::{
        AbstractionCommand, abstraction, abstraction_set_dry_run_value, set_abstraction,
    };

    match args.command.as_ref() {
        Some(AbstractionCommand::Set(set_args)) => {
            if inv.dry_run() {
                let signer_address = inv.resolve_signer().ok().map(|r| r.address());
                let fallback_address: Address = "0x0000000000000000000000000000000000000001"
                    .parse()
                    .expect("valid placeholder address");
                let preview_args = abstraction_set_dry_run_value(
                    inv.remote()?.chain(),
                    signer_address.unwrap_or(fallback_address),
                    set_args,
                );
                let signer = dry_run_signer_address(inv);
                return print_dry_run(
                    inv,
                    "account abstraction set",
                    dry_run_signed_details(
                        inv,
                        "set_abstraction",
                        preview_args,
                        signer,
                        None,
                        None,
                    ),
                );
            }
            let resolved = inv.resolve_signer()?;
            if resolved.query_address() != resolved.address() {
                return Err(errors::CliError::Unsupported(
                    "userSetAbstraction must be signed by the master account; select the master wallet, not a stored API wallet"
                        .to_string(),
                )
                .into());
            }
            set_abstraction(inv, &resolved.selected_signer(), set_args, true).await
        }
        None => {
            let address = resolve_user_string(inv, args.address.as_deref())?;
            abstraction(inv, &address).await
        }
    }
}

async fn subaccount_create(
    inv: &Invocation,
    args: &hyperliquid_cli::commands::subaccounts::CreateArgs,
) -> Result<(), anyhow::Error> {
    hyperliquid_cli::commands::subaccounts::validate_create_args(args)?;
    if inv.dry_run() {
        let envelope = signed_action_dry_run_envelope(
            inv,
            "subaccount create",
            "create_subaccount",
            hyperliquid_cli::commands::subaccounts::SubaccountActionKind::Create.reversibility(),
            hyperliquid_cli::commands::subaccounts::create_dry_run_value(
                inv.remote()?.chain(),
                args,
            ),
            default_dry_run_signing_context(inv),
        );
        return print_dry_run_envelope(inv, envelope);
    }
    let resolved = inv.resolve_signer()?;
    hyperliquid_cli::commands::subaccounts::create(inv, &resolved.selected_signer(), args).await
}

async fn subaccount_transfer(
    inv: &Invocation,
    args: &hyperliquid_cli::commands::subaccounts::TransferArgs,
) -> Result<(), anyhow::Error> {
    hyperliquid_cli::commands::subaccounts::validate_transfer_args(args)?;
    let subaccount = inv.resolve_acting_account(&args.subaccount)?;
    if inv.dry_run() {
        let envelope = signed_action_dry_run_envelope(
            inv,
            "subaccount transfer",
            "subaccount_usdc_transfer",
            hyperliquid_cli::commands::subaccounts::SubaccountActionKind::TransferUsdc
                .reversibility(),
            hyperliquid_cli::commands::subaccounts::transfer_dry_run_value(
                inv.remote()?.chain(),
                subaccount,
                args,
            )?,
            default_dry_run_signing_context(inv),
        );
        return print_dry_run_envelope(inv, envelope);
    }
    let resolved = inv.resolve_signer()?;
    hyperliquid_cli::commands::subaccounts::transfer(
        inv,
        &resolved.selected_signer(),
        subaccount,
        args,
    )
    .await
}

async fn subaccount_spot_transfer(
    inv: &Invocation,
    args: &hyperliquid_cli::commands::subaccounts::SpotTransferArgs,
) -> Result<(), anyhow::Error> {
    hyperliquid_cli::commands::subaccounts::validate_spot_transfer_args(args)?;
    let subaccount = inv.resolve_acting_account(&args.subaccount)?;
    if inv.dry_run() {
        let envelope = signed_action_dry_run_envelope(
            inv,
            "subaccount spot-transfer",
            "subaccount_spot_transfer",
            hyperliquid_cli::commands::subaccounts::SubaccountActionKind::TransferSpot
                .reversibility(),
            hyperliquid_cli::commands::subaccounts::spot_transfer_dry_run_value(
                inv.remote()?.chain(),
                subaccount,
                args,
            )?,
            default_dry_run_signing_context(inv),
        );
        return print_dry_run_envelope(inv, envelope);
    }
    let resolved = inv.resolve_signer()?;
    hyperliquid_cli::commands::subaccounts::spot_transfer(
        inv,
        &resolved.selected_signer(),
        subaccount,
        args,
    )
    .await
}

async fn create_order(
    inv: &Invocation,
    args: &hyperliquid_cli::commands::orders::CreateArgs,
) -> Result<(), anyhow::Error> {
    hyperliquid_cli::commands::orders::validate_create_args(args)?;
    let client = inv.remote()?.api().sdk();
    let resolver = inv.trading_resolver().await?;
    let coin_query = qualify_dex_asset(args.dex.as_deref(), &args.coin);
    let vault_address = resolve_optional_acting_account_target(inv, args.on_behalf_of.as_deref())?;
    let vault_address_string = vault_address.map(|address| address.to_string());
    if inv.dry_run() {
        let plan =
            hyperliquid_cli::commands::orders::create_dry_run_plan(client, &resolver, args).await?;
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
                .map_err(errors::CliError::from)?;
            hyperliquid_cli::commands::orders::validate_create_resolved_asset(args, &resolved)?;
            match resolved {
                Market::Perp(perp) => Some((perp.name, perp.dex)),
                Market::Spot(_) | Market::Outcome(_) => None,
            }
        } else if args.margin_mode.is_some() {
            return Err(errors::CliError::Configuration(
                "orders create --margin-mode is only supported for perpetual orders".to_string(),
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
    hyperliquid_cli::commands::orders::create(
        order_execution_context(inv, &resolver, &signer)?,
        args,
        vault_address,
        true,
    )
    .await
}

async fn scale_orders(
    inv: &Invocation,
    args: &hyperliquid_cli::commands::orders::ScaleArgs,
) -> Result<(), anyhow::Error> {
    hyperliquid_cli::commands::orders::validate_scale_args(args)?;
    let client = inv.remote()?.api().sdk();
    let resolver = inv.trading_resolver().await?;
    let vault_address = resolve_optional_acting_account_target(inv, args.on_behalf_of.as_deref())?;
    let vault_address_string = vault_address.map(|address| address.to_string());
    if inv.dry_run() {
        let plan =
            hyperliquid_cli::commands::orders::scale_dry_run_plan(client, &resolver, args).await?;
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
                .map_err(errors::CliError::from)?
            {
                Market::Perp(perp) => Some((perp.name, perp.dex)),
                Market::Spot(_) | Market::Outcome(_) => {
                    if args.margin_mode.is_some() {
                        return Err(errors::CliError::Configuration(
                            "orders scale --margin-mode is only supported for perpetual orders"
                                .to_string(),
                        )
                        .into());
                    }
                    None
                }
            }
        } else if args.margin_mode.is_some() {
            return Err(errors::CliError::Configuration(
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
    hyperliquid_cli::commands::orders::scale(
        order_execution_context(inv, &resolver, &signer)?,
        args,
        vault_address,
    )
    .await
}

async fn batch_create_orders(
    inv: &Invocation,
    args: &hyperliquid_cli::commands::orders::BatchCreateArgs,
) -> Result<(), anyhow::Error> {
    hyperliquid_cli::commands::orders::validate_batch_create_args(args)?;
    let orders = hyperliquid_cli::commands::orders::read_validated_batch_create_orders(args)?;
    let client = inv.remote()?.api().sdk();
    let resolver = inv.trading_resolver().await?;
    let vault_address = resolve_optional_acting_account_target(inv, args.on_behalf_of.as_deref())?;
    let vault_address_string = vault_address.map(|address| address.to_string());
    if inv.dry_run() {
        let plan = hyperliquid_cli::commands::orders::batch_create_dry_run_plan(
            client, &resolver, args, orders,
        )
        .await?;
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
    hyperliquid_cli::commands::orders::batch_create(
        order_execution_context(inv, &resolver, &signer)?,
        args,
        orders,
        vault_address,
    )
    .await
}

async fn create_tpsl(
    inv: &Invocation,
    args: &hyperliquid_cli::commands::orders::TpslArgs,
) -> Result<(), anyhow::Error> {
    hyperliquid_cli::commands::orders::validate_tpsl_args(args)?;
    let client = inv.remote()?.api().sdk();
    let resolver = inv.trading_resolver().await?;
    let coin_query = qualify_dex_asset(args.dex.as_deref(), &args.coin);
    let (resolver, resolved_tpsl_perp) = resolver
        .with_resolved_perp(client, &coin_query)
        .await
        .map_err(errors::CliError::from)?;
    let vault_address = resolve_optional_acting_account_target(inv, args.on_behalf_of.as_deref())?;
    let vault_address_string = vault_address.map(|address| address.to_string());
    if inv.dry_run() {
        let plan = hyperliquid_cli::commands::orders::tpsl_dry_run_plan(&resolver, args)?;
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
    hyperliquid_cli::commands::orders::tpsl(
        order_execution_context(inv, &resolver, &signer)?,
        args,
        vault_address,
        true,
    )
    .await
}

async fn cancel_order(
    inv: &Invocation,
    args: &hyperliquid_cli::commands::orders::CancelArgs,
) -> Result<(), anyhow::Error> {
    let vault_address = resolve_optional_acting_account_target(inv, args.on_behalf_of.as_deref())?;
    let vault_address_string = vault_address.map(|address| address.to_string());
    if inv.dry_run() {
        let plan = hyperliquid_cli::commands::orders::cancel_dry_run_plan(args)?;
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
    hyperliquid_cli::commands::orders::cancel(
        order_execution_context(inv, &resolver, &signer)?,
        user,
        args,
        vault_address,
    )
    .await
}

async fn cancel_all_orders(
    inv: &Invocation,
    args: &hyperliquid_cli::commands::orders::CancelAllArgs,
) -> Result<(), anyhow::Error> {
    let vault_address = resolve_optional_acting_account_target(inv, args.on_behalf_of.as_deref())?;
    let vault_address_string = vault_address.map(|address| address.to_string());
    if inv.dry_run() {
        let resolver = inv.trading_resolver().await?;
        let plan = hyperliquid_cli::commands::orders::cancel_all_dry_run_plan(&resolver, args)?;
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
    hyperliquid_cli::commands::orders::cancel_all(
        order_execution_context(inv, &resolver, &signer)?,
        user,
        args,
        vault_address,
    )
    .await
}

async fn modify_order(
    inv: &Invocation,
    args: &hyperliquid_cli::commands::orders::ModifyArgs,
) -> Result<(), anyhow::Error> {
    hyperliquid_cli::commands::orders::validate_modify_args(args)?;
    let vault_address = resolve_optional_acting_account_target(inv, args.on_behalf_of.as_deref())?;
    let vault_address_string = vault_address.map(|address| address.to_string());
    if inv.dry_run() {
        let plan = hyperliquid_cli::commands::orders::modify_dry_run_plan(args)?;
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
    hyperliquid_cli::commands::orders::modify(
        order_execution_context(inv, &resolver, &signer)?,
        user,
        args,
        vault_address,
    )
    .await
}

async fn open_orders(inv: &Invocation) -> Result<(), anyhow::Error> {
    let resolver = inv.asset_resolver().await?;
    let user = inv.resolve_signer()?.query_address();
    hyperliquid_cli::commands::orders::open(inv, &resolver, user).await
}

async fn order_history(inv: &Invocation) -> Result<(), anyhow::Error> {
    let resolver = inv.asset_resolver().await?;
    let user = inv.resolve_signer()?.query_address();
    hyperliquid_cli::commands::orders::history(inv, &resolver, user).await
}

async fn create_twap(
    inv: &Invocation,
    args: &hyperliquid_cli::commands::orders::TwapCreateArgs,
) -> Result<(), anyhow::Error> {
    let client = inv.remote()?.api().sdk();
    let resolver = inv.trading_resolver().await?;
    let coin_query = qualify_dex_asset(args.dex.as_deref(), &args.coin);
    let (resolver, resolved_twap_perp) = resolver
        .with_resolved_perp(client, &coin_query)
        .await
        .map_err(errors::CliError::from)?;
    let dry_run_plan =
        hyperliquid_cli::commands::orders::twap_create_dry_run_plan(&resolver, args)?;
    let vault_address = resolve_optional_acting_account_target(inv, args.on_behalf_of.as_deref())?;
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
    hyperliquid_cli::commands::orders::twap_create(
        order_submission_context(inv, &signer)?,
        &resolver,
        args,
        vault_address,
    )
    .await
}

async fn trailing_stop_order(
    inv: &Invocation,
    args: &hyperliquid_cli::commands::orders::TrailingStopArgs,
) -> Result<(), anyhow::Error> {
    hyperliquid_cli::commands::orders::validate_trailing_stop_args(args)?;
    let client = inv.remote()?.api().sdk();
    let resolver = inv.trading_resolver().await?;
    let coin_query = qualify_dex_asset(args.dex.as_deref(), &args.coin);
    let (resolver, _) = resolver
        .with_resolved_perp(client, &coin_query)
        .await
        .map_err(errors::CliError::from)?;
    let dry_run_plan =
        hyperliquid_cli::commands::orders::trailing_stop_dry_run_plan(&resolver, args)?;
    let vault_address = resolve_optional_acting_account_target(inv, args.on_behalf_of.as_deref())?;
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
    hyperliquid_cli::commands::orders::trailing_stop(
        order_submission_context(inv, &signer)?,
        &resolver,
        args,
        vault_address,
    )
    .await
}

async fn cancel_twap(
    inv: &Invocation,
    args: &hyperliquid_cli::commands::orders::TwapCancelArgs,
) -> Result<(), anyhow::Error> {
    let client = inv.remote()?.api().sdk();
    let resolver = inv.trading_resolver().await?;
    let coin_query = qualify_dex_asset(args.dex.as_deref(), &args.coin);
    let (resolver, _) = resolver
        .with_resolved_perp(client, &coin_query)
        .await
        .map_err(errors::CliError::from)?;
    let dry_run_plan =
        hyperliquid_cli::commands::orders::twap_cancel_dry_run_plan(&resolver, args)?;
    let vault_address = resolve_optional_acting_account_target(inv, args.on_behalf_of.as_deref())?;
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
    hyperliquid_cli::commands::orders::twap_cancel(
        order_submission_context(inv, &signer)?,
        &resolver,
        args,
        vault_address,
    )
    .await
}

async fn validate_order_margin_mode(
    client: &hypersdk::hypercore::HttpClient,
    user: hypersdk::Address,
    coin: &str,
    dex: Option<&str>,
    margin_mode: Option<hyperliquid_cli::commands::orders::MarginModeArg>,
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
        if matches!(
            margin_mode,
            hyperliquid_cli::commands::orders::MarginModeArg::Cross
        ) {
            return Ok(());
        }
        return Err(errors::CliError::Configuration(format!(
            "orders with --margin-mode {} require an open {} perp position so the mode can be verified; establish it first with `positions update-leverage --coin {} --leverage <N>{}`",
            margin_mode,
            coin,
            coin,
            if matches!(margin_mode, hyperliquid_cli::commands::orders::MarginModeArg::Isolated) {
                " --isolated"
            } else {
                ""
            }
        ))
        .into());
    };

    let actual_margin_mode = match position.position.leverage.leverage_type {
        hypersdk::hypercore::types::LeverageType::Cross => {
            hyperliquid_cli::commands::orders::MarginModeArg::Cross
        }
        hypersdk::hypercore::types::LeverageType::Isolated => {
            hyperliquid_cli::commands::orders::MarginModeArg::Isolated
        }
    };
    if actual_margin_mode != margin_mode {
        return Err(errors::CliError::Configuration(format!(
            "orders requested --margin-mode {} but the current {} perp position is {}; use `positions update-leverage --coin {} --leverage <N>{}` first",
            margin_mode,
            coin,
            actual_margin_mode,
            coin,
            if matches!(margin_mode, hyperliquid_cli::commands::orders::MarginModeArg::Isolated) {
                " --isolated"
            } else {
                ""
            }
        ))
        .into());
    }

    Ok(())
}

async fn schedule_cancel(
    inv: &Invocation,
    args: &hyperliquid_cli::commands::orders::ScheduleCancelArgs,
) -> Result<(), anyhow::Error> {
    let vault_address = resolve_optional_acting_account_target(inv, args.on_behalf_of.as_deref())?;
    let vault_address_string = vault_address.map(|address| address.to_string());
    if inv.dry_run() {
        let plan = hyperliquid_cli::commands::orders::schedule_cancel_dry_run_plan(args)?;
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
    hyperliquid_cli::commands::orders::schedule_cancel(
        order_submission_context(inv, &signer)?,
        args,
        vault_address,
    )
    .await
}

async fn update_position_leverage(
    inv: &Invocation,
    args: &hyperliquid_cli::commands::positions::UpdateLeverageArgs,
) -> Result<(), anyhow::Error> {
    hyperliquid_cli::commands::positions::validate_update_leverage_args(args)?;
    let resolver = inv.trading_resolver().await?;
    let prepared = hyperliquid_cli::commands::positions::prepare_update_leverage(&resolver, args)?;
    if inv.dry_run() {
        let (signer, acting_as) = dry_run_signing_addresses(inv);
        return print_dry_run(
            inv,
            "positions update-leverage",
            dry_run_signed_details(
                inv,
                "update_position_leverage",
                hyperliquid_cli::commands::positions::update_leverage_dry_run_value(
                    inv.remote()?.network().to_string(),
                    args,
                    &prepared,
                ),
                signer,
                acting_as,
                None,
            ),
        );
    }
    let resolved = inv.resolve_signer()?;
    hyperliquid_cli::commands::positions::submit_update_leverage(
        inv,
        &resolved.selected_signer(),
        prepared,
    )
    .await
}

async fn update_position_margin(
    inv: &Invocation,
    args: &hyperliquid_cli::commands::positions::UpdateMarginArgs,
) -> Result<(), anyhow::Error> {
    hyperliquid_cli::commands::positions::validate_update_margin_args(args)?;
    let resolver = inv.trading_resolver().await?;
    let prepared = hyperliquid_cli::commands::positions::prepare_update_margin(&resolver, args)?;
    if inv.dry_run() {
        let (signer, acting_as) = dry_run_signing_addresses(inv);
        return print_dry_run(
            inv,
            "positions update-margin",
            dry_run_signed_details(
                inv,
                "update_isolated_margin",
                hyperliquid_cli::commands::positions::update_margin_dry_run_value(
                    inv.remote()?.network().to_string(),
                    args,
                    &prepared,
                ),
                signer,
                acting_as,
                None,
            ),
        );
    }
    let resolved = inv.resolve_signer()?;
    let signer = resolved.selected_signer();
    hyperliquid_cli::commands::positions::submit_update_margin(inv, &signer, prepared).await
}

async fn transfer_spot_to_perp(
    inv: &Invocation,
    args: &hyperliquid_cli::commands::transfers::ClassTransferArgs,
) -> Result<(), anyhow::Error> {
    hyperliquid_cli::commands::transfers::validate_class_transfer_args(args)?;
    if inv.dry_run() {
        let envelope = transfer_dry_run_envelope(
            inv,
            "transfer spot-to-perp",
            hyperliquid_cli::commands::transfers::TransferActionKind::SpotToPerp,
            "transfer_spot_to_perp",
            serde_json::json!({"amount": args.amount.to_string()}),
        );
        return print_dry_run_envelope(inv, envelope);
    }
    let resolved = inv.resolve_signer()?;
    let signer = resolved.selected_signer();
    hyperliquid_cli::commands::transfers::spot_to_perp(inv, &signer, resolved.query_address(), args)
        .await
}

async fn transfer_perp_to_spot(
    inv: &Invocation,
    args: &hyperliquid_cli::commands::transfers::ClassTransferArgs,
) -> Result<(), anyhow::Error> {
    hyperliquid_cli::commands::transfers::validate_class_transfer_args(args)?;
    if inv.dry_run() {
        let envelope = transfer_dry_run_envelope(
            inv,
            "transfer perp-to-spot",
            hyperliquid_cli::commands::transfers::TransferActionKind::PerpToSpot,
            "transfer_perp_to_spot",
            serde_json::json!({"amount": args.amount.to_string()}),
        );
        return print_dry_run_envelope(inv, envelope);
    }
    let resolved = inv.resolve_signer()?;
    let signer = resolved.selected_signer();
    hyperliquid_cli::commands::transfers::perp_to_spot(inv, &signer, resolved.query_address(), args)
        .await
}

async fn transfer_send(
    inv: &Invocation,
    args: &hyperliquid_cli::commands::transfers::SendArgs,
) -> Result<(), anyhow::Error> {
    hyperliquid_cli::commands::transfers::validate_send_args(args)?;
    if inv.dry_run() {
        let envelope = transfer_dry_run_envelope(
            inv,
            "transfer send",
            hyperliquid_cli::commands::transfers::TransferActionKind::Send,
            "send_usdc",
            serde_json::json!({"to": args.to, "amount": args.amount.to_string()}),
        );
        return print_dry_run_envelope(inv, envelope);
    }
    let resolved = inv.resolve_signer()?;
    hyperliquid_cli::commands::transfers::send(
        inv,
        &resolved.selected_signer(),
        resolved.query_address(),
        args,
    )
    .await
}

async fn transfer_spot_send(
    inv: &Invocation,
    args: &hyperliquid_cli::commands::transfers::SpotSendArgs,
) -> Result<(), anyhow::Error> {
    hyperliquid_cli::commands::transfers::validate_spot_send_args(args)?;
    let client = inv.remote()?.api().sdk();
    let args_json =
        hyperliquid_cli::commands::transfers::spot_send_dry_run_args(client, args).await?;
    if inv.dry_run() {
        let signer = dry_run_signer_address(inv);
        let envelope = transfer_dry_run_envelope(
            inv,
            "transfer spot-send",
            hyperliquid_cli::commands::transfers::TransferActionKind::SpotSend,
            "spot_send",
            args_json,
        )
        .with_signing_context(DryRunSigningContext::new(
            signer,
            Some(args.to.clone()),
            None,
        ));
        return print_dry_run_envelope(inv, envelope);
    }
    let resolved = inv.resolve_signer()?;
    hyperliquid_cli::commands::transfers::spot_send(inv, &resolved.selected_signer(), args).await
}

async fn transfer_send_asset(
    inv: &Invocation,
    args: &hyperliquid_cli::commands::transfers::SendAssetArgs,
) -> Result<(), anyhow::Error> {
    hyperliquid_cli::commands::transfers::validate_send_asset_args(args)?;
    let client = inv.remote()?.api().sdk();
    let args_json =
        hyperliquid_cli::commands::transfers::send_asset_dry_run_args(client, args).await?;
    if inv.dry_run() {
        let signer = dry_run_signer_address(inv);
        let envelope = transfer_dry_run_envelope(
            inv,
            "transfer send-asset",
            hyperliquid_cli::commands::transfers::TransferActionKind::SendAsset,
            "send_asset",
            args_json,
        )
        .with_signing_context(DryRunSigningContext::new(
            signer,
            Some(args.to.clone()),
            None,
        ));
        return print_dry_run_envelope(inv, envelope);
    }
    let resolved = inv.resolve_signer()?;
    hyperliquid_cli::commands::transfers::send_asset(inv, &resolved.selected_signer(), args).await
}

async fn transfer_withdraw(
    inv: &Invocation,
    args: &hyperliquid_cli::commands::transfers::WithdrawArgs,
) -> Result<(), anyhow::Error> {
    hyperliquid_cli::commands::transfers::validate_withdraw_args(args)?;
    if inv.dry_run() {
        let envelope = transfer_dry_run_envelope(
            inv,
            "transfer withdraw",
            hyperliquid_cli::commands::transfers::TransferActionKind::Withdraw,
            "withdraw_usdc",
            serde_json::json!({"to": args.to, "amount": args.amount.to_string()}),
        );
        return print_dry_run_envelope(inv, envelope);
    }
    let resolved = inv.resolve_signer()?;
    hyperliquid_cli::commands::transfers::withdraw(
        inv,
        &resolved.selected_signer(),
        resolved.query_address(),
        args,
    )
    .await
}

async fn staking_delegate(
    inv: &Invocation,
    args: &hyperliquid_cli::commands::staking::DelegateArgs,
) -> Result<(), anyhow::Error> {
    hyperliquid_cli::commands::staking::validate_delegate_args(args)?;
    if inv.dry_run() {
        let action_kind = hyperliquid_cli::commands::staking::StakingActionKind::Delegate;
        let args_json = hyperliquid_cli::commands::staking::delegate_dry_run_value(
            inv.remote()?.chain(),
            args,
            action_kind,
        )?;
        let envelope = signed_action_dry_run_envelope(
            inv,
            "staking delegate",
            action_kind.would_execute(),
            action_kind.reversibility(),
            args_json,
            default_dry_run_signing_context(inv),
        );
        return print_dry_run_envelope(inv, envelope);
    }
    let resolved = inv.resolve_signer()?;
    let signer = resolved.selected_signer();
    hyperliquid_cli::commands::staking::delegate(inv, &signer, args).await
}

async fn staking_undelegate(
    inv: &Invocation,
    args: &hyperliquid_cli::commands::staking::DelegateArgs,
) -> Result<(), anyhow::Error> {
    hyperliquid_cli::commands::staking::validate_delegate_args(args)?;
    if inv.dry_run() {
        let action_kind = hyperliquid_cli::commands::staking::StakingActionKind::Undelegate;
        let args_json = hyperliquid_cli::commands::staking::delegate_dry_run_value(
            inv.remote()?.chain(),
            args,
            action_kind,
        )?;
        let envelope = signed_action_dry_run_envelope(
            inv,
            "staking undelegate",
            action_kind.would_execute(),
            action_kind.reversibility(),
            args_json,
            default_dry_run_signing_context(inv),
        );
        return print_dry_run_envelope(inv, envelope);
    }
    let resolved = inv.resolve_signer()?;
    let signer = resolved.selected_signer();
    hyperliquid_cli::commands::staking::undelegate(inv, &signer, args).await
}

async fn staking_deposit(
    inv: &Invocation,
    args: &hyperliquid_cli::commands::staking::AmountArgs,
) -> Result<(), anyhow::Error> {
    hyperliquid_cli::commands::staking::validate_amount_args(args)?;
    if inv.dry_run() {
        let action_kind = hyperliquid_cli::commands::staking::StakingActionKind::Deposit;
        let args_json = hyperliquid_cli::commands::staking::amount_dry_run_value(
            inv.remote()?.chain(),
            args,
            action_kind,
        )?;
        let envelope = signed_action_dry_run_envelope(
            inv,
            "staking deposit",
            action_kind.would_execute(),
            action_kind.reversibility(),
            args_json,
            default_dry_run_signing_context(inv),
        );
        return print_dry_run_envelope(inv, envelope);
    }
    let resolved = inv.resolve_signer()?;
    let signer = resolved.selected_signer();
    hyperliquid_cli::commands::staking::deposit(inv, &signer, args).await
}

async fn staking_withdraw(
    inv: &Invocation,
    args: &hyperliquid_cli::commands::staking::AmountArgs,
) -> Result<(), anyhow::Error> {
    hyperliquid_cli::commands::staking::validate_amount_args(args)?;
    if inv.dry_run() {
        let action_kind = hyperliquid_cli::commands::staking::StakingActionKind::Withdraw;
        let args_json = hyperliquid_cli::commands::staking::amount_dry_run_value(
            inv.remote()?.chain(),
            args,
            action_kind,
        )?;
        let envelope = signed_action_dry_run_envelope(
            inv,
            "staking withdraw",
            action_kind.would_execute(),
            action_kind.reversibility(),
            args_json,
            default_dry_run_signing_context(inv),
        );
        return print_dry_run_envelope(inv, envelope);
    }
    let resolved = inv.resolve_signer()?;
    let signer = resolved.selected_signer();
    hyperliquid_cli::commands::staking::withdraw(inv, &signer, args).await
}

async fn staking_claim_rewards(inv: &Invocation) -> Result<(), anyhow::Error> {
    if inv.dry_run() {
        let action_kind = hyperliquid_cli::commands::staking::StakingActionKind::ClaimRewards;
        let envelope = signed_action_dry_run_envelope(
            inv,
            "staking claim-rewards",
            action_kind.would_execute(),
            action_kind.reversibility(),
            hyperliquid_cli::commands::staking::claim_rewards_dry_run_value(inv.remote()?.chain()),
            default_dry_run_signing_context(inv),
        );
        return print_dry_run_envelope(inv, envelope);
    }
    let resolved = inv.resolve_signer()?;
    let signer = resolved.selected_signer();
    hyperliquid_cli::commands::staking::claim_rewards(inv, &signer).await
}

async fn staking_link(
    inv: &Invocation,
    args: &hyperliquid_cli::commands::staking::LinkArgs,
    is_finalize: bool,
) -> Result<(), anyhow::Error> {
    hyperliquid_cli::commands::staking::validate_link_args(args)?;
    let phase = if is_finalize { "finalize" } else { "initiate" };
    if inv.dry_run() {
        let args_json = hyperliquid_cli::commands::staking::link_dry_run_value(
            inv.remote()?.chain(),
            args,
            is_finalize,
        )?;
        let action_kind = if is_finalize {
            hyperliquid_cli::commands::staking::StakingActionKind::LinkFinalize
        } else {
            hyperliquid_cli::commands::staking::StakingActionKind::LinkInitiate
        };
        let envelope = signed_action_dry_run_envelope(
            inv,
            &format!("staking link {phase}"),
            action_kind.would_execute(),
            action_kind.reversibility(),
            args_json,
            default_dry_run_signing_context(inv),
        );
        return print_dry_run_envelope(inv, envelope);
    }
    let resolved = inv.resolve_signer()?;
    let signer = resolved.selected_signer();
    hyperliquid_cli::commands::staking::link_staking_user(inv, &signer, args, is_finalize).await
}

async fn vault_deposit(
    inv: &Invocation,
    args: &hyperliquid_cli::commands::vaults::VaultTransferArgs,
) -> Result<(), anyhow::Error> {
    hyperliquid_cli::commands::vaults::validate_transfer_args(args)?;
    if inv.dry_run() {
        let args_json = hyperliquid_cli::commands::vaults::transfer_dry_run_value(
            inv.remote()?.chain(),
            args,
            hyperliquid_cli::commands::vaults::VaultTransferActionKind::Deposit,
        )?;
        let envelope = signed_action_dry_run_envelope(
            inv,
            "vault deposit",
            "deposit_usdc_to_vault",
            hyperliquid_cli::commands::vaults::VaultTransferActionKind::Deposit.reversibility(),
            args_json,
            default_dry_run_signing_context(inv),
        );
        return print_dry_run_envelope(inv, envelope);
    }
    let resolved = inv.resolve_signer()?;
    hyperliquid_cli::commands::vaults::deposit(inv, &resolved.selected_signer(), args).await
}

async fn vault_withdraw(
    inv: &Invocation,
    args: &hyperliquid_cli::commands::vaults::VaultTransferArgs,
) -> Result<(), anyhow::Error> {
    hyperliquid_cli::commands::vaults::validate_transfer_args(args)?;
    if inv.dry_run() {
        let args_json = hyperliquid_cli::commands::vaults::transfer_dry_run_value(
            inv.remote()?.chain(),
            args,
            hyperliquid_cli::commands::vaults::VaultTransferActionKind::Withdraw,
        )?;
        let envelope = signed_action_dry_run_envelope(
            inv,
            "vault withdraw",
            "withdraw_usdc_from_vault",
            hyperliquid_cli::commands::vaults::VaultTransferActionKind::Withdraw.reversibility(),
            args_json,
            default_dry_run_signing_context(inv),
        );
        return print_dry_run_envelope(inv, envelope);
    }
    let resolved = inv.resolve_signer()?;
    hyperliquid_cli::commands::vaults::withdraw(inv, &resolved.selected_signer(), args).await
}

async fn borrowlend_action(
    inv: &Invocation,
    operation: hyperliquid_cli::commands::borrowlend::BorrowLendOperation,
    args: &hyperliquid_cli::commands::borrowlend::ActionArgs,
) -> Result<(), anyhow::Error> {
    let preview =
        hyperliquid_cli::commands::borrowlend::action_preview(inv.remote()?.api(), operation, args)
            .await?;
    if inv.dry_run() {
        let envelope = signed_action_dry_run_envelope(
            inv,
            &format!("borrowlend {}", operation.as_str()),
            &format!("{}_borrowlend", operation.as_str()),
            operation.reversibility(),
            preview,
            default_dry_run_signing_context(inv),
        );
        return print_dry_run_envelope(inv, envelope);
    }

    let resolved = inv.resolve_signer()?;
    let signer = resolved.selected_signer();
    hyperliquid_cli::commands::borrowlend::action(inv, &signer, operation, args).await
}

async fn prio_bid(
    inv: &Invocation,
    args: &hyperliquid_cli::commands::prio::BidArgs,
) -> Result<(), anyhow::Error> {
    if inv.dry_run() {
        return print_dry_run(
            inv,
            "prio bid",
            dry_run_details(
                inv,
                "place_priority_auction_bid",
                serde_json::json!({"ip": args.ip, "max": args.max.to_string(), "slot": args.slot}),
            ),
        );
    }
    let resolved = inv.resolve_signer()?;
    hyperliquid_cli::commands::prio::bid(inv, &resolved.selected_signer(), args).await
}

async fn set_referral(inv: &Invocation, code: Option<&str>) -> Result<(), anyhow::Error> {
    let owned_default: Option<String>;
    let code: &str = match code {
        Some(code) => code,
        None => {
            let remote = inv.remote()?;
            owned_default = hyperliquid_cli::commands::referral::resolve_default_referral_code(
                remote.settings(),
                remote.network().is_testnet(),
            )?;
            match owned_default.as_deref() {
                Some(default_code) => default_code,
                None => {
                    return Err(errors::CliError::Unsupported(
                        "referral code is required; pass CODE explicitly, set HYPERLIQUID_DEFAULT_REFERRAL_CODE, or use --testnet"
                            .to_string(),
                    )
                    .into());
                }
            }
        }
    };
    hyperliquid_cli::commands::referral::validate_referral_code(code)?;
    if inv.dry_run() {
        let signer_context = inv.resolve_signer().ok();
        let envelope = signed_action_dry_run_envelope(
            inv,
            "referral set",
            "set_referral_code",
            hyperliquid_cli::commands::referral::ReferralActionKind::Set.reversibility(),
            hyperliquid_cli::commands::referral::referral_dry_run_value(
                inv.remote()?.chain(),
                signer_context.as_ref().map(|resolved| resolved.address()),
                signer_context
                    .as_ref()
                    .map(|resolved| resolved.query_address()),
                code,
                hyperliquid_cli::commands::referral::ReferralActionKind::Set,
            ),
            default_dry_run_signing_context(inv),
        );
        return print_dry_run_envelope(inv, envelope);
    }
    let resolved = inv.resolve_signer()?;
    let signer = resolved.selected_signer();
    hyperliquid_cli::commands::referral::set(inv, &signer, code).await
}

async fn register_referrer(inv: &Invocation, code: &str) -> Result<(), anyhow::Error> {
    hyperliquid_cli::commands::referral::validate_referrer_code(code)?;
    if inv.dry_run() {
        let signer_context = inv.resolve_signer().ok();
        let envelope = signed_action_dry_run_envelope(
            inv,
            "referral register",
            "register_referrer_code",
            hyperliquid_cli::commands::referral::ReferralActionKind::Register.reversibility(),
            hyperliquid_cli::commands::referral::referral_dry_run_value(
                inv.remote()?.chain(),
                signer_context.as_ref().map(|resolved| resolved.address()),
                signer_context
                    .as_ref()
                    .map(|resolved| resolved.query_address()),
                code,
                hyperliquid_cli::commands::referral::ReferralActionKind::Register,
            ),
            default_dry_run_signing_context(inv),
        );
        return print_dry_run_envelope(inv, envelope);
    }
    let resolved = inv.resolve_signer()?;
    let signer = resolved.selected_signer();
    hyperliquid_cli::commands::referral::register(inv, &signer, code).await
}

fn unimplemented_command_error() -> anyhow::Error {
    errors::CliError::Internal(anyhow::anyhow!(
        "This command isn't implemented in this version of hyperliquid"
    ))
    .into()
}

fn ensure_outcome_coin(coin: &str) -> Result<(), errors::CliError> {
    hyperliquid_cli::assets::asset_id::parse_outcome_notation(coin)?;
    Ok(())
}

async fn maybe_hip3_dry_run_side_effect(
    api: &hyperliquid_cli::http_api::ApiClient,
    user: hypersdk::Address,
    args: &hyperliquid_cli::commands::orders::CreateArgs,
    preview: &serde_json::Map<String, serde_json::Value>,
) -> Result<Option<serde_json::Value>, anyhow::Error> {
    let Some(dex) =
        hyperliquid_cli::commands::orders::hip3_dex_from_coin(&args.coin, args.dex.as_deref())
    else {
        return Ok(None);
    };
    let notional = preview_notional(preview).unwrap_or(hypersdk::Decimal::ZERO);
    let plan = hyperliquid_cli::commands::orders::plan_hip3_margin_transfer(
        api,
        user,
        Some(dex.as_str()),
        notional,
    )
    .await?;
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

async fn chase_order(
    inv: &Invocation,
    args: &hyperliquid_cli::commands::orders::ChaseArgs,
) -> Result<(), anyhow::Error> {
    hyperliquid_cli::commands::orders::chase_dry_run_plan(args, None)?;
    let client = inv.remote()?.api().sdk();
    let resolver = inv.trading_resolver().await?;
    let vault_address = resolve_optional_acting_account_target(inv, args.on_behalf_of.as_deref())?;
    let vault_address_string = vault_address.map(|address| address.to_string());
    if inv.dry_run() {
        let coin_key = qualify_dex_asset(args.dex.as_deref(), &args.coin);
        let mid = client
            .all_mids(None)
            .await
            .ok()
            .and_then(|mids| mids.get(&coin_key).copied());
        let plan = hyperliquid_cli::commands::orders::chase_dry_run_plan(args, mid)?;
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
    hyperliquid_cli::commands::orders::chase(
        order_execution_context(inv, &resolver, &signer)?,
        args,
        vault_address,
    )
    .await
}

async fn bracket_order(
    inv: &Invocation,
    args: &hyperliquid_cli::commands::orders::BracketArgs,
) -> Result<(), anyhow::Error> {
    hyperliquid_cli::commands::orders::validate_bracket_args(args)?;
    let client = inv.remote()?.api().sdk();
    let resolver = inv.trading_resolver().await?;
    let vault_address = resolve_optional_acting_account_target(inv, args.on_behalf_of.as_deref())?;
    let vault_address_string = vault_address.map(|address| address.to_string());
    if inv.dry_run() {
        let plan = hyperliquid_cli::commands::orders::bracket_dry_run_plan(client, &resolver, args)
            .await?;
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
    hyperliquid_cli::commands::orders::bracket(
        order_execution_context(inv, &resolver, &signer)?,
        args,
        vault_address,
    )
    .await
}

async fn watch_risk_command(
    inv: &Invocation,
    args: &hyperliquid_cli::commands::watch_risk::WatchRiskArgs,
) -> Result<(), anyhow::Error> {
    let user = if let Some(raw) = args.user.as_deref() {
        hyperliquid_cli::commands::watch_risk::parse_watch_user(raw)?
    } else {
        inv.resolve_signer()?.query_address()
    };
    hyperliquid_cli::commands::watch_risk::watch_risk(inv, user, args).await
}

#[cfg(test)]
mod tests {
    use super::{replay_args, replay_command};

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_string()).collect()
    }

    #[test]
    fn replay_drops_secret_options_and_quotes_for_the_shell() {
        let args = replay_args(strings(&[
            "--private-key",
            "0xsecret",
            "--keystore-password=hunter2",
            "orders",
            "create",
            "--coin",
            "BTC",
            "--cloid",
            "it's here",
        ]));
        assert_eq!(
            replay_command(&args),
            r"hyperliquid orders create --coin BTC --cloid 'it'\''s here'"
        );
    }
}
