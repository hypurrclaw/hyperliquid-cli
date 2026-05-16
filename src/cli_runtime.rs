use super::*;

pub(super) async fn run(cli: &Cli, registry_path: Option<&[String]>) -> Result<(), anyhow::Error> {
    run_command(cli, registry_path).await
}

async fn run_command(cli: &Cli, registry_path: Option<&[String]>) -> Result<(), anyhow::Error> {
    validate_cli_inputs(cli)?;
    validate_stream_bounds(cli)?;

    if matches!(cli.command, Some(Commands::Update)) {
        if cli.payload_json.is_some() || cli.payload_file.is_some() {
            return Err(errors::CliError::Unsupported(
                "payload input is not supported for update".to_string(),
            )
            .into());
        }
        return hyperliquid_cli::update_check::update(cli.format, cli.dry_run).await;
    }

    let command_contract = command_contract_for_path(registry_path)?;
    if cli.payload_json.is_some() || cli.payload_file.is_some() {
        validate_raw_payload_supported(command_contract.as_ref())?;
    }
    validate_prompt_policy(cli, command_contract.as_ref())?;

    if let Some(result) = run_local_command_without_context(cli).await {
        return result;
    }

    let payload = read_payload_json(cli)?;
    if cli.dry_run && !command_accepts_mutating_preview(command_contract.as_ref()) {
        return Err(errors::CliError::Unsupported(
            "--dry-run is only supported for mutating commands with preview support".to_string(),
        )
        .into());
    }
    if payload.is_some() && !cli.dry_run {
        return Err(errors::CliError::Unsupported(
            "payload input currently requires --dry-run so raw payloads can be validated without side effects"
                .to_string(),
        )
        .into());
    }

    let context = AppContext::resolve(cli)?;
    if context.has_api_base_url_override() && command_uses_api(command_contract.as_ref()) {
        context.validate_api_override().await?;
    }

    match &cli.command {
        None => {
            println!(
                "hyperliquid-cli v{} — Hyperliquid DEX CLI",
                env!("CARGO_PKG_VERSION")
            );
            println!("Network: {}", context.network);
            println!("API: {}", context.api_base_url());
            println!("Use --help to see available commands.");
            Ok(())
        }
        Some(Commands::Wallet {
            subcommand: WalletCommands::Address,
        }) => hyperliquid_cli::commands::wallet::address(
            context.private_key.as_deref(),
            context.keystore.as_deref(),
            context.keystore_password.as_deref(),
            context.account.as_deref(),
            context.ows_signer.as_deref(),
            cli.format,
        ),
        Some(Commands::Wallet {
            subcommand: WalletCommands::Show,
        }) => hyperliquid_cli::commands::wallet::show(
            context.private_key.as_deref(),
            context.keystore.as_deref(),
            context.keystore_password.as_deref(),
            context.account.as_deref(),
            context.ows_signer.as_deref(),
            cli.format,
        ),
        Some(Commands::Perps {
            subcommand: PerpsCommands::List(args),
        }) => {
            let client = context.http_client();
            let command_context =
                cli_command_context(&context, cli, Some(&client), payload.is_some());
            hyperliquid_cli::commands::perps::list_with_context(&command_context, args).await
        }
        Some(Commands::Perps {
            subcommand: PerpsCommands::Get(args),
        }) => resolve_and_print_perp(&context, args, cli.format).await,
        Some(Commands::Spot {
            subcommand: SpotCommands::List,
        }) => {
            let client = context.http_client();
            let command_context =
                cli_command_context(&context, cli, Some(&client), payload.is_some());
            hyperliquid_cli::commands::spot::list_with_context(&command_context).await
        }
        Some(Commands::Spot {
            subcommand: SpotCommands::Get { pair },
        }) => resolve_and_print_spot(&context, pair, cli.format).await,
        Some(Commands::Asset {
            subcommand: AssetCommands::Decode(args),
        }) => {
            let command_context = cli_command_context(&context, cli, None, payload.is_some());
            hyperliquid_cli::commands::asset::decode_with_context(&command_context, args).await
        }
        Some(Commands::Asset {
            subcommand: AssetCommands::Search(args),
        }) => {
            let command_context = cli_command_context(&context, cli, None, payload.is_some());
            hyperliquid_cli::commands::asset::search_with_context(&command_context, args).await
        }
        Some(Commands::Book { coin, watch }) => {
            if watch.watch {
                watch_book(&context, coin, cli.format, watch.max_ticks).await
            } else {
                resolve_and_print_book(&context, cli, coin, payload.is_some()).await
            }
        }
        Some(Commands::Candles {
            coin,
            interval,
            limit,
            watch,
        }) => {
            if watch.watch {
                watch_candles(
                    &context,
                    coin,
                    *interval,
                    *limit,
                    cli.format,
                    watch.max_ticks,
                )
                .await
            } else {
                resolve_and_print_candles(&context, cli, coin, *interval, *limit, payload.is_some())
                    .await
            }
        }
        Some(Commands::Spread { coin }) => {
            resolve_and_print_spread(&context, cli, coin, payload.is_some()).await
        }
        Some(Commands::Funding { coin }) => {
            resolve_and_print_funding(&context, cli, coin, payload.is_some()).await
        }
        Some(Commands::Builder {
            subcommand: BuilderCommands::MaxFee(args),
        }) => builder_max_fee(&context, args, cli.format).await,
        Some(Commands::Builder {
            subcommand: BuilderCommands::Approved(args),
        }) => builder_approved(&context, args, cli.format).await,
        Some(Commands::Builder {
            subcommand: BuilderCommands::Approve(args),
        }) => builder_approve(&context, args, cli.format, cli.dry_run, payload.as_ref()).await,
        Some(Commands::Outcomes {
            subcommand: OutcomeCommands::List(args),
        }) => print_outcomes_list(&context, args, cli.format).await,
        Some(Commands::Outcomes {
            subcommand: OutcomeCommands::Get(args),
        }) => print_outcomes_get(&context, args, cli.format).await,
        Some(Commands::Mids { watch }) => {
            if watch.watch {
                watch_mids(&context, cli.format, cli.select.as_deref(), watch.max_ticks).await
            } else {
                print_mids(&context, cli, payload.is_some()).await
            }
        }
        Some(Commands::Account {
            subcommand: AccountCommands::Fills(args),
        }) => print_account_fills(&context, cli, args, payload.is_some()).await,
        Some(Commands::Account {
            subcommand: AccountCommands::Fees(args),
        }) => print_account_fees(&context, cli, args, payload.is_some()).await,
        Some(Commands::Account {
            subcommand: AccountCommands::RateLimit(args),
        }) => print_account_rate_limit(&context, cli, args, payload.is_some()).await,
        Some(Commands::Account {
            subcommand: AccountCommands::Orders { address },
        }) => print_account_orders(&context, cli, address.as_deref(), payload.is_some()).await,
        Some(Commands::Account {
            subcommand: AccountCommands::Portfolio { address },
        }) => print_account_portfolio(&context, cli, address.as_deref(), payload.is_some()).await,
        Some(Commands::Account {
            subcommand: AccountCommands::Subaccounts { address },
        }) => print_account_subaccounts(&context, cli, address.as_deref(), payload.is_some()).await,
        Some(Commands::ApiWallet {
            subcommand: ApiWalletCommands::Create(args),
        }) => api_wallet_create(&context, args, cli.format, cli.dry_run).await,
        Some(Commands::ApiWallet {
            subcommand: ApiWalletCommands::Approve(args),
        }) => api_wallet_approve(&context, args, cli.format, cli.dry_run).await,
        Some(Commands::ApiWallet {
            subcommand: ApiWalletCommands::List { account },
        }) => api_wallet_list(&context, account.as_deref(), cli.format).await,
        Some(Commands::ApiWallet {
            subcommand: ApiWalletCommands::Revoke(args),
        }) => api_wallet_revoke(&context, args, cli.format, cli.dry_run).await,
        Some(Commands::Account {
            subcommand: AccountCommands::PortfolioHistory(args),
        }) => print_account_portfolio_history(&context, cli, args, payload.is_some()).await,
        Some(Commands::Account {
            subcommand: AccountCommands::Ledger(args),
        }) => print_account_ledger(&context, cli, args, payload.is_some()).await,
        Some(Commands::Account {
            subcommand: AccountCommands::Funding(args),
        }) => print_account_funding(&context, cli, args, payload.is_some()).await,
        Some(Commands::Account {
            subcommand: AccountCommands::TwapHistory(args),
        }) => print_account_twap_history(&context, cli, args, payload.is_some()).await,
        Some(Commands::Account {
            subcommand: AccountCommands::TwapFills(args),
        }) => print_account_twap_fills(&context, cli, args, payload.is_some()).await,
        Some(Commands::Account {
            subcommand: AccountCommands::Abstraction(args),
        }) => account_abstraction(&context, cli, args, payload.as_ref()).await,
        Some(Commands::Subaccount {
            subcommand: SubaccountCommands::List { address },
        }) => print_account_subaccounts(&context, cli, address.as_deref(), payload.is_some()).await,
        Some(Commands::Subaccount {
            subcommand: SubaccountCommands::Create(args),
        }) => subaccount_create(&context, args, cli.format, cli.dry_run, payload.as_ref()).await,
        Some(Commands::Subaccount {
            subcommand: SubaccountCommands::Transfer(args),
        }) => subaccount_transfer(&context, args, cli.format, cli.dry_run, payload.as_ref()).await,
        Some(Commands::Subaccount {
            subcommand: SubaccountCommands::SpotTransfer(args),
        }) => {
            subaccount_spot_transfer(&context, args, cli.format, cli.dry_run, payload.as_ref())
                .await
        }
        Some(Commands::Status) => print_status(&context, cli, payload.is_some()).await,
        Some(Commands::Meta) => print_meta(&context, cli, payload.is_some()).await,
        Some(Commands::Orders {
            subcommand: OrderCommands::Create(args),
        }) => create_order(&context, args, cli.format, cli.dry_run, payload.as_ref()).await,
        Some(Commands::Orders {
            subcommand: OrderCommands::Scale(args),
        }) => scale_orders(&context, args, cli.format, cli.dry_run, payload.as_ref()).await,
        Some(Commands::Orders {
            subcommand: OrderCommands::BatchCreate(args),
        }) => batch_create_orders(&context, args, cli.format, cli.dry_run, payload.as_ref()).await,
        Some(Commands::Orders {
            subcommand: OrderCommands::Tpsl(args),
        }) => create_tpsl(&context, args, cli.format, cli.dry_run, payload.as_ref()).await,
        Some(Commands::Orders {
            subcommand: OrderCommands::Cancel(args),
        }) => cancel_order(&context, args, cli.format, cli.dry_run, payload.as_ref()).await,
        Some(Commands::Orders {
            subcommand: OrderCommands::CancelAll(args),
        }) => cancel_all_orders(&context, args, cli.format, cli.dry_run, payload.as_ref()).await,
        Some(Commands::Orders {
            subcommand: OrderCommands::Modify(args),
        }) => modify_order(&context, args, cli.format, cli.dry_run, payload.as_ref()).await,
        Some(Commands::Orders {
            subcommand: OrderCommands::Open { watch },
        }) => {
            if watch.watch {
                watch_open_orders(&context, cli.format, watch.max_ticks).await
            } else {
                open_orders(&context, cli.format).await
            }
        }
        Some(Commands::Orders {
            subcommand: OrderCommands::History,
        }) => order_history(&context, cli.format).await,
        Some(Commands::Orders {
            subcommand: OrderCommands::Status(args),
        }) => order_status(&context, args, cli.format).await,
        Some(Commands::Orders {
            subcommand: OrderCommands::TwapCreate(args),
        }) => create_twap(&context, args, cli.format, cli.dry_run, payload.as_ref()).await,
        Some(Commands::Orders {
            subcommand: OrderCommands::TwapCancel(args),
        }) => cancel_twap(&context, args, cli.format, cli.dry_run, payload.as_ref()).await,
        Some(Commands::Orders {
            subcommand: OrderCommands::ScheduleCancel(args),
        }) => schedule_cancel(&context, args, cli.format, cli.dry_run, payload.as_ref()).await,
        Some(Commands::Positions {
            subcommand: PositionCommands::List { watch },
        }) => {
            if watch.watch {
                watch_positions(&context, cli.format, watch.max_ticks).await
            } else {
                list_positions(&context, cli.format).await
            }
        }
        Some(Commands::Positions {
            subcommand: PositionCommands::UpdateLeverage(args),
        }) => {
            update_position_leverage(&context, args, cli.format, cli.dry_run, payload.as_ref())
                .await
        }
        Some(Commands::Positions {
            subcommand: PositionCommands::UpdateMargin(args),
        }) => {
            update_position_margin(&context, args, cli.format, cli.dry_run, payload.as_ref()).await
        }
        Some(Commands::Transfer {
            subcommand: TransferCommands::SpotToPerp(args),
        }) => {
            transfer_spot_to_perp(&context, args, cli.format, cli.dry_run, payload.as_ref()).await
        }
        Some(Commands::Transfer {
            subcommand: TransferCommands::PerpToSpot(args),
        }) => {
            transfer_perp_to_spot(&context, args, cli.format, cli.dry_run, payload.as_ref()).await
        }
        Some(Commands::Transfer {
            subcommand: TransferCommands::Send(args),
        }) => transfer_send(&context, args, cli.format, cli.dry_run, payload.as_ref()).await,
        Some(Commands::Transfer {
            subcommand: TransferCommands::SpotSend(args),
        }) => transfer_spot_send(&context, args, cli.format, cli.dry_run, payload.as_ref()).await,
        Some(Commands::Transfer {
            subcommand: TransferCommands::SendAsset(args),
        }) => transfer_send_asset(&context, args, cli.format, cli.dry_run, payload.as_ref()).await,
        Some(Commands::Transfer {
            subcommand: TransferCommands::Withdraw(args),
        }) => transfer_withdraw(&context, args, cli.format, cli.dry_run, payload.as_ref()).await,
        Some(Commands::Setup(args)) => {
            hyperliquid_cli::commands::setup::run(context.network, args, cli.format).await
        }
        Some(Commands::Staking {
            subcommand: StakingCommands::Summary { address },
        }) => staking_summary(&context, address.as_deref(), cli.format).await,
        Some(Commands::Staking {
            subcommand: StakingCommands::Validators,
        }) => staking_validators(&context, cli.format).await,
        Some(Commands::Staking {
            subcommand: StakingCommands::Rewards { address },
        }) => staking_rewards(&context, address.as_deref(), cli.format).await,
        Some(Commands::Staking {
            subcommand: StakingCommands::History { address },
        }) => staking_history(&context, address.as_deref(), cli.format).await,
        Some(Commands::Staking {
            subcommand: StakingCommands::Delegate(args),
        }) => staking_delegate(&context, args, cli.format, cli.dry_run, payload.as_ref()).await,
        Some(Commands::Staking {
            subcommand: StakingCommands::Undelegate(args),
        }) => staking_undelegate(&context, args, cli.format, cli.dry_run, payload.as_ref()).await,
        Some(Commands::Staking {
            subcommand: StakingCommands::Deposit(args),
        }) => staking_deposit(&context, args, cli.format, cli.dry_run, payload.as_ref()).await,
        Some(Commands::Staking {
            subcommand: StakingCommands::Withdraw(args),
        }) => staking_withdraw(&context, args, cli.format, cli.dry_run, payload.as_ref()).await,
        Some(Commands::Staking {
            subcommand: StakingCommands::ClaimRewards,
        }) => staking_claim_rewards(&context, cli.format, cli.dry_run, payload.as_ref()).await,
        Some(Commands::Staking {
            subcommand:
                StakingCommands::Link {
                    subcommand: StakingLinkCommands::Initiate(args),
                },
        }) => {
            staking_link(
                &context,
                args,
                false,
                cli.format,
                cli.dry_run,
                payload.as_ref(),
            )
            .await
        }
        Some(Commands::Staking {
            subcommand:
                StakingCommands::Link {
                    subcommand: StakingLinkCommands::Finalize(args),
                },
        }) => {
            staking_link(
                &context,
                args,
                true,
                cli.format,
                cli.dry_run,
                payload.as_ref(),
            )
            .await
        }
        Some(Commands::Vaults {
            subcommand: VaultCommands::List(args),
        }) => vault_list(&context, args, cli.format).await,
        Some(Commands::Vaults {
            subcommand: VaultCommands::Search(args),
        }) => vault_search(&context, args, cli.format).await,
        Some(Commands::Vaults {
            subcommand: VaultCommands::Get { address },
        }) => vault_get(&context, address, cli.format).await,
        Some(Commands::Vaults {
            subcommand: VaultCommands::Positions { address },
        }) => vault_positions(&context, address, cli.format).await,
        Some(Commands::Vaults {
            subcommand: VaultCommands::Deposit(args),
        }) => vault_deposit(&context, args, cli.format, cli.dry_run, payload.as_ref()).await,
        Some(Commands::Vaults {
            subcommand: VaultCommands::Withdraw(args),
        }) => vault_withdraw(&context, args, cli.format, cli.dry_run, payload.as_ref()).await,
        Some(Commands::Borrowlend {
            subcommand: BorrowLendCommands::Rates,
        }) => borrowlend_rates(&context, cli.format).await,
        Some(Commands::Borrowlend {
            subcommand: BorrowLendCommands::Get { token },
        }) => borrowlend_get(&context, token, cli.format).await,
        Some(Commands::Borrowlend {
            subcommand: BorrowLendCommands::User { address },
        }) => borrowlend_user(&context, address.as_deref(), cli.format).await,
        Some(Commands::Borrowlend {
            subcommand: BorrowLendCommands::Supply(args),
        }) => {
            borrowlend_action(
                &context,
                hyperliquid_cli::commands::borrowlend::BorrowLendOperation::Supply,
                args,
                cli.format,
                cli.dry_run,
                payload.as_ref(),
            )
            .await
        }
        Some(Commands::Borrowlend {
            subcommand: BorrowLendCommands::Withdraw(args),
        }) => {
            borrowlend_action(
                &context,
                hyperliquid_cli::commands::borrowlend::BorrowLendOperation::Withdraw,
                args,
                cli.format,
                cli.dry_run,
                payload.as_ref(),
            )
            .await
        }
        Some(Commands::Prio {
            subcommand: PrioCommands::Status,
        }) => prio_status(&context, cli.format).await,
        Some(Commands::Prio {
            subcommand: PrioCommands::Bid(args),
        }) => prio_bid(&context, args, cli.format, cli.dry_run, payload.as_ref()).await,
        Some(Commands::Referral {
            subcommand: ReferralCommands::Set { code },
        }) => {
            set_referral(
                &context,
                code.as_deref(),
                cli.format,
                cli.dry_run,
                payload.as_ref(),
            )
            .await
        }
        Some(Commands::Referral {
            subcommand: ReferralCommands::Register { code },
        }) => register_referrer(&context, code, cli.format, cli.dry_run, payload.as_ref()).await,
        Some(Commands::Referral {
            subcommand: ReferralCommands::Status,
        }) => referral_status(&context, cli.format).await,
        Some(Commands::Subscribe { subcommand }) => subscribe(&context, subcommand).await,
        _ => {
            // Placeholder for all other unimplemented commands
            Err(unimplemented_command_error())
        }
    }
}

async fn run_local_command_without_context(cli: &Cli) -> Option<Result<(), anyhow::Error>> {
    match &cli.command {
        Some(Commands::Schema { command_path }) => Some(hyperliquid_cli::commands::schema::show(
            command_path,
            cli.format,
        )),
        Some(Commands::Feedback(_)) if cli.dry_run => Some(Err(errors::CliError::Unsupported(
            "--dry-run is not supported for feedback".to_string(),
        )
        .into())),
        Some(Commands::Feedback(args)) => {
            Some(hyperliquid_cli::commands::feedback::submit(args, cli.format).await)
        }
        Some(Commands::Wallet {
            subcommand: WalletCommands::Create,
        }) if cli.dry_run => Some(print_dry_run(
            "wallet create",
            serde_json::json!({"would_execute": "create_and_store_encrypted_wallet"}),
            cli.format,
        )),
        Some(Commands::Wallet {
            subcommand: WalletCommands::Create,
        }) => Some(hyperliquid_cli::commands::wallet::create(cli.format)),
        Some(Commands::Wallet {
            subcommand: WalletCommands::Import { private_key },
        }) if cli.dry_run => Some(print_dry_run(
            "wallet import",
            serde_json::json!({
                "would_execute": "validate_and_store_encrypted_wallet",
                "args": {"private_key_supplied": private_key.is_some()}
            }),
            cli.format,
        )),
        Some(Commands::Wallet {
            subcommand: WalletCommands::Import { private_key },
        }) => Some(hyperliquid_cli::commands::wallet::import(
            private_key.as_deref(),
            cli.format,
        )),
        Some(Commands::Wallet {
            subcommand: WalletCommands::Reset { yes },
        }) if cli.dry_run => Some(print_dry_run(
            "wallet reset",
            serde_json::json!({
                "would_execute": "remove_wallet_configuration",
                "args": {"yes": yes}
            }),
            cli.format,
        )),
        Some(Commands::Wallet {
            subcommand: WalletCommands::Reset { yes },
        }) => Some(hyperliquid_cli::commands::wallet::reset(*yes, cli.format)),
        Some(Commands::Wallet {
            subcommand: WalletCommands::ImportMnemonic { mnemonic, alias },
        }) if cli.dry_run => Some(print_dry_run(
            "wallet import-mnemonic",
            serde_json::json!({
                "would_execute": "import_mnemonic_to_ows_vault",
                "args": {"mnemonic_supplied": mnemonic.is_some(), "alias": alias}
            }),
            cli.format,
        )),
        Some(Commands::Wallet {
            subcommand: WalletCommands::ImportMnemonic { mnemonic, alias },
        }) => Some(hyperliquid_cli::commands::wallet::import_mnemonic(
            mnemonic.as_deref(),
            alias.as_deref(),
            cli.format,
        )),
        Some(Commands::Wallet {
            subcommand: WalletCommands::List,
        }) => Some(hyperliquid_cli::commands::wallet::list(cli.format)),
        Some(Commands::Wallet {
            subcommand: WalletCommands::Rename { .. },
        }) if cli.dry_run => Some(Err(errors::CliError::Unsupported(
            "--dry-run is not supported for wallet rename".to_string(),
        )
        .into())),
        Some(Commands::Wallet {
            subcommand: WalletCommands::Rename { selector, new_name },
        }) => Some(hyperliquid_cli::commands::wallet::rename(
            selector, new_name, cli.format,
        )),
        Some(Commands::Wallet {
            subcommand: WalletCommands::Delete { selector, yes },
        }) if cli.dry_run => Some(print_dry_run(
            "wallet delete",
            serde_json::json!({
                "would_execute": "delete_ows_wallet",
                "args": {"selector": selector, "yes": yes}
            }),
            cli.format,
        )),
        Some(Commands::Wallet {
            subcommand: WalletCommands::Delete { selector, yes },
        }) => Some(hyperliquid_cli::commands::wallet::delete(
            selector, *yes, cli.format,
        )),
        Some(Commands::Wallet {
            subcommand: WalletCommands::Export { .. },
        }) if cli.dry_run => Some(Err(errors::CliError::Unsupported(
            "--dry-run is not supported for wallet export".to_string(),
        )
        .into())),
        Some(Commands::Wallet {
            subcommand: WalletCommands::Export { selector, yes },
        }) => Some(hyperliquid_cli::commands::wallet::export(
            selector, *yes, cli.format,
        )),
        Some(Commands::Account {
            subcommand:
                AccountCommands::Add {
                    private_key,
                    alias,
                    account_type,
                    default,
                },
        }) if cli.dry_run => Some(print_dry_run(
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
            cli.format,
        )),
        Some(Commands::Account {
            subcommand:
                AccountCommands::Add {
                    private_key,
                    alias,
                    account_type,
                    default,
                },
        }) => Some(hyperliquid_cli::commands::wallet::account_add(
            private_key.as_deref(),
            alias.as_deref(),
            account_type.as_deref(),
            *default,
            cli.format,
        )),
        Some(Commands::Account {
            subcommand: AccountCommands::Ls,
        }) => Some(hyperliquid_cli::commands::wallet::account_ls(cli.format)),
        Some(Commands::Account {
            subcommand: AccountCommands::SetDefault { selector },
        }) if cli.dry_run => Some(print_dry_run(
            "account set-default",
            serde_json::json!({
                "would_execute": "set_default_account",
                "args": {"selector": selector}
            }),
            cli.format,
        )),
        Some(Commands::Account {
            subcommand: AccountCommands::SetDefault { selector },
        }) => Some(hyperliquid_cli::commands::wallet::account_set_default(
            selector.as_deref(),
            cli.format,
        )),
        Some(Commands::Account {
            subcommand: AccountCommands::Remove { selector, yes },
        }) if cli.dry_run => Some(print_dry_run(
            "account remove",
            serde_json::json!({
                "would_execute": "remove_stored_account",
                "args": {"selector": selector, "yes": yes}
            }),
            cli.format,
        )),
        Some(Commands::Account {
            subcommand: AccountCommands::Remove { selector, yes },
        }) => Some(hyperliquid_cli::commands::wallet::account_remove(
            selector.as_deref(),
            *yes,
            cli.format,
        )),
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
    command: Option<&CommandContract>,
) -> Result<(), errors::CliError> {
    let Some(command) = command else {
        return Ok(());
    };
    if machine_context(cli) && argv_secret_supplied(&cli.command) {
        return Err(errors::CliError::Unsupported(
            "argv secret input is not supported in machine-readable contexts; use an interactive prompt or a future safe stdin/file secret input"
                .to_string(),
        ));
    }
    if cli.dry_run
        || !machine_context(cli)
        || command.confirmation != ConfirmationPolicy::Prompt
        || !prompt_required_for_invocation(cli, command)
        || confirmation_bypassed(&cli.command)
    {
        return Ok(());
    }

    Err(errors::CliError::Unsupported(format!(
        "{} requires confirmation in machine-readable contexts; rerun with --yes when available or use --dry-run for preview",
        command.command_key()
    )))
}

fn prompt_required_for_invocation(cli: &Cli, command: &CommandContract) -> bool {
    let command_key = command.command_key();
    if cli.testnet
        && matches!(
            command_key.as_str(),
            "orders create"
                | "orders scale"
                | "orders batch-create"
                | "orders tpsl"
                | "orders twap-create"
                | "orders schedule-cancel"
        )
    {
        return false;
    }
    true
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

fn confirmation_bypassed(command: &Option<Commands>) -> bool {
    match command {
        Some(Commands::Builder {
            subcommand: BuilderCommands::Approve(args),
        }) => args.yes,
        Some(Commands::Subaccount {
            subcommand: SubaccountCommands::Transfer(args),
        }) => args.yes,
        Some(Commands::Subaccount {
            subcommand: SubaccountCommands::SpotTransfer(args),
        }) => args.yes,
        Some(Commands::Wallet {
            subcommand: WalletCommands::Reset { yes } | WalletCommands::Delete { yes, .. },
        }) => *yes,
        Some(Commands::Account {
            subcommand: AccountCommands::SetDefault { selector },
        }) => selector.is_some(),
        Some(Commands::Account {
            subcommand: AccountCommands::Remove { yes, .. },
        }) => *yes,
        Some(Commands::Account {
            subcommand:
                AccountCommands::Abstraction(hyperliquid_cli::commands::account::AbstractionArgs {
                    command: Some(hyperliquid_cli::commands::account::AbstractionCommand::Set(args)),
                    ..
                }),
        }) => args.yes,
        Some(Commands::Orders { subcommand }) => match subcommand {
            OrderCommands::Create(args) => args.yes,
            OrderCommands::Scale(args) => args.yes,
            OrderCommands::BatchCreate(args) => args.yes,
            OrderCommands::Tpsl(args) => args.yes,
            OrderCommands::CancelAll(args) => args.yes,
            OrderCommands::TwapCreate(args) => args.yes,
            OrderCommands::ScheduleCancel(args) => args.yes,
            _ => false,
        },
        Some(Commands::Transfer { subcommand }) => match subcommand {
            TransferCommands::SpotToPerp(args) | TransferCommands::PerpToSpot(args) => args.yes,
            TransferCommands::Send(args) => args.yes,
            TransferCommands::SpotSend(args) => args.yes,
            TransferCommands::SendAsset(args) => args.yes,
            TransferCommands::Withdraw(args) => args.yes,
        },
        Some(Commands::Staking {
            subcommand:
                StakingCommands::Link {
                    subcommand:
                        StakingLinkCommands::Initiate(args) | StakingLinkCommands::Finalize(args),
                },
        }) => args.yes,
        _ => false,
    }
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
                hyperliquid_cli::commands::outcomes::parse_outcome_notation(&args.notation)?;
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

fn validate_stream_bounds(cli: &Cli) -> Result<(), errors::CliError> {
    if machine_context(cli) {
        if snapshot_watch_requested_without_bound(&cli.command) {
            validate_env_bound("HYPERLIQUID_WATCH_MAX_TICKS")?;
            if snapshot_watch_requested_without_bound(&cli.command) {
                return Err(errors::CliError::Unsupported(
                    "JSON watch output must be bounded with --max-ticks <N> or HYPERLIQUID_WATCH_MAX_TICKS"
                        .to_string(),
                ));
            }
        }

        if subscribe_stream_args(&cli.command).is_some() {
            validate_env_bound("HYPERLIQUID_SUBSCRIBE_MAX_EVENTS")?;
            if subscribe_requested_without_bound(&cli.command) {
                return Err(errors::CliError::Unsupported(
                    "JSON subscribe output must be bounded with --max-events <N>, --idle-timeout-ms <N>, or HYPERLIQUID_SUBSCRIBE_MAX_EVENTS"
                        .to_string(),
                ));
            }
        }
    }
    Ok(())
}

fn machine_context(cli: &Cli) -> bool {
    cli.format == output::OutputFormat::Json
        || std::env::var("HYPERLIQUID_AGENT").is_ok_and(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
}

fn snapshot_watch_requested_without_bound(command: &Option<Commands>) -> bool {
    snapshot_watch_args(command).is_some_and(|watch| {
        watch.watch && watch.max_ticks.is_none() && env_watch_max_ticks().is_none()
    })
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

fn subscribe_requested_without_bound(command: &Option<Commands>) -> bool {
    subscribe_stream_args(command).is_some_and(|stream| {
        stream.max_events.is_none()
            && stream.idle_timeout_ms.is_none()
            && env_subscribe_max_events().is_none()
    })
}

fn subscribe_stream_args(command: &Option<Commands>) -> Option<&SubscribeStreamArgs> {
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

fn env_watch_max_ticks() -> Option<usize> {
    hyperliquid_cli::watch::max_count_from_env("HYPERLIQUID_WATCH_MAX_TICKS")
        .filter(|value| *value > 0)
}

fn env_subscribe_max_events() -> Option<usize> {
    hyperliquid_cli::watch::max_count_from_env("HYPERLIQUID_SUBSCRIBE_MAX_EVENTS")
}

fn validate_env_bound(name: &str) -> Result<(), errors::CliError> {
    let Ok(raw) = std::env::var(name) else {
        return Ok(());
    };
    raw.trim().parse::<usize>().map(|_| ()).map_err(|err| {
        errors::CliError::Configuration(format!("{name} must be a non-negative integer: {err}"))
    })
}

fn validate_asset_input(label: &str, value: &str) -> Result<(), errors::CliError> {
    use hyperliquid_cli::commands::AssetQuery;
    use hyperliquid_cli::input_hardening::validate_resource_id;

    if matches!(
        hyperliquid_cli::commands::parse_asset_query(value),
        AssetQuery::Outcome(_)
    ) {
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
    command: impl Into<String>,
    details: serde_json::Value,
    format: output::OutputFormat,
) -> Result<(), anyhow::Error> {
    print_dry_run_envelope(DryRunEnvelope::from_details(command, details), format)
}

fn print_dry_run_envelope(
    envelope: DryRunEnvelope,
    format: output::OutputFormat,
) -> Result<(), anyhow::Error> {
    output::print_data_no_timing(&envelope, format);
    Ok(())
}

fn dry_run_details(
    would_execute: &str,
    args: serde_json::Value,
    payload: Option<&serde_json::Value>,
) -> serde_json::Value {
    let mut details = serde_json::Map::new();
    details.insert(
        "would_execute".to_string(),
        serde_json::json!(would_execute),
    );
    details.insert("args".to_string(), args);
    if let Some(payload) = payload {
        details.insert("payload".to_string(), redact_payload(payload.clone()));
    }
    serde_json::Value::Object(details)
}

fn dry_run_signed_details(
    would_execute: &str,
    args: serde_json::Value,
    payload: Option<&serde_json::Value>,
    signer: Option<String>,
    acting_as: Option<String>,
    vault_address: Option<String>,
) -> serde_json::Value {
    let mut details = match dry_run_details(would_execute, args, payload) {
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
    command: &str,
    action: hyperliquid_cli::commands::transfers::TransferActionKind,
    would_execute: &str,
    args: serde_json::Value,
    payload: Option<&serde_json::Value>,
) -> DryRunEnvelope {
    let plan = ActionPlan::signed_exchange_action(
        would_execute,
        action.reversibility(),
        LiveSubmissionPolicy::ValidateConfirmSignSubmit,
    );
    DryRunEnvelope::from_action_plan(command, &plan, args)
        .with_payload(payload.map(|payload| redact_payload(payload.clone())))
}

fn signed_action_dry_run_envelope(
    command: &str,
    would_execute: &str,
    reversibility: ActionReversibility,
    args: serde_json::Value,
    payload: Option<&serde_json::Value>,
    signing_context: DryRunSigningContext,
) -> DryRunEnvelope {
    let plan = ActionPlan::signed_exchange_action(
        would_execute,
        reversibility,
        LiveSubmissionPolicy::ValidateConfirmSignSubmit,
    );
    DryRunEnvelope::from_action_plan(command, &plan, args)
        .with_payload(payload.map(|payload| redact_payload(payload.clone())))
        .with_signing_context(signing_context)
}

fn dry_run_signer_address(context: &AppContext) -> Option<String> {
    dry_run_signing_addresses(context).0
}

fn dry_run_signing_addresses(context: &AppContext) -> (Option<String>, Option<String>) {
    context
        .resolve_signer()
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
    context: &AppContext,
    vault_address: Option<String>,
) -> (Option<String>, Option<String>, Option<String>) {
    let (signer, query_address) = dry_run_signing_addresses(context);
    let acting_as = vault_address.clone().or(query_address);
    (signer, acting_as, vault_address)
}

fn default_dry_run_signing_context(context: &AppContext) -> DryRunSigningContext {
    let (signer, acting_as) = dry_run_signing_addresses(context);
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

/// Runtime settings resolved from CLI flags, environment variables, and config file.
#[derive(Debug)]
struct AppContext {
    private_key: Option<String>,
    keystore: Option<PathBuf>,
    keystore_password: Option<String>,
    account: Option<String>,
    ows_signer: Option<String>,
    network: config::Network,
    api_base_url_override: Option<reqwest::Url>,
}

impl AppContext {
    fn resolve(cli: &Cli) -> Result<Self, anyhow::Error> {
        let testnet = config::resolve_testnet(cli.testnet)?;
        let network = if testnet {
            config::Network::Testnet
        } else {
            config::Network::Mainnet
        };
        let private_key = if cli.account.is_some() || cli.ows_signer.is_some() {
            None
        } else {
            config::resolve_private_key(cli.private_key.as_deref())?
        };
        Ok(Self {
            private_key,
            keystore: cli.keystore.clone(),
            keystore_password: cli.keystore_password.clone(),
            account: cli.account.clone(),
            ows_signer: cli.ows_signer.clone(),
            network,
            api_base_url_override: config::resolve_api_base_url_override_for_network(network)?,
        })
    }

    fn api_base_url(&self) -> String {
        self.api_base_url_override
            .as_ref()
            .map(std::string::ToString::to_string)
            .unwrap_or_else(|| {
                config::api_base_url(self.network == config::Network::Testnet).to_string()
            })
    }

    fn chain(&self) -> Chain {
        match self.network {
            config::Network::Mainnet => Chain::Mainnet,
            config::Network::Testnet => Chain::Testnet,
        }
    }

    fn http_client(&self) -> HttpClient {
        let client = HttpClient::new(self.chain());
        if let Some(api_base_url) = self.api_base_url_override.as_ref() {
            client.with_url(api_base_url.clone())
        } else {
            client
        }
    }

    fn resolve_signer(&self) -> Result<hyperliquid_cli::auth::ResolvedSigner, anyhow::Error> {
        hyperliquid_cli::resolvers::resolve_selected_signer(SignerResolverInput {
            resolved_private_key: self.private_key.as_deref(),
            keystore_path: self.keystore.as_deref(),
            keystore_password: self.keystore_password.as_deref(),
            account_selector: self.account.as_deref(),
            ows_selector: self.ows_signer.as_deref(),
            default_fallback: DefaultSignerFallback::AllowStoredDefaultOrFirst,
        })
    }

    fn has_api_base_url_override(&self) -> bool {
        self.api_base_url_override.is_some()
    }

    async fn validate_api_override(&self) -> Result<(), errors::CliError> {
        let Some(api_base_url) = self.api_base_url_override.as_ref() else {
            return Ok(());
        };

        let mut url = api_base_url.clone();
        url.set_path("/info");
        url.set_query(None);

        let response = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .map_err(|err| errors::CliError::Internal(anyhow::anyhow!(err)))?
            .post(url)
            .json(&serde_json::json!({ "type": "allMids" }))
            .send()
            .await
            .map_err(|err| {
                errors::CliError::Unavailable(format!("Check your network connection. {err}"))
            })?;

        let status = response.status();
        let body = response.text().await.map_err(|err| {
            errors::CliError::Unavailable(format!("Failed to read API response. {err}"))
        })?;

        if errors::http_response_indicates_rate_limit(status.as_u16(), &body) {
            return Err(errors::CliError::RateLimited);
        }

        if !status.is_success() {
            return Err(errors::CliError::Unavailable(format!(
                "API returned HTTP {status}. Check your network connection."
            )));
        }

        Ok(())
    }
}

fn order_submission_context<'a>(
    context: &AppContext,
    api_base_url: &'a str,
    signer: &'a hyperliquid_cli::signing::SelectedSigner,
) -> hyperliquid_cli::commands::orders::OrderSubmissionContext<'a> {
    hyperliquid_cli::commands::orders::OrderSubmissionContext {
        api_base_url,
        chain: context.chain(),
        signer,
        require_mainnet_confirmation: context.network == config::Network::Mainnet,
    }
}

fn order_execution_context<'a>(
    context: &AppContext,
    api_base_url: &'a str,
    client: &'a HttpClient,
    resolver: &'a AssetResolver,
    signer: &'a hyperliquid_cli::signing::SelectedSigner,
) -> hyperliquid_cli::commands::orders::OrderExecutionContext<'a> {
    hyperliquid_cli::commands::orders::OrderExecutionContext {
        submission: order_submission_context(context, api_base_url, signer),
        client,
        resolver,
    }
}

async fn resolve_and_print_perp(
    context: &AppContext,
    args: &hyperliquid_cli::commands::perps::PerpsGetArgs,
    format: output::OutputFormat,
) -> Result<(), anyhow::Error> {
    let client = context.http_client();
    let resolver = load_asset_resolver(context).await?;
    hyperliquid_cli::commands::perps::get(
        &client,
        &resolver,
        &args.coin,
        args.dex.as_deref(),
        format,
    )
    .await
}

async fn resolve_and_print_spot(
    context: &AppContext,
    pair: &str,
    format: output::OutputFormat,
) -> Result<(), anyhow::Error> {
    let client = context.http_client();
    let resolver = load_asset_resolver(context).await?;
    hyperliquid_cli::commands::spot::get(&client, &resolver, pair, format).await
}

async fn resolve_and_print_book(
    context: &AppContext,
    cli: &Cli,
    coin: &str,
    payload_present: bool,
) -> Result<(), anyhow::Error> {
    let resolver = load_asset_resolver(context).await?;
    let command_context = cli_command_context(context, cli, None, payload_present);
    hyperliquid_cli::commands::orderbook::book_with_context(
        &command_context,
        context.chain(),
        &resolver,
        coin,
    )
    .await
}

async fn resolve_and_print_candles(
    context: &AppContext,
    cli: &Cli,
    coin: &str,
    interval: hypersdk::hypercore::CandleInterval,
    limit: usize,
    payload_present: bool,
) -> Result<(), anyhow::Error> {
    let client = context.http_client();
    let resolver = load_asset_resolver(context).await?;
    let command_context = cli_command_context(context, cli, Some(&client), payload_present);
    hyperliquid_cli::commands::orderbook::candles_with_context(
        &command_context,
        &resolver,
        coin,
        interval,
        limit,
    )
    .await
}

async fn resolve_and_print_spread(
    context: &AppContext,
    cli: &Cli,
    coin: &str,
    payload_present: bool,
) -> Result<(), anyhow::Error> {
    let resolver = load_asset_resolver(context).await?;
    let command_context = cli_command_context(context, cli, None, payload_present);
    hyperliquid_cli::commands::orderbook::spread_with_context(
        &command_context,
        context.chain(),
        &resolver,
        coin,
    )
    .await
}

async fn resolve_and_print_funding(
    context: &AppContext,
    cli: &Cli,
    coin: &str,
    payload_present: bool,
) -> Result<(), anyhow::Error> {
    let client = context.http_client();
    let resolver = load_asset_resolver(context).await?;
    let command_context = cli_command_context(context, cli, Some(&client), payload_present);
    hyperliquid_cli::commands::orderbook::funding_with_context(
        &command_context,
        context.chain(),
        &resolver,
        coin,
    )
    .await
}

async fn print_mids(
    context: &AppContext,
    cli: &Cli,
    payload_present: bool,
) -> Result<(), anyhow::Error> {
    let client = context.http_client();
    let command_context = cli_command_context(context, cli, Some(&client), payload_present);
    hyperliquid_cli::commands::orderbook::mids_with_context(&command_context, cli.select.as_deref())
        .await
}

async fn watch_book(
    context: &AppContext,
    coin: &str,
    format: output::OutputFormat,
    max_ticks: Option<usize>,
) -> Result<(), anyhow::Error> {
    let resolver = std::sync::Arc::new(load_asset_resolver(context).await?);
    let subscription_coin = resolve_subscription_coin(&resolver, coin)?;
    let chain = context.chain();

    hyperliquid_cli::watch::run_snapshot_watch(
        &format!("Live order book: {subscription_coin}"),
        chain,
        vec![Subscription::L2Book {
            coin: subscription_coin,
            n_sig_figs: None,
            mantissa: None,
        }],
        format,
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

async fn watch_mids(
    context: &AppContext,
    format: output::OutputFormat,
    select: Option<&str>,
    max_ticks: Option<usize>,
) -> Result<(), anyhow::Error> {
    let client = std::sync::Arc::new(context.http_client());
    let chain = context.chain();
    let select = select.map(str::to_string);

    hyperliquid_cli::watch::run_snapshot_watch(
        "Live mid prices",
        chain,
        vec![Subscription::AllMids { dex: None }],
        format,
        max_ticks,
        move || {
            let client = std::sync::Arc::clone(&client);
            let select = select.clone();
            async move {
                hyperliquid_cli::commands::orderbook::mids_snapshot(&client, select.as_deref())
                    .await
            }
        },
    )
    .await
}

async fn watch_candles(
    context: &AppContext,
    coin: &str,
    interval: hypersdk::hypercore::CandleInterval,
    limit: usize,
    format: output::OutputFormat,
    max_ticks: Option<usize>,
) -> Result<(), anyhow::Error> {
    let client = std::sync::Arc::new(context.http_client());
    let resolver = std::sync::Arc::new(load_asset_resolver(context).await?);
    let subscription_coin = resolve_subscription_coin(&resolver, coin)?;
    let chain = context.chain();

    hyperliquid_cli::watch::run_snapshot_watch(
        &format!("Live candles: {subscription_coin} {interval}"),
        chain,
        vec![Subscription::Candle {
            coin: subscription_coin,
            interval: interval.to_string(),
        }],
        format,
        max_ticks,
        move || {
            let client = std::sync::Arc::clone(&client);
            let resolver = std::sync::Arc::clone(&resolver);
            async move {
                hyperliquid_cli::commands::orderbook::candles_snapshot(
                    &client, &resolver, coin, interval, limit,
                )
                .await
            }
        },
    )
    .await
}

async fn watch_open_orders(
    context: &AppContext,
    format: output::OutputFormat,
    max_ticks: Option<usize>,
) -> Result<(), anyhow::Error> {
    let client = std::sync::Arc::new(context.http_client());
    let resolver = std::sync::Arc::new(load_asset_resolver(context).await?);
    let resolved = context.resolve_signer()?;
    let user = resolved.query_address();

    hyperliquid_cli::watch::run_snapshot_watch(
        "Live open orders",
        context.chain(),
        vec![Subscription::OrderUpdates { user }],
        format,
        max_ticks,
        move || {
            let client = std::sync::Arc::clone(&client);
            let resolver = std::sync::Arc::clone(&resolver);
            async move {
                hyperliquid_cli::commands::orders::open_snapshot(&client, &resolver, user).await
            }
        },
    )
    .await
}

async fn watch_positions(
    context: &AppContext,
    format: output::OutputFormat,
    max_ticks: Option<usize>,
) -> Result<(), anyhow::Error> {
    let client = std::sync::Arc::new(context.http_client());
    let resolved = context.resolve_signer()?;
    let user = resolved.query_address();

    hyperliquid_cli::watch::run_snapshot_watch(
        "Live positions",
        context.chain(),
        vec![Subscription::WebData2 { user, dex: None }],
        format,
        max_ticks,
        move || {
            let client = std::sync::Arc::clone(&client);
            async move { hyperliquid_cli::commands::positions::list_snapshot(&client, user).await }
        },
    )
    .await
}

async fn subscribe(context: &AppContext, command: &SubscribeCommands) -> Result<(), anyhow::Error> {
    match command {
        SubscribeCommands::Trades { asset, stream } => {
            let resolver = load_asset_resolver(context).await?;
            let coin = resolve_subscription_coin(&resolver, asset)?;
            hyperliquid_cli::watch::stream_subscription(
                context.chain(),
                Subscription::Trades { coin },
                stream.max_events,
                stream.idle_timeout(),
                |message| subscription_event_matches(SubscribeEventKind::Trades, message),
            )
            .await
        }
        SubscribeCommands::Orderbook { asset, stream } => {
            let resolver = load_asset_resolver(context).await?;
            let coin = resolve_subscription_coin(&resolver, asset)?;
            hyperliquid_cli::watch::stream_subscription(
                context.chain(),
                Subscription::L2Book {
                    coin,
                    n_sig_figs: None,
                    mantissa: None,
                },
                stream.max_events,
                stream.idle_timeout(),
                |message| subscription_event_matches(SubscribeEventKind::Orderbook, message),
            )
            .await
        }
        SubscribeCommands::Candles {
            asset,
            interval,
            stream,
        } => {
            hyperliquid_cli::commands::orderbook::parse_candle_interval(interval)
                .map_err(errors::CliError::Configuration)?;
            let resolver = load_asset_resolver(context).await?;
            let coin = resolve_subscription_coin(&resolver, asset)?;
            hyperliquid_cli::watch::stream_subscription(
                context.chain(),
                Subscription::Candle {
                    coin,
                    interval: interval.clone(),
                },
                stream.max_events,
                stream.idle_timeout(),
                |message| subscription_event_matches(SubscribeEventKind::Candles, message),
            )
            .await
        }
        SubscribeCommands::AllMids { stream } => {
            hyperliquid_cli::watch::stream_subscription(
                context.chain(),
                Subscription::AllMids { dex: None },
                stream.max_events,
                stream.idle_timeout(),
                |message| subscription_event_matches(SubscribeEventKind::AllMids, message),
            )
            .await
        }
        SubscribeCommands::OrderUpdates { stream } => {
            let user = context.resolve_signer()?.query_address();
            hyperliquid_cli::watch::stream_subscription(
                context.chain(),
                Subscription::OrderUpdates { user },
                stream.max_events,
                stream.idle_timeout(),
                |message| subscription_event_matches(SubscribeEventKind::OrderUpdates, message),
            )
            .await
        }
        SubscribeCommands::Fills { stream } => {
            let user = context.resolve_signer()?.query_address();
            hyperliquid_cli::watch::stream_subscription(
                context.chain(),
                Subscription::UserFills { user },
                stream.max_events,
                stream.idle_timeout(),
                |message| subscription_event_matches(SubscribeEventKind::Fills, message),
            )
            .await
        }
    }
}

fn resolve_subscription_coin(
    resolver: &AssetResolver,
    asset: &str,
) -> Result<String, errors::CliError> {
    hyperliquid_cli::commands::orderbook::resolve_subscription_info_coin(resolver, asset)
}

fn resolve_protocol_user_string(selector: &str) -> Result<String, anyhow::Error> {
    Ok(resolve_protocol_user_address(selector)?.to_string())
}

fn resolve_protocol_user_address(selector: &str) -> Result<Address, anyhow::Error> {
    Ok(hyperliquid_cli::resolvers::resolve_protocol_user_address(selector)?.address())
}

fn resolve_protocol_user_string_or_selected(
    context: &AppContext,
    selector: Option<&str>,
) -> Result<String, anyhow::Error> {
    match selector {
        Some(selector) => resolve_protocol_user_string(selector),
        None => Ok(context.resolve_signer()?.query_address().to_string()),
    }
}

fn resolve_acting_account_address(selector: &str) -> Result<Address, anyhow::Error> {
    Ok(hyperliquid_cli::resolvers::resolve_acting_account_address(selector)?.address())
}

fn resolve_optional_acting_account_target(
    selector: Option<&str>,
) -> Result<Option<Address>, anyhow::Error> {
    selector.map(resolve_acting_account_address).transpose()
}

async fn print_account_fills(
    context: &AppContext,
    cli: &Cli,
    args: &hyperliquid_cli::commands::account::FillsArgs,
    payload_present: bool,
) -> Result<(), anyhow::Error> {
    let client = context.http_client();
    let address = resolve_protocol_user_string_or_selected(context, args.address.as_deref())?;
    let command_context = cli_command_context(context, cli, Some(&client), payload_present);
    hyperliquid_cli::commands::account::fills_with_context(&command_context, &address, args).await
}

async fn print_account_fees(
    context: &AppContext,
    cli: &Cli,
    args: &hyperliquid_cli::commands::account::AddressArgs,
    payload_present: bool,
) -> Result<(), anyhow::Error> {
    let address = resolve_protocol_user_string_or_selected(context, args.address.as_deref())?;
    let command_context = cli_command_context(context, cli, None, payload_present);
    hyperliquid_cli::commands::account::fees_with_context(&command_context, &address).await
}

async fn print_account_rate_limit(
    context: &AppContext,
    cli: &Cli,
    args: &hyperliquid_cli::commands::account::AddressArgs,
    payload_present: bool,
) -> Result<(), anyhow::Error> {
    let address = resolve_protocol_user_string_or_selected(context, args.address.as_deref())?;
    let command_context = cli_command_context(context, cli, None, payload_present);
    hyperliquid_cli::commands::account::rate_limit_with_context(&command_context, &address).await
}

async fn print_account_orders(
    context: &AppContext,
    cli: &Cli,
    address: Option<&str>,
    payload_present: bool,
) -> Result<(), anyhow::Error> {
    let client = context.http_client();
    let address = resolve_protocol_user_string_or_selected(context, address)?;
    let command_context = cli_command_context(context, cli, Some(&client), payload_present);
    hyperliquid_cli::commands::account::orders_with_context(&command_context, &address).await
}

async fn print_account_portfolio(
    context: &AppContext,
    cli: &Cli,
    address: Option<&str>,
    payload_present: bool,
) -> Result<(), anyhow::Error> {
    let client = context.http_client();
    let address = resolve_protocol_user_string_or_selected(context, address)?;
    let command_context = cli_command_context(context, cli, Some(&client), payload_present);
    hyperliquid_cli::commands::account::portfolio_with_context(&command_context, &address).await
}

async fn print_account_subaccounts(
    context: &AppContext,
    cli: &Cli,
    address: Option<&str>,
    payload_present: bool,
) -> Result<(), anyhow::Error> {
    let client = context.http_client();
    let address = resolve_protocol_user_string_or_selected(context, address)?;
    let command_context = cli_command_context(context, cli, Some(&client), payload_present);
    hyperliquid_cli::commands::account::subaccounts_with_context(&command_context, &address).await
}

async fn api_wallet_create(
    context: &AppContext,
    args: &hyperliquid_cli::commands::api_wallet::CreateArgs,
    format: output::OutputFormat,
    dry_run: bool,
) -> Result<(), anyhow::Error> {
    let signer = api_wallet_approval_signer(context, dry_run)?;
    hyperliquid_cli::commands::api_wallet::create(
        &context.api_base_url(),
        context.chain(),
        signer.as_ref(),
        args,
        dry_run,
        format,
    )
    .await
}

async fn api_wallet_approve(
    context: &AppContext,
    args: &hyperliquid_cli::commands::api_wallet::ApproveArgs,
    format: output::OutputFormat,
    dry_run: bool,
) -> Result<(), anyhow::Error> {
    let signer = api_wallet_approval_signer(context, dry_run)?;
    hyperliquid_cli::commands::api_wallet::approve(
        &context.api_base_url(),
        context.chain(),
        signer.as_ref(),
        args,
        dry_run,
        format,
    )
    .await
}

async fn api_wallet_list(
    context: &AppContext,
    account: Option<&str>,
    format: output::OutputFormat,
) -> Result<(), anyhow::Error> {
    let master_address = match account {
        Some(account) => resolve_protocol_user_address(account)?,
        None => context.resolve_signer()?.query_address(),
    };
    let client = context.http_client();
    hyperliquid_cli::commands::api_wallet::list(&client, master_address, format).await
}

async fn api_wallet_revoke(
    context: &AppContext,
    args: &hyperliquid_cli::commands::api_wallet::RevokeArgs,
    format: output::OutputFormat,
    dry_run: bool,
) -> Result<(), anyhow::Error> {
    if args.name.chars().count() > hyperliquid_cli::commands::api_wallet::MAX_AGENT_NAME_LEN {
        return Err(errors::CliError::Configuration(format!(
            "--name must be between 1 and {} characters",
            hyperliquid_cli::commands::api_wallet::MAX_AGENT_NAME_LEN
        ))
        .into());
    }
    let signer = api_wallet_approval_signer(context, dry_run)?;
    hyperliquid_cli::commands::api_wallet::revoke(
        &context.api_base_url(),
        context.chain(),
        signer.as_ref(),
        args,
        dry_run,
        format,
    )
    .await
}

fn api_wallet_approval_signer(
    context: &AppContext,
    dry_run: bool,
) -> Result<Option<hyperliquid_cli::signing::SelectedSigner>, anyhow::Error> {
    if dry_run {
        return Ok(None);
    }
    let resolved = context.resolve_signer()?;
    if resolved.query_address() != resolved.address() {
        return Err(errors::CliError::Unsupported(
            "approveAgent must be signed by the master account; select the master wallet, not a stored API wallet"
                .to_string(),
        )
        .into());
    }
    Ok(Some(resolved.selected_signer()))
}

async fn print_account_portfolio_history(
    context: &AppContext,
    cli: &Cli,
    args: &hyperliquid_cli::commands::account::AddressArgs,
    payload_present: bool,
) -> Result<(), anyhow::Error> {
    let address = resolve_protocol_user_string_or_selected(context, args.address.as_deref())?;
    let command_context = cli_command_context(context, cli, None, payload_present);
    hyperliquid_cli::commands::account::portfolio_history_with_context(&command_context, &address)
        .await
}

async fn print_account_ledger(
    context: &AppContext,
    cli: &Cli,
    args: &hyperliquid_cli::commands::account::TimeRangeArgs,
    payload_present: bool,
) -> Result<(), anyhow::Error> {
    let address = resolve_protocol_user_string_or_selected(context, args.address.as_deref())?;
    let command_context = cli_command_context(context, cli, None, payload_present);
    hyperliquid_cli::commands::account::ledger_with_context(&command_context, &address, args).await
}

async fn print_account_funding(
    context: &AppContext,
    cli: &Cli,
    args: &hyperliquid_cli::commands::account::TimeRangeArgs,
    payload_present: bool,
) -> Result<(), anyhow::Error> {
    let address = resolve_protocol_user_string_or_selected(context, args.address.as_deref())?;
    let command_context = cli_command_context(context, cli, None, payload_present);
    hyperliquid_cli::commands::account::funding_with_context(&command_context, &address, args).await
}

async fn print_account_twap_history(
    context: &AppContext,
    cli: &Cli,
    args: &hyperliquid_cli::commands::account::AddressArgs,
    payload_present: bool,
) -> Result<(), anyhow::Error> {
    let address = resolve_protocol_user_string_or_selected(context, args.address.as_deref())?;
    let command_context = cli_command_context(context, cli, None, payload_present);
    hyperliquid_cli::commands::account::twap_history_with_context(&command_context, &address).await
}

async fn print_account_twap_fills(
    context: &AppContext,
    cli: &Cli,
    args: &hyperliquid_cli::commands::account::TwapFillsArgs,
    payload_present: bool,
) -> Result<(), anyhow::Error> {
    let address = resolve_protocol_user_string_or_selected(context, args.address.as_deref())?;
    let command_context = cli_command_context(context, cli, None, payload_present);
    hyperliquid_cli::commands::account::twap_fills_with_context(&command_context, &address, args)
        .await
}

async fn print_outcomes_list(
    context: &AppContext,
    args: &hyperliquid_cli::commands::outcomes::OutcomeListArgs,
    format: output::OutputFormat,
) -> Result<(), anyhow::Error> {
    hyperliquid_cli::commands::outcomes::list(&context.api_base_url(), args, format).await
}

async fn print_outcomes_get(
    context: &AppContext,
    args: &hyperliquid_cli::commands::outcomes::OutcomeGetArgs,
    format: output::OutputFormat,
) -> Result<(), anyhow::Error> {
    hyperliquid_cli::commands::outcomes::get(&context.api_base_url(), args, format).await
}

async fn builder_max_fee(
    context: &AppContext,
    args: &hyperliquid_cli::commands::builder::MaxFeeArgs,
    format: output::OutputFormat,
) -> Result<(), anyhow::Error> {
    let user = resolve_protocol_user_address(&args.user)?;
    let builder = hyperliquid_cli::commands::builder::parse_builder_address(&args.builder)?;
    hyperliquid_cli::commands::builder::max_fee(&context.api_base_url(), user, builder, format)
        .await
}

async fn builder_approved(
    context: &AppContext,
    args: &hyperliquid_cli::commands::builder::ApprovedArgs,
    format: output::OutputFormat,
) -> Result<(), anyhow::Error> {
    let user = resolve_protocol_user_address(&args.user)?;
    hyperliquid_cli::commands::builder::approved(&context.api_base_url(), user, format).await
}

async fn builder_approve(
    context: &AppContext,
    args: &hyperliquid_cli::commands::builder::ApproveArgs,
    format: output::OutputFormat,
    dry_run: bool,
    payload: Option<&serde_json::Value>,
) -> Result<(), anyhow::Error> {
    let builder = hyperliquid_cli::commands::builder::parse_builder_address(&args.builder)?;
    hyperliquid_cli::commands::builder::validate_max_fee_rate(&args.max_fee_rate)?;
    if dry_run {
        let signer_context = context.resolve_signer().ok();
        let preview_args = hyperliquid_cli::commands::builder::approve_dry_run_value(
            context.chain(),
            signer_context.as_ref().map(|resolved| resolved.address()),
            signer_context
                .as_ref()
                .map(|resolved| resolved.query_address()),
            builder,
            args,
        )?;
        let envelope = signed_action_dry_run_envelope(
            "builder approve",
            "approve_builder_fee",
            hyperliquid_cli::commands::builder::BuilderActionKind::ApproveFee.reversibility(),
            preview_args,
            payload,
            default_dry_run_signing_context(context),
        );
        return print_dry_run_envelope(envelope, format);
    }
    let resolved = context.resolve_signer()?;
    if resolved.query_address() != resolved.address() {
        return Err(errors::CliError::Unsupported(
            "approveBuilderFee must be signed by the master account; select the master wallet, not a stored API wallet"
                .to_string(),
        )
        .into());
    }
    hyperliquid_cli::commands::builder::approve(
        &context.api_base_url(),
        context.chain(),
        &resolved.selected_signer(),
        builder,
        args,
        true,
        format,
    )
    .await
}

async fn account_abstraction(
    context: &AppContext,
    cli: &Cli,
    args: &hyperliquid_cli::commands::account::AbstractionArgs,
    payload: Option<&serde_json::Value>,
) -> Result<(), anyhow::Error> {
    use hyperliquid_cli::commands::account::{
        AbstractionCommand, abstraction_set_dry_run_value, abstraction_with_context,
        set_abstraction,
    };

    match args.command.as_ref() {
        Some(AbstractionCommand::Set(set_args)) => {
            if cli.dry_run {
                let signer_address = context.resolve_signer().ok().map(|r| r.address());
                let fallback_address: Address = "0x0000000000000000000000000000000000000001"
                    .parse()
                    .expect("valid placeholder address");
                let preview_args = abstraction_set_dry_run_value(
                    context.chain(),
                    signer_address.unwrap_or(fallback_address),
                    set_args,
                );
                let signer = dry_run_signer_address(context);
                return print_dry_run(
                    "account abstraction set",
                    dry_run_signed_details(
                        "set_abstraction",
                        preview_args,
                        payload,
                        signer,
                        None,
                        None,
                    ),
                    cli.format,
                );
            }
            let resolved = context.resolve_signer()?;
            if resolved.query_address() != resolved.address() {
                return Err(errors::CliError::Unsupported(
                    "userSetAbstraction must be signed by the master account; select the master wallet, not a stored API wallet"
                        .to_string(),
                )
                .into());
            }
            set_abstraction(
                &context.api_base_url(),
                context.chain(),
                &resolved.selected_signer(),
                set_args,
                true,
                cli.format,
            )
            .await
        }
        None => {
            let address = match args.address.as_deref() {
                Some(addr) => resolve_protocol_user_string(addr)?,
                None => context.resolve_signer()?.query_address().to_string(),
            };
            let command_context = cli_command_context(context, cli, None, payload.is_some());
            abstraction_with_context(&command_context, &address).await
        }
    }
}

async fn subaccount_create(
    context: &AppContext,
    args: &hyperliquid_cli::commands::subaccounts::CreateArgs,
    format: output::OutputFormat,
    dry_run: bool,
    payload: Option<&serde_json::Value>,
) -> Result<(), anyhow::Error> {
    hyperliquid_cli::commands::subaccounts::validate_create_args(args)?;
    if dry_run {
        let envelope = signed_action_dry_run_envelope(
            "subaccount create",
            "create_subaccount",
            hyperliquid_cli::commands::subaccounts::SubaccountActionKind::Create.reversibility(),
            hyperliquid_cli::commands::subaccounts::create_dry_run_value(context.chain(), args),
            payload,
            default_dry_run_signing_context(context),
        );
        return print_dry_run_envelope(envelope, format);
    }
    let resolved = context.resolve_signer()?;
    let signer = resolved.selected_signer();
    hyperliquid_cli::commands::subaccounts::create(
        &context.api_base_url(),
        context.chain(),
        &signer,
        args,
        format,
    )
    .await
}

async fn subaccount_transfer(
    context: &AppContext,
    args: &hyperliquid_cli::commands::subaccounts::TransferArgs,
    format: output::OutputFormat,
    dry_run: bool,
    payload: Option<&serde_json::Value>,
) -> Result<(), anyhow::Error> {
    hyperliquid_cli::commands::subaccounts::validate_transfer_args(args)?;
    let subaccount = resolve_acting_account_address(&args.subaccount)?;
    if dry_run {
        let envelope = signed_action_dry_run_envelope(
            "subaccount transfer",
            "subaccount_usdc_transfer",
            hyperliquid_cli::commands::subaccounts::SubaccountActionKind::TransferUsdc
                .reversibility(),
            hyperliquid_cli::commands::subaccounts::transfer_dry_run_value(
                context.chain(),
                subaccount,
                args,
            )?,
            payload,
            default_dry_run_signing_context(context),
        );
        return print_dry_run_envelope(envelope, format);
    }
    let resolved = context.resolve_signer()?;
    let signer = resolved.selected_signer();
    hyperliquid_cli::commands::subaccounts::transfer(
        &context.api_base_url(),
        context.chain(),
        &signer,
        subaccount,
        args,
        format,
    )
    .await
}

async fn subaccount_spot_transfer(
    context: &AppContext,
    args: &hyperliquid_cli::commands::subaccounts::SpotTransferArgs,
    format: output::OutputFormat,
    dry_run: bool,
    payload: Option<&serde_json::Value>,
) -> Result<(), anyhow::Error> {
    hyperliquid_cli::commands::subaccounts::validate_spot_transfer_args(args)?;
    let subaccount = resolve_acting_account_address(&args.subaccount)?;
    if dry_run {
        let envelope = signed_action_dry_run_envelope(
            "subaccount spot-transfer",
            "subaccount_spot_transfer",
            hyperliquid_cli::commands::subaccounts::SubaccountActionKind::TransferSpot
                .reversibility(),
            hyperliquid_cli::commands::subaccounts::spot_transfer_dry_run_value(
                context.chain(),
                subaccount,
                args,
            )?,
            payload,
            default_dry_run_signing_context(context),
        );
        return print_dry_run_envelope(envelope, format);
    }
    let resolved = context.resolve_signer()?;
    let signer = resolved.selected_signer();
    hyperliquid_cli::commands::subaccounts::spot_transfer(
        &context.api_base_url(),
        context.chain(),
        &signer,
        subaccount,
        args,
        format,
    )
    .await
}

async fn print_status(
    context: &AppContext,
    cli: &Cli,
    payload_present: bool,
) -> Result<(), anyhow::Error> {
    let client = context.http_client();
    let command_context = cli_command_context(context, cli, Some(&client), payload_present);

    hyperliquid_cli::commands::status::show_with_context(&command_context).await
}

async fn print_meta(
    context: &AppContext,
    cli: &Cli,
    payload_present: bool,
) -> Result<(), anyhow::Error> {
    let client = context.http_client();
    let command_context = cli_command_context(context, cli, Some(&client), payload_present);

    hyperliquid_cli::commands::meta::show_with_context(&command_context).await
}

fn cli_command_context<'a>(
    context: &AppContext,
    cli: &Cli,
    client: Option<&'a HttpClient>,
    payload_present: bool,
) -> CommandContext<'a> {
    let command_context = CommandContext::new(
        context.network.to_string(),
        context.api_base_url().to_string(),
        CommandOutputContext::new(
            cli.format,
            cli.select.as_deref(),
            cli.results_only,
            cli.max_results,
        ),
        CommandTransportPolicy::CliProcess,
    );
    let command_context = if let Some(client) = client {
        command_context.with_clients(CommandClients::with_hypercore(client))
    } else {
        command_context
    };
    command_context
        .with_account_selector(cli.account.as_deref())
        .with_dry_run(cli.dry_run)
        .with_payload(PayloadMetadata::from_presence(payload_present))
}

async fn create_order(
    context: &AppContext,
    args: &hyperliquid_cli::commands::orders::CreateArgs,
    format: output::OutputFormat,
    dry_run: bool,
    payload: Option<&serde_json::Value>,
) -> Result<(), anyhow::Error> {
    hyperliquid_cli::commands::orders::validate_create_args(args)?;
    let client = context.http_client();
    let resolver = load_trading_resolver(context).await?;
    let coin_query = qualify_dex_asset(args.dex.as_deref(), &args.coin);
    let vault_address = resolve_optional_acting_account_target(args.on_behalf_of.as_deref())?;
    let vault_address_string = vault_address.map(|address| address.to_string());
    if dry_run {
        let plan = hyperliquid_cli::commands::orders::create_dry_run_plan(&client, &resolver, args)
            .await?;
        let (signer, acting_as, vault_address_string) =
            dry_run_vault_signing_addresses(context, vault_address_string);
        return print_dry_run(
            plan.command(),
            dry_run_signed_details(
                plan.would_execute(),
                plan.into_args(),
                payload,
                signer,
                acting_as,
                vault_address_string,
            ),
            format,
        );
    }
    let margin_validation_target = if !matches!(
        hyperliquid_cli::commands::parse_asset_query(&args.coin),
        hyperliquid_cli::commands::AssetQuery::Outcome(_)
    ) {
        let resolved = resolve_asset_or_load_hip3(&client, &resolver, &coin_query).await?;
        hyperliquid_cli::commands::orders::validate_create_resolved_asset(args, &resolved)?;
        match resolved {
            hyperliquid_cli::commands::ResolvedAsset::Perp { name, dex, .. } => Some((name, dex)),
            hyperliquid_cli::commands::ResolvedAsset::Spot { .. } => None,
        }
    } else if args.margin_mode.is_some() {
        return Err(errors::CliError::Configuration(
            "orders create --margin-mode is only supported for perpetual orders".to_string(),
        )
        .into());
    } else {
        None
    };
    let resolved_signer = context.resolve_signer()?;
    if let Some((coin, dex)) = margin_validation_target {
        let margin_mode_user = vault_address.unwrap_or_else(|| resolved_signer.query_address());
        validate_order_margin_mode(
            &client,
            margin_mode_user,
            coin.as_str(),
            dex.as_deref(),
            args.margin_mode,
        )
        .await?;
    }
    let signer = resolved_signer.selected_signer();
    let api_base_url = context.api_base_url();
    hyperliquid_cli::commands::orders::create(
        order_execution_context(context, &api_base_url, &client, &resolver, &signer),
        args,
        vault_address,
        format,
    )
    .await
}

async fn scale_orders(
    context: &AppContext,
    args: &hyperliquid_cli::commands::orders::ScaleArgs,
    format: output::OutputFormat,
    dry_run: bool,
    payload: Option<&serde_json::Value>,
) -> Result<(), anyhow::Error> {
    hyperliquid_cli::commands::orders::validate_scale_args(args)?;
    let client = context.http_client();
    let resolver = load_trading_resolver(context).await?;
    let vault_address = resolve_optional_acting_account_target(args.on_behalf_of.as_deref())?;
    let vault_address_string = vault_address.map(|address| address.to_string());
    if dry_run {
        let plan =
            hyperliquid_cli::commands::orders::scale_dry_run_plan(&client, &resolver, args).await?;
        let (signer, acting_as, vault_address_string) =
            dry_run_vault_signing_addresses(context, vault_address_string);
        return print_dry_run(
            plan.command(),
            dry_run_signed_details(
                plan.would_execute(),
                plan.into_args(),
                payload,
                signer,
                acting_as,
                vault_address_string,
            ),
            format,
        );
    }
    let coin_query = qualify_dex_asset(args.dex.as_deref(), &args.coin);
    let margin_validation_target = if !matches!(
        hyperliquid_cli::commands::parse_asset_query(&args.coin),
        hyperliquid_cli::commands::AssetQuery::Outcome(_)
    ) {
        match resolve_asset_or_load_hip3(&client, &resolver, &coin_query).await? {
            hyperliquid_cli::commands::ResolvedAsset::Perp { name, dex, .. } => Some((name, dex)),
            hyperliquid_cli::commands::ResolvedAsset::Spot { .. } => {
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
    let resolved_signer = context.resolve_signer()?;
    if let Some((coin, dex)) = margin_validation_target {
        let margin_mode_user = vault_address.unwrap_or_else(|| resolved_signer.query_address());
        validate_order_margin_mode(
            &client,
            margin_mode_user,
            coin.as_str(),
            dex.as_deref(),
            args.margin_mode,
        )
        .await?;
    }
    let signer = resolved_signer.selected_signer();
    let api_base_url = context.api_base_url();
    hyperliquid_cli::commands::orders::scale(
        order_execution_context(context, &api_base_url, &client, &resolver, &signer),
        args,
        vault_address,
        format,
    )
    .await
}

async fn batch_create_orders(
    context: &AppContext,
    args: &hyperliquid_cli::commands::orders::BatchCreateArgs,
    format: output::OutputFormat,
    dry_run: bool,
    payload: Option<&serde_json::Value>,
) -> Result<(), anyhow::Error> {
    hyperliquid_cli::commands::orders::validate_batch_create_args(args)?;
    let orders = hyperliquid_cli::commands::orders::read_validated_batch_create_orders(args)?;
    let client = context.http_client();
    let resolver = load_trading_resolver(context).await?;
    let vault_address = resolve_optional_acting_account_target(args.on_behalf_of.as_deref())?;
    let vault_address_string = vault_address.map(|address| address.to_string());
    if dry_run {
        let plan = hyperliquid_cli::commands::orders::batch_create_dry_run_plan(
            &client, &resolver, args, orders,
        )
        .await?;
        let (signer, acting_as, vault_address_string) =
            dry_run_vault_signing_addresses(context, vault_address_string);
        return print_dry_run(
            plan.command(),
            dry_run_signed_details(
                plan.would_execute(),
                plan.into_args(),
                payload,
                signer,
                acting_as,
                vault_address_string,
            ),
            format,
        );
    }
    let resolved_signer = context.resolve_signer()?;
    let signer = resolved_signer.selected_signer();
    let api_base_url = context.api_base_url();
    hyperliquid_cli::commands::orders::batch_create(
        order_execution_context(context, &api_base_url, &client, &resolver, &signer),
        args,
        orders,
        vault_address,
        format,
    )
    .await
}

async fn create_tpsl(
    context: &AppContext,
    args: &hyperliquid_cli::commands::orders::TpslArgs,
    format: output::OutputFormat,
    dry_run: bool,
    payload: Option<&serde_json::Value>,
) -> Result<(), anyhow::Error> {
    hyperliquid_cli::commands::orders::validate_tpsl_args(args)?;
    let client = context.http_client();
    let resolver = load_trading_resolver(context).await?;
    let coin_query = qualify_dex_asset(args.dex.as_deref(), &args.coin);
    let (resolver, resolved_tpsl_perp) =
        resolver_with_loaded_hip3_perp(&client, resolver, &coin_query).await?;
    let vault_address = resolve_optional_acting_account_target(args.on_behalf_of.as_deref())?;
    let vault_address_string = vault_address.map(|address| address.to_string());
    if dry_run {
        let plan = hyperliquid_cli::commands::orders::tpsl_dry_run_plan(&resolver, args)?;
        let (signer, acting_as, vault_address_string) =
            dry_run_vault_signing_addresses(context, vault_address_string);
        return print_dry_run(
            plan.command(),
            dry_run_signed_details(
                plan.would_execute(),
                plan.into_args(),
                payload,
                signer,
                acting_as,
                vault_address_string,
            ),
            format,
        );
    }
    let resolved_signer = context.resolve_signer()?;
    let margin_mode_user = vault_address.unwrap_or_else(|| resolved_signer.query_address());
    validate_order_margin_mode(
        &client,
        margin_mode_user,
        resolved_tpsl_perp.0.as_str(),
        resolved_tpsl_perp.1.as_deref(),
        args.margin_mode,
    )
    .await?;
    let signer = resolved_signer.selected_signer();
    let api_base_url = context.api_base_url();
    hyperliquid_cli::commands::orders::tpsl(
        order_execution_context(context, &api_base_url, &client, &resolver, &signer),
        args,
        vault_address,
        format,
    )
    .await
}

async fn cancel_order(
    context: &AppContext,
    args: &hyperliquid_cli::commands::orders::CancelArgs,
    format: output::OutputFormat,
    dry_run: bool,
    payload: Option<&serde_json::Value>,
) -> Result<(), anyhow::Error> {
    let vault_address = resolve_optional_acting_account_target(args.on_behalf_of.as_deref())?;
    let vault_address_string = vault_address.map(|address| address.to_string());
    if dry_run {
        let plan = hyperliquid_cli::commands::orders::cancel_dry_run_plan(args)?;
        let (signer, acting_as, vault_address_string) =
            dry_run_vault_signing_addresses(context, vault_address_string);
        return print_dry_run(
            plan.command(),
            dry_run_signed_details(
                plan.would_execute(),
                plan.into_args(),
                payload,
                signer,
                acting_as,
                vault_address_string,
            ),
            format,
        );
    }
    let client = context.http_client();
    let resolver = load_trading_resolver(context).await?;
    let resolved = context.resolve_signer()?;
    let user = vault_address.unwrap_or_else(|| resolved.query_address());
    let signer = resolved.selected_signer();
    let api_base_url = context.api_base_url();
    hyperliquid_cli::commands::orders::cancel(
        order_execution_context(context, &api_base_url, &client, &resolver, &signer),
        user,
        args,
        vault_address,
        format,
    )
    .await
}

async fn cancel_all_orders(
    context: &AppContext,
    args: &hyperliquid_cli::commands::orders::CancelAllArgs,
    format: output::OutputFormat,
    dry_run: bool,
    payload: Option<&serde_json::Value>,
) -> Result<(), anyhow::Error> {
    let vault_address = resolve_optional_acting_account_target(args.on_behalf_of.as_deref())?;
    let vault_address_string = vault_address.map(|address| address.to_string());
    if dry_run {
        let resolver = load_trading_resolver(context).await?;
        let plan = hyperliquid_cli::commands::orders::cancel_all_dry_run_plan(&resolver, args)?;
        let (signer, acting_as, vault_address_string) =
            dry_run_vault_signing_addresses(context, vault_address_string);
        return print_dry_run(
            plan.command(),
            dry_run_signed_details(
                plan.would_execute(),
                plan.into_args(),
                payload,
                signer,
                acting_as,
                vault_address_string,
            ),
            format,
        );
    }
    let client = context.http_client();
    let resolver = load_trading_resolver(context).await?;
    let resolved = context.resolve_signer()?;
    let user = vault_address.unwrap_or_else(|| resolved.query_address());
    let signer = resolved.selected_signer();
    let api_base_url = context.api_base_url();
    hyperliquid_cli::commands::orders::cancel_all(
        order_execution_context(context, &api_base_url, &client, &resolver, &signer),
        user,
        args,
        vault_address,
        format,
    )
    .await
}

async fn modify_order(
    context: &AppContext,
    args: &hyperliquid_cli::commands::orders::ModifyArgs,
    format: output::OutputFormat,
    dry_run: bool,
    payload: Option<&serde_json::Value>,
) -> Result<(), anyhow::Error> {
    hyperliquid_cli::commands::orders::validate_modify_args(args)?;
    let vault_address = resolve_optional_acting_account_target(args.on_behalf_of.as_deref())?;
    let vault_address_string = vault_address.map(|address| address.to_string());
    if dry_run {
        let plan = hyperliquid_cli::commands::orders::modify_dry_run_plan(args)?;
        let (signer, acting_as, vault_address_string) =
            dry_run_vault_signing_addresses(context, vault_address_string);
        return print_dry_run(
            plan.command(),
            dry_run_signed_details(
                plan.would_execute(),
                plan.into_args(),
                payload,
                signer,
                acting_as,
                vault_address_string,
            ),
            format,
        );
    }
    let client = context.http_client();
    let resolver = load_trading_resolver(context).await?;
    let resolved = context.resolve_signer()?;
    let user = vault_address.unwrap_or_else(|| resolved.query_address());
    let signer = resolved.selected_signer();
    let api_base_url = context.api_base_url();
    hyperliquid_cli::commands::orders::modify(
        order_execution_context(context, &api_base_url, &client, &resolver, &signer),
        user,
        args,
        vault_address,
        format,
    )
    .await
}

async fn open_orders(
    context: &AppContext,
    format: output::OutputFormat,
) -> Result<(), anyhow::Error> {
    let client = context.http_client();
    let resolver = load_asset_resolver(context).await?;
    let user = context.resolve_signer()?.query_address();
    hyperliquid_cli::commands::orders::open(&client, &resolver, user, format).await
}

async fn order_history(
    context: &AppContext,
    format: output::OutputFormat,
) -> Result<(), anyhow::Error> {
    let client = context.http_client();
    let resolver = load_asset_resolver(context).await?;
    let user = context.resolve_signer()?.query_address();
    hyperliquid_cli::commands::orders::history(&client, &resolver, user, format).await
}

async fn order_status(
    context: &AppContext,
    args: &hyperliquid_cli::commands::orders::StatusArgs,
    format: output::OutputFormat,
) -> Result<(), anyhow::Error> {
    let user = resolve_protocol_user_address(&args.user)?;
    hyperliquid_cli::commands::orders::status(&context.api_base_url(), user, args, format).await
}

async fn create_twap(
    context: &AppContext,
    args: &hyperliquid_cli::commands::orders::TwapCreateArgs,
    format: output::OutputFormat,
    dry_run: bool,
    payload: Option<&serde_json::Value>,
) -> Result<(), anyhow::Error> {
    let client = context.http_client();
    let resolver = load_trading_resolver(context).await?;
    let coin_query = qualify_dex_asset(args.dex.as_deref(), &args.coin);
    let (resolver, resolved_twap_perp) =
        resolver_with_loaded_hip3_perp(&client, resolver, &coin_query).await?;
    let dry_run_plan =
        hyperliquid_cli::commands::orders::twap_create_dry_run_plan(&resolver, args)?;
    let vault_address = resolve_optional_acting_account_target(args.on_behalf_of.as_deref())?;
    let vault_address_string = vault_address.map(|address| address.to_string());
    if dry_run {
        let (signer, acting_as, vault_address_string) =
            dry_run_vault_signing_addresses(context, vault_address_string);
        return print_dry_run(
            dry_run_plan.command(),
            dry_run_signed_details(
                dry_run_plan.would_execute(),
                dry_run_plan.into_args(),
                payload,
                signer,
                acting_as,
                vault_address_string,
            ),
            format,
        );
    }
    let resolved = context.resolve_signer()?;
    let margin_mode_user = vault_address.unwrap_or_else(|| resolved.query_address());
    validate_order_margin_mode(
        &client,
        margin_mode_user,
        resolved_twap_perp.0.as_str(),
        resolved_twap_perp.1.as_deref(),
        args.margin_mode,
    )
    .await?;
    let signer = resolved.selected_signer();
    let api_base_url = context.api_base_url();
    hyperliquid_cli::commands::orders::twap_create(
        order_submission_context(context, &api_base_url, &signer),
        &resolver,
        args,
        vault_address,
        format,
    )
    .await
}

async fn cancel_twap(
    context: &AppContext,
    args: &hyperliquid_cli::commands::orders::TwapCancelArgs,
    format: output::OutputFormat,
    dry_run: bool,
    payload: Option<&serde_json::Value>,
) -> Result<(), anyhow::Error> {
    let client = context.http_client();
    let resolver = load_trading_resolver(context).await?;
    let coin_query = qualify_dex_asset(args.dex.as_deref(), &args.coin);
    let (resolver, _) = resolver_with_loaded_hip3_perp(&client, resolver, &coin_query).await?;
    let dry_run_plan =
        hyperliquid_cli::commands::orders::twap_cancel_dry_run_plan(&resolver, args)?;
    let vault_address = resolve_optional_acting_account_target(args.on_behalf_of.as_deref())?;
    let vault_address_string = vault_address.map(|address| address.to_string());
    if dry_run {
        let (signer, acting_as, vault_address_string) =
            dry_run_vault_signing_addresses(context, vault_address_string);
        return print_dry_run(
            dry_run_plan.command(),
            dry_run_signed_details(
                dry_run_plan.would_execute(),
                dry_run_plan.into_args(),
                payload,
                signer,
                acting_as,
                vault_address_string,
            ),
            format,
        );
    }
    let resolved = context.resolve_signer()?;
    let signer = resolved.selected_signer();
    let api_base_url = context.api_base_url();
    hyperliquid_cli::commands::orders::twap_cancel(
        order_submission_context(context, &api_base_url, &signer),
        &resolver,
        args,
        vault_address,
        format,
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
    context: &AppContext,
    args: &hyperliquid_cli::commands::orders::ScheduleCancelArgs,
    format: output::OutputFormat,
    dry_run: bool,
    payload: Option<&serde_json::Value>,
) -> Result<(), anyhow::Error> {
    let vault_address = resolve_optional_acting_account_target(args.on_behalf_of.as_deref())?;
    let vault_address_string = vault_address.map(|address| address.to_string());
    if dry_run {
        let plan = hyperliquid_cli::commands::orders::schedule_cancel_dry_run_plan(args)?;
        let (signer, acting_as, vault_address_string) =
            dry_run_vault_signing_addresses(context, vault_address_string);
        return print_dry_run(
            plan.command(),
            dry_run_signed_details(
                plan.would_execute(),
                plan.into_args(),
                payload,
                signer,
                acting_as,
                vault_address_string,
            ),
            format,
        );
    }
    let resolved = context.resolve_signer()?;
    let signer = resolved.selected_signer();
    let api_base_url = context.api_base_url();
    hyperliquid_cli::commands::orders::schedule_cancel(
        order_submission_context(context, &api_base_url, &signer),
        args,
        vault_address,
        format,
    )
    .await
}

async fn list_positions(
    context: &AppContext,
    format: output::OutputFormat,
) -> Result<(), anyhow::Error> {
    let client = context.http_client();
    let user = context.resolve_signer()?.query_address();
    hyperliquid_cli::commands::positions::list(&client, user, format).await
}

async fn update_position_leverage(
    context: &AppContext,
    args: &hyperliquid_cli::commands::positions::UpdateLeverageArgs,
    format: output::OutputFormat,
    dry_run: bool,
    payload: Option<&serde_json::Value>,
) -> Result<(), anyhow::Error> {
    hyperliquid_cli::commands::positions::validate_update_leverage_args(args)?;
    let resolver = load_trading_resolver(context).await?;
    let prepared = hyperliquid_cli::commands::positions::prepare_update_leverage(&resolver, args)?;
    if dry_run {
        let (signer, acting_as) = dry_run_signing_addresses(context);
        return print_dry_run(
            "positions update-leverage",
            dry_run_signed_details(
                "update_position_leverage",
                hyperliquid_cli::commands::positions::update_leverage_dry_run_value(
                    context.network.to_string(),
                    args,
                    &prepared,
                ),
                payload,
                signer,
                acting_as,
                None,
            ),
            format,
        );
    }
    let resolved = context.resolve_signer()?;
    hyperliquid_cli::commands::positions::submit_update_leverage(
        &context.api_base_url(),
        context.chain(),
        &resolved.selected_signer(),
        prepared,
        format,
    )
    .await
}

async fn update_position_margin(
    context: &AppContext,
    args: &hyperliquid_cli::commands::positions::UpdateMarginArgs,
    format: output::OutputFormat,
    dry_run: bool,
    payload: Option<&serde_json::Value>,
) -> Result<(), anyhow::Error> {
    hyperliquid_cli::commands::positions::validate_update_margin_args(args)?;
    let resolver = load_trading_resolver(context).await?;
    let prepared = hyperliquid_cli::commands::positions::prepare_update_margin(&resolver, args)?;
    if dry_run {
        let (signer, acting_as) = dry_run_signing_addresses(context);
        return print_dry_run(
            "positions update-margin",
            dry_run_signed_details(
                "update_isolated_margin",
                hyperliquid_cli::commands::positions::update_margin_dry_run_value(
                    context.network.to_string(),
                    args,
                    &prepared,
                ),
                payload,
                signer,
                acting_as,
                None,
            ),
            format,
        );
    }
    let resolved = context.resolve_signer()?;
    let signer = resolved.selected_signer();
    hyperliquid_cli::commands::positions::submit_update_margin(
        &context.api_base_url(),
        context.chain(),
        &signer,
        prepared,
        format,
    )
    .await
}

async fn transfer_spot_to_perp(
    context: &AppContext,
    args: &hyperliquid_cli::commands::transfers::ClassTransferArgs,
    format: output::OutputFormat,
    dry_run: bool,
    payload: Option<&serde_json::Value>,
) -> Result<(), anyhow::Error> {
    hyperliquid_cli::commands::transfers::validate_class_transfer_args(args)?;
    if dry_run {
        let envelope = transfer_dry_run_envelope(
            "transfer spot-to-perp",
            hyperliquid_cli::commands::transfers::TransferActionKind::SpotToPerp,
            "transfer_spot_to_perp",
            serde_json::json!({"amount": args.amount.to_string()}),
            payload,
        );
        return print_dry_run_envelope(envelope, format);
    }
    let client = context.http_client();
    let resolved = context.resolve_signer()?;
    let signer = resolved.selected_signer();
    hyperliquid_cli::commands::transfers::spot_to_perp(
        &context.api_base_url(),
        context.chain(),
        &client,
        &signer,
        resolved.query_address(),
        args,
        format,
    )
    .await
}

async fn transfer_perp_to_spot(
    context: &AppContext,
    args: &hyperliquid_cli::commands::transfers::ClassTransferArgs,
    format: output::OutputFormat,
    dry_run: bool,
    payload: Option<&serde_json::Value>,
) -> Result<(), anyhow::Error> {
    hyperliquid_cli::commands::transfers::validate_class_transfer_args(args)?;
    if dry_run {
        let envelope = transfer_dry_run_envelope(
            "transfer perp-to-spot",
            hyperliquid_cli::commands::transfers::TransferActionKind::PerpToSpot,
            "transfer_perp_to_spot",
            serde_json::json!({"amount": args.amount.to_string()}),
            payload,
        );
        return print_dry_run_envelope(envelope, format);
    }
    let client = context.http_client();
    let resolved = context.resolve_signer()?;
    let signer = resolved.selected_signer();
    hyperliquid_cli::commands::transfers::perp_to_spot(
        &context.api_base_url(),
        context.chain(),
        &client,
        &signer,
        resolved.query_address(),
        args,
        format,
    )
    .await
}

async fn transfer_send(
    context: &AppContext,
    args: &hyperliquid_cli::commands::transfers::SendArgs,
    format: output::OutputFormat,
    dry_run: bool,
    payload: Option<&serde_json::Value>,
) -> Result<(), anyhow::Error> {
    hyperliquid_cli::commands::transfers::validate_send_args(args)?;
    if dry_run {
        let envelope = transfer_dry_run_envelope(
            "transfer send",
            hyperliquid_cli::commands::transfers::TransferActionKind::Send,
            "send_usdc",
            serde_json::json!({"to": args.to, "amount": args.amount.to_string()}),
            payload,
        );
        return print_dry_run_envelope(envelope, format);
    }
    let client = context.http_client();
    let resolved = context.resolve_signer()?;
    hyperliquid_cli::commands::transfers::send(
        &context.api_base_url(),
        context.chain(),
        &client,
        &resolved.selected_signer(),
        resolved.query_address(),
        args,
        format,
    )
    .await
}

async fn transfer_spot_send(
    context: &AppContext,
    args: &hyperliquid_cli::commands::transfers::SpotSendArgs,
    format: output::OutputFormat,
    dry_run: bool,
    payload: Option<&serde_json::Value>,
) -> Result<(), anyhow::Error> {
    hyperliquid_cli::commands::transfers::validate_spot_send_args(args)?;
    let client = context.http_client();
    let args_json =
        hyperliquid_cli::commands::transfers::spot_send_dry_run_args(&client, args).await?;
    if dry_run {
        let signer = dry_run_signer_address(context);
        let envelope = transfer_dry_run_envelope(
            "transfer spot-send",
            hyperliquid_cli::commands::transfers::TransferActionKind::SpotSend,
            "spot_send",
            args_json,
            payload,
        )
        .with_signing_context(DryRunSigningContext::new(
            signer,
            Some(args.to.clone()),
            None,
        ));
        return print_dry_run_envelope(envelope, format);
    }
    let resolved = context.resolve_signer()?;
    hyperliquid_cli::commands::transfers::spot_send(
        &context.api_base_url(),
        context.chain(),
        &client,
        &resolved.selected_signer(),
        args,
        format,
    )
    .await
}

async fn transfer_send_asset(
    context: &AppContext,
    args: &hyperliquid_cli::commands::transfers::SendAssetArgs,
    format: output::OutputFormat,
    dry_run: bool,
    payload: Option<&serde_json::Value>,
) -> Result<(), anyhow::Error> {
    hyperliquid_cli::commands::transfers::validate_send_asset_args(args)?;
    let client = context.http_client();
    let args_json =
        hyperliquid_cli::commands::transfers::send_asset_dry_run_args(&client, args).await?;
    if dry_run {
        let signer = dry_run_signer_address(context);
        let envelope = transfer_dry_run_envelope(
            "transfer send-asset",
            hyperliquid_cli::commands::transfers::TransferActionKind::SendAsset,
            "send_asset",
            args_json,
            payload,
        )
        .with_signing_context(DryRunSigningContext::new(
            signer,
            Some(args.to.clone()),
            None,
        ));
        return print_dry_run_envelope(envelope, format);
    }
    let resolved = context.resolve_signer()?;
    hyperliquid_cli::commands::transfers::send_asset(
        &context.api_base_url(),
        context.chain(),
        &client,
        &resolved.selected_signer(),
        args,
        format,
    )
    .await
}

async fn transfer_withdraw(
    context: &AppContext,
    args: &hyperliquid_cli::commands::transfers::WithdrawArgs,
    format: output::OutputFormat,
    dry_run: bool,
    payload: Option<&serde_json::Value>,
) -> Result<(), anyhow::Error> {
    hyperliquid_cli::commands::transfers::validate_withdraw_args(args)?;
    if dry_run {
        let envelope = transfer_dry_run_envelope(
            "transfer withdraw",
            hyperliquid_cli::commands::transfers::TransferActionKind::Withdraw,
            "withdraw_usdc",
            serde_json::json!({"to": args.to, "amount": args.amount.to_string()}),
            payload,
        );
        return print_dry_run_envelope(envelope, format);
    }
    let client = context.http_client();
    let resolved = context.resolve_signer()?;
    hyperliquid_cli::commands::transfers::withdraw(
        &context.api_base_url(),
        context.chain(),
        &client,
        &resolved.selected_signer(),
        resolved.query_address(),
        args,
        format,
    )
    .await
}

async fn staking_summary(
    context: &AppContext,
    address: Option<&str>,
    format: output::OutputFormat,
) -> Result<(), anyhow::Error> {
    let address = resolve_protocol_user_string_or_selected(context, address)?;
    hyperliquid_cli::commands::staking::summary(&context.api_base_url(), &address, format).await
}

async fn staking_validators(
    context: &AppContext,
    format: output::OutputFormat,
) -> Result<(), anyhow::Error> {
    hyperliquid_cli::commands::staking::validators(&context.api_base_url(), format).await
}

async fn staking_rewards(
    context: &AppContext,
    address: Option<&str>,
    format: output::OutputFormat,
) -> Result<(), anyhow::Error> {
    let address = resolve_protocol_user_string_or_selected(context, address)?;
    hyperliquid_cli::commands::staking::rewards(&context.api_base_url(), &address, format).await
}

async fn staking_history(
    context: &AppContext,
    address: Option<&str>,
    format: output::OutputFormat,
) -> Result<(), anyhow::Error> {
    let address = resolve_protocol_user_string_or_selected(context, address)?;
    hyperliquid_cli::commands::staking::history(&context.api_base_url(), &address, format).await
}

async fn staking_delegate(
    context: &AppContext,
    args: &hyperliquid_cli::commands::staking::DelegateArgs,
    format: output::OutputFormat,
    dry_run: bool,
    payload: Option<&serde_json::Value>,
) -> Result<(), anyhow::Error> {
    hyperliquid_cli::commands::staking::validate_delegate_args(args)?;
    if dry_run {
        let action_kind = hyperliquid_cli::commands::staking::StakingActionKind::Delegate;
        let args_json = hyperliquid_cli::commands::staking::delegate_dry_run_value(
            context.chain(),
            args,
            action_kind,
        )?;
        let envelope = signed_action_dry_run_envelope(
            "staking delegate",
            action_kind.would_execute(),
            action_kind.reversibility(),
            args_json,
            payload,
            default_dry_run_signing_context(context),
        );
        return print_dry_run_envelope(envelope, format);
    }
    let resolved = context.resolve_signer()?;
    let signer = resolved.selected_signer();
    hyperliquid_cli::commands::staking::delegate(
        &context.api_base_url(),
        context.chain(),
        &signer,
        args,
        format,
    )
    .await
}

async fn staking_undelegate(
    context: &AppContext,
    args: &hyperliquid_cli::commands::staking::DelegateArgs,
    format: output::OutputFormat,
    dry_run: bool,
    payload: Option<&serde_json::Value>,
) -> Result<(), anyhow::Error> {
    hyperliquid_cli::commands::staking::validate_delegate_args(args)?;
    if dry_run {
        let action_kind = hyperliquid_cli::commands::staking::StakingActionKind::Undelegate;
        let args_json = hyperliquid_cli::commands::staking::delegate_dry_run_value(
            context.chain(),
            args,
            action_kind,
        )?;
        let envelope = signed_action_dry_run_envelope(
            "staking undelegate",
            action_kind.would_execute(),
            action_kind.reversibility(),
            args_json,
            payload,
            default_dry_run_signing_context(context),
        );
        return print_dry_run_envelope(envelope, format);
    }
    let resolved = context.resolve_signer()?;
    let signer = resolved.selected_signer();
    hyperliquid_cli::commands::staking::undelegate(
        &context.api_base_url(),
        context.chain(),
        &signer,
        args,
        format,
    )
    .await
}

async fn staking_deposit(
    context: &AppContext,
    args: &hyperliquid_cli::commands::staking::AmountArgs,
    format: output::OutputFormat,
    dry_run: bool,
    payload: Option<&serde_json::Value>,
) -> Result<(), anyhow::Error> {
    hyperliquid_cli::commands::staking::validate_amount_args(args)?;
    if dry_run {
        let action_kind = hyperliquid_cli::commands::staking::StakingActionKind::Deposit;
        let args_json = hyperliquid_cli::commands::staking::amount_dry_run_value(
            context.chain(),
            args,
            action_kind,
        )?;
        let envelope = signed_action_dry_run_envelope(
            "staking deposit",
            action_kind.would_execute(),
            action_kind.reversibility(),
            args_json,
            payload,
            default_dry_run_signing_context(context),
        );
        return print_dry_run_envelope(envelope, format);
    }
    let resolved = context.resolve_signer()?;
    let signer = resolved.selected_signer();
    hyperliquid_cli::commands::staking::deposit(
        &context.api_base_url(),
        context.chain(),
        &signer,
        args,
        format,
    )
    .await
}

async fn staking_withdraw(
    context: &AppContext,
    args: &hyperliquid_cli::commands::staking::AmountArgs,
    format: output::OutputFormat,
    dry_run: bool,
    payload: Option<&serde_json::Value>,
) -> Result<(), anyhow::Error> {
    hyperliquid_cli::commands::staking::validate_amount_args(args)?;
    if dry_run {
        let action_kind = hyperliquid_cli::commands::staking::StakingActionKind::Withdraw;
        let args_json = hyperliquid_cli::commands::staking::amount_dry_run_value(
            context.chain(),
            args,
            action_kind,
        )?;
        let envelope = signed_action_dry_run_envelope(
            "staking withdraw",
            action_kind.would_execute(),
            action_kind.reversibility(),
            args_json,
            payload,
            default_dry_run_signing_context(context),
        );
        return print_dry_run_envelope(envelope, format);
    }
    let resolved = context.resolve_signer()?;
    let signer = resolved.selected_signer();
    hyperliquid_cli::commands::staking::withdraw(
        &context.api_base_url(),
        context.chain(),
        &signer,
        args,
        format,
    )
    .await
}

async fn staking_claim_rewards(
    context: &AppContext,
    format: output::OutputFormat,
    dry_run: bool,
    payload: Option<&serde_json::Value>,
) -> Result<(), anyhow::Error> {
    if dry_run {
        let action_kind = hyperliquid_cli::commands::staking::StakingActionKind::ClaimRewards;
        let envelope = signed_action_dry_run_envelope(
            "staking claim-rewards",
            action_kind.would_execute(),
            action_kind.reversibility(),
            hyperliquid_cli::commands::staking::claim_rewards_dry_run_value(context.chain()),
            payload,
            default_dry_run_signing_context(context),
        );
        return print_dry_run_envelope(envelope, format);
    }
    let resolved = context.resolve_signer()?;
    let signer = resolved.selected_signer();
    hyperliquid_cli::commands::staking::claim_rewards(
        &context.api_base_url(),
        context.chain(),
        &signer,
        format,
    )
    .await
}

async fn staking_link(
    context: &AppContext,
    args: &hyperliquid_cli::commands::staking::LinkArgs,
    is_finalize: bool,
    format: output::OutputFormat,
    dry_run: bool,
    payload: Option<&serde_json::Value>,
) -> Result<(), anyhow::Error> {
    hyperliquid_cli::commands::staking::validate_link_args(args)?;
    let phase = if is_finalize { "finalize" } else { "initiate" };
    if dry_run {
        let args_json = hyperliquid_cli::commands::staking::link_dry_run_value(
            context.chain(),
            args,
            is_finalize,
        )?;
        let action_kind = if is_finalize {
            hyperliquid_cli::commands::staking::StakingActionKind::LinkFinalize
        } else {
            hyperliquid_cli::commands::staking::StakingActionKind::LinkInitiate
        };
        let envelope = signed_action_dry_run_envelope(
            &format!("staking link {phase}"),
            action_kind.would_execute(),
            action_kind.reversibility(),
            args_json,
            payload,
            default_dry_run_signing_context(context),
        );
        return print_dry_run_envelope(envelope, format);
    }
    let resolved = context.resolve_signer()?;
    let signer = resolved.selected_signer();
    hyperliquid_cli::commands::staking::link_staking_user(
        &context.api_base_url(),
        context.chain(),
        &signer,
        args,
        is_finalize,
        format,
    )
    .await
}

async fn vault_get(
    context: &AppContext,
    address: &str,
    format: output::OutputFormat,
) -> Result<(), anyhow::Error> {
    hyperliquid_cli::commands::vaults::get(&context.api_base_url(), address, format).await
}

async fn vault_list(
    context: &AppContext,
    args: &hyperliquid_cli::commands::vaults::VaultListArgs,
    format: output::OutputFormat,
) -> Result<(), anyhow::Error> {
    hyperliquid_cli::commands::vaults::list(&context.api_base_url(), args, format).await
}

async fn vault_search(
    context: &AppContext,
    args: &hyperliquid_cli::commands::vaults::VaultSearchArgs,
    format: output::OutputFormat,
) -> Result<(), anyhow::Error> {
    hyperliquid_cli::commands::vaults::search(&context.api_base_url(), args, format).await
}

async fn vault_positions(
    context: &AppContext,
    address: &str,
    format: output::OutputFormat,
) -> Result<(), anyhow::Error> {
    let client = context.http_client();
    hyperliquid_cli::commands::vaults::positions(&client, address, format).await
}

async fn vault_deposit(
    context: &AppContext,
    args: &hyperliquid_cli::commands::vaults::VaultTransferArgs,
    format: output::OutputFormat,
    dry_run: bool,
    payload: Option<&serde_json::Value>,
) -> Result<(), anyhow::Error> {
    hyperliquid_cli::commands::vaults::validate_transfer_args(args)?;
    if dry_run {
        let args_json = hyperliquid_cli::commands::vaults::transfer_dry_run_value(
            context.chain(),
            args,
            hyperliquid_cli::commands::vaults::VaultTransferActionKind::Deposit,
        )?;
        let envelope = signed_action_dry_run_envelope(
            "vault deposit",
            "deposit_usdc_to_vault",
            hyperliquid_cli::commands::vaults::VaultTransferActionKind::Deposit.reversibility(),
            args_json,
            payload,
            default_dry_run_signing_context(context),
        );
        return print_dry_run_envelope(envelope, format);
    }
    let resolved = context.resolve_signer()?;
    hyperliquid_cli::commands::vaults::deposit(
        &context.api_base_url(),
        context.chain(),
        &resolved.selected_signer(),
        args,
        format,
    )
    .await
}

async fn vault_withdraw(
    context: &AppContext,
    args: &hyperliquid_cli::commands::vaults::VaultTransferArgs,
    format: output::OutputFormat,
    dry_run: bool,
    payload: Option<&serde_json::Value>,
) -> Result<(), anyhow::Error> {
    hyperliquid_cli::commands::vaults::validate_transfer_args(args)?;
    if dry_run {
        let args_json = hyperliquid_cli::commands::vaults::transfer_dry_run_value(
            context.chain(),
            args,
            hyperliquid_cli::commands::vaults::VaultTransferActionKind::Withdraw,
        )?;
        let envelope = signed_action_dry_run_envelope(
            "vault withdraw",
            "withdraw_usdc_from_vault",
            hyperliquid_cli::commands::vaults::VaultTransferActionKind::Withdraw.reversibility(),
            args_json,
            payload,
            default_dry_run_signing_context(context),
        );
        return print_dry_run_envelope(envelope, format);
    }
    let resolved = context.resolve_signer()?;
    hyperliquid_cli::commands::vaults::withdraw(
        &context.api_base_url(),
        context.chain(),
        &resolved.selected_signer(),
        args,
        format,
    )
    .await
}

async fn borrowlend_rates(
    context: &AppContext,
    format: output::OutputFormat,
) -> Result<(), anyhow::Error> {
    let client = context.http_client();
    hyperliquid_cli::commands::borrowlend::rates(&client, &context.api_base_url(), format).await
}

async fn borrowlend_get(
    context: &AppContext,
    token: &str,
    format: output::OutputFormat,
) -> Result<(), anyhow::Error> {
    let client = context.http_client();
    hyperliquid_cli::commands::borrowlend::get(&client, &context.api_base_url(), token, format)
        .await
}

async fn borrowlend_user(
    context: &AppContext,
    address: Option<&str>,
    format: output::OutputFormat,
) -> Result<(), anyhow::Error> {
    let client = context.http_client();
    let address = resolve_protocol_user_string_or_selected(context, address)?;
    hyperliquid_cli::commands::borrowlend::user(&client, &address, format).await
}

async fn borrowlend_action(
    context: &AppContext,
    operation: hyperliquid_cli::commands::borrowlend::BorrowLendOperation,
    args: &hyperliquid_cli::commands::borrowlend::ActionArgs,
    format: output::OutputFormat,
    dry_run: bool,
    payload: Option<&serde_json::Value>,
) -> Result<(), anyhow::Error> {
    let client = context.http_client();
    let preview = hyperliquid_cli::commands::borrowlend::action_preview(
        &client,
        context.chain(),
        operation,
        args,
    )
    .await?;
    if dry_run {
        let envelope = signed_action_dry_run_envelope(
            &format!("borrowlend {}", operation.as_str()),
            &format!("{}_borrowlend", operation.as_str()),
            operation.reversibility(),
            preview,
            payload,
            default_dry_run_signing_context(context),
        );
        return print_dry_run_envelope(envelope, format);
    }

    let resolved = context.resolve_signer()?;
    let signer = resolved.selected_signer();
    hyperliquid_cli::commands::borrowlend::action(
        &client,
        &context.api_base_url(),
        context.chain(),
        &signer,
        operation,
        args,
        format,
    )
    .await
}

async fn prio_status(
    context: &AppContext,
    format: output::OutputFormat,
) -> Result<(), anyhow::Error> {
    let client = context.http_client();
    hyperliquid_cli::commands::prio::status(&client, format).await
}

async fn prio_bid(
    context: &AppContext,
    args: &hyperliquid_cli::commands::prio::BidArgs,
    format: output::OutputFormat,
    dry_run: bool,
    payload: Option<&serde_json::Value>,
) -> Result<(), anyhow::Error> {
    if dry_run {
        return print_dry_run(
            "prio bid",
            dry_run_details(
                "place_priority_auction_bid",
                serde_json::json!({"ip": args.ip, "max": args.max.to_string(), "slot": args.slot}),
                payload,
            ),
            format,
        );
    }
    let client = context.http_client();
    let resolved = context.resolve_signer()?;
    let signer = resolved.selected_signer();
    hyperliquid_cli::commands::prio::bid(
        &context.api_base_url(),
        &client,
        &signer,
        context.chain(),
        args,
        format,
    )
    .await
}

async fn set_referral(
    context: &AppContext,
    code: Option<&str>,
    format: output::OutputFormat,
    dry_run: bool,
    payload: Option<&serde_json::Value>,
) -> Result<(), anyhow::Error> {
    let owned_default: Option<String>;
    let code: &str = match code {
        Some(code) => code,
        None => {
            owned_default = hyperliquid_cli::commands::referral::resolve_default_referral_code(
                context.network == config::Network::Testnet,
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
    if dry_run {
        let signer_context = context.resolve_signer().ok();
        let envelope = signed_action_dry_run_envelope(
            "referral set",
            "set_referral_code",
            hyperliquid_cli::commands::referral::ReferralActionKind::Set.reversibility(),
            hyperliquid_cli::commands::referral::referral_dry_run_value(
                context.chain(),
                signer_context.as_ref().map(|resolved| resolved.address()),
                signer_context
                    .as_ref()
                    .map(|resolved| resolved.query_address()),
                code,
                hyperliquid_cli::commands::referral::ReferralActionKind::Set,
            ),
            payload,
            default_dry_run_signing_context(context),
        );
        return print_dry_run_envelope(envelope, format);
    }
    let resolved = context.resolve_signer()?;
    let signer = resolved.selected_signer();
    hyperliquid_cli::commands::referral::set(
        &context.api_base_url(),
        context.chain(),
        &signer,
        code,
        format,
    )
    .await
}

async fn register_referrer(
    context: &AppContext,
    code: &str,
    format: output::OutputFormat,
    dry_run: bool,
    payload: Option<&serde_json::Value>,
) -> Result<(), anyhow::Error> {
    hyperliquid_cli::commands::referral::validate_referrer_code(code)?;
    if dry_run {
        let signer_context = context.resolve_signer().ok();
        let envelope = signed_action_dry_run_envelope(
            "referral register",
            "register_referrer_code",
            hyperliquid_cli::commands::referral::ReferralActionKind::Register.reversibility(),
            hyperliquid_cli::commands::referral::referral_dry_run_value(
                context.chain(),
                signer_context.as_ref().map(|resolved| resolved.address()),
                signer_context
                    .as_ref()
                    .map(|resolved| resolved.query_address()),
                code,
                hyperliquid_cli::commands::referral::ReferralActionKind::Register,
            ),
            payload,
            default_dry_run_signing_context(context),
        );
        return print_dry_run_envelope(envelope, format);
    }
    let resolved = context.resolve_signer()?;
    let signer = resolved.selected_signer();
    hyperliquid_cli::commands::referral::register(
        &context.api_base_url(),
        context.chain(),
        &signer,
        code,
        format,
    )
    .await
}

async fn referral_status(
    context: &AppContext,
    format: output::OutputFormat,
) -> Result<(), anyhow::Error> {
    let user = context.resolve_signer()?.query_address();
    hyperliquid_cli::commands::referral::status(&context.api_base_url(), user, format).await
}

async fn load_trading_resolver(context: &AppContext) -> Result<AssetResolver, errors::CliError> {
    match load_asset_resolver(context).await {
        Ok(resolver) => Ok(resolver),
        Err(_) => {
            hyperliquid_cli::commands::orders::load_perp_resolver_from_api_base(
                &context.api_base_url(),
            )
            .await
        }
    }
}

async fn load_asset_resolver(context: &AppContext) -> Result<AssetResolver, errors::CliError> {
    let client = context.http_client();
    let metadata = ASSET_METADATA_CACHE
        .get_or_init(MetadataCache::new)
        .get_or_load(context.chain(), &client)
        .await?;
    Ok(AssetResolver::new(metadata))
}

fn unimplemented_command_error() -> anyhow::Error {
    errors::CliError::Internal(anyhow::anyhow!(
        "Command not yet implemented. Check back soon!"
    ))
    .into()
}

async fn resolve_asset_or_load_hip3(
    client: &hypersdk::hypercore::HttpClient,
    resolver: &AssetResolver,
    query: &str,
) -> Result<hyperliquid_cli::commands::ResolvedAsset, errors::CliError> {
    match resolver.resolve(query) {
        Ok(resolved) => Ok(resolved),
        Err(err) => match hyperliquid_cli::commands::parse_asset_query(query) {
            hyperliquid_cli::commands::AssetQuery::Hip3 { dex, token } => {
                match load_hip3_perp_asset(client, query, &dex, &token).await {
                    Ok(asset) => Ok(resolved_asset_from_perp(asset)),
                    Err(errors::CliError::AssetNotFoundNoSuggestion { .. }) => Err(err),
                    Err(err) => Err(err),
                }
            }
            _ => Err(err),
        },
    }
}

async fn resolver_with_loaded_hip3_perp(
    client: &hypersdk::hypercore::HttpClient,
    resolver: AssetResolver,
    query: &str,
) -> Result<(AssetResolver, (String, Option<String>)), errors::CliError> {
    match resolver.resolve_perp(query) {
        Ok(hyperliquid_cli::commands::ResolvedAsset::Perp {
            name,
            index: _,
            dex,
            ..
        }) => Ok((resolver, (name, dex))),
        Ok(hyperliquid_cli::commands::ResolvedAsset::Spot { .. }) => {
            unreachable!("resolve_perp never returns spots")
        }
        Err(err) => match hyperliquid_cli::commands::parse_asset_query(query) {
            hyperliquid_cli::commands::AssetQuery::Hip3 { dex, token } => {
                match load_hip3_perp_asset(client, query, &dex, &token).await {
                    Ok(asset) => {
                        let resolved_name = asset.name.clone();
                        let resolved_dex = asset.dex.clone();
                        Ok((
                            resolver.with_perp_asset(asset),
                            (resolved_name, resolved_dex),
                        ))
                    }
                    Err(errors::CliError::AssetNotFoundNoSuggestion { .. }) => Err(err),
                    Err(err) => Err(err),
                }
            }
            _ => Err(err),
        },
    }
}

async fn load_hip3_perp_asset(
    client: &hypersdk::hypercore::HttpClient,
    input: &str,
    dex_name: &str,
    token: &str,
) -> Result<hyperliquid_cli::commands::PerpAsset, errors::CliError> {
    let dex = client
        .perp_dexs()
        .await
        .map_err(errors::from_anyhow)?
        .into_iter()
        .find(|dex| dex.name().eq_ignore_ascii_case(dex_name))
        .ok_or_else(|| errors::CliError::Unsupported(format!("Unknown DEX: {dex_name}")))?;
    let canonical_dex_name = dex.name().to_string();
    let prefix = format!("{canonical_dex_name}:");
    let market = client
        .perps_from(dex)
        .await
        .map_err(errors::from_anyhow)?
        .into_iter()
        .find(|market| {
            let display_token = market.name.strip_prefix(&prefix).unwrap_or(&market.name);
            display_token.eq_ignore_ascii_case(token) || market.name.eq_ignore_ascii_case(token)
        })
        .ok_or_else(|| errors::CliError::AssetNotFoundNoSuggestion {
            asset: input.to_string(),
        })?;
    let name = market
        .name
        .strip_prefix(&prefix)
        .unwrap_or(market.name.as_str())
        .to_string();

    Ok(hyperliquid_cli::commands::PerpAsset {
        name,
        index: market.index,
        dex: Some(canonical_dex_name),
        sz_decimals: u32::try_from(market.sz_decimals).unwrap_or_default(),
        collateral: market.collateral.name,
    })
}

fn resolved_asset_from_perp(
    asset: hyperliquid_cli::commands::PerpAsset,
) -> hyperliquid_cli::commands::ResolvedAsset {
    hyperliquid_cli::commands::ResolvedAsset::Perp {
        name: asset.name,
        index: asset.index,
        dex: asset.dex,
        sz_decimals: asset.sz_decimals,
        collateral: asset.collateral,
    }
}

fn qualify_dex_asset(dex: Option<&str>, coin: &str) -> String {
    match dex {
        Some(dex) if !coin.contains(':') => format!("{dex}:{coin}"),
        _ => coin.to_string(),
    }
}
