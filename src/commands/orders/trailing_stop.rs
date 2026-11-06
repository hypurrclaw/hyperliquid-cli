//! `orders trailing-stop`: native perpetual trailing stops.
//!
//! The exchange tracks the mark price from the moment the stop activates and
//! fires a market order once the mark retraces from its best level by the
//! configured percent or price distance. The trigger never moves back.

use hypersdk::hypercore::types::{TrailingStop, TrailingStopRetracement};

use super::*;

/// Arguments for `orders trailing-stop`.
#[derive(Args, Debug, Clone)]
pub struct TrailingStopArgs {
    /// Perpetual coin (for example: BTC, ETH, SOL). Spot and outcome markets are not supported
    #[arg(long)]
    pub coin: String,

    /// HIP-3 DEX to trade on (equivalent to --coin dex:COIN)
    #[arg(long)]
    pub dex: Option<String>,

    /// Order side. A sell closes a long and trails the highest mark price; a buy closes a short and trails the lowest
    #[arg(long, value_enum)]
    pub side: OrderSide,

    /// Base-asset size
    #[arg(long, allow_hyphen_values = true)]
    pub size: Decimal,

    /// Retracement from the best mark price as a percent, above 0 and below 100 (for example 5 or 5%). Use this or --retracement-px
    #[arg(
        long,
        allow_hyphen_values = true,
        value_parser = parse_retracement_pct,
        conflicts_with = "retracement_px",
        required_unless_present = "retracement_px"
    )]
    pub retracement_pct: Option<Decimal>,

    /// Retracement from the best mark price as a price distance, above 0. Use this or --retracement-pct
    #[arg(
        long,
        allow_hyphen_values = true,
        conflicts_with = "retracement_pct",
        required_unless_present = "retracement_pct"
    )]
    pub retracement_px: Option<Decimal>,

    /// Mark price at which the stop starts tracking. Omit to start tracking immediately
    #[arg(long, allow_hyphen_values = true)]
    pub activation_price: Option<Decimal>,

    /// Only reduce an existing perpetual position
    #[arg(long)]
    pub reduce_only: bool,

    /// Acting-account selector for vaultAddress: subaccount/vault address, stored account alias, or stored account id
    #[arg(long)]
    pub on_behalf_of: Option<String>,

    /// Skip the mainnet confirmation prompt for deliberate automation
    #[arg(short = 'y', long)]
    pub yes: bool,
}

/// Parse `5`, `5.5`, or `5%` as a percent value.
pub(super) fn parse_retracement_pct(value: &str) -> Result<Decimal, String> {
    let trimmed = value.trim();
    let digits = trimmed.strip_suffix('%').unwrap_or(trimmed).trim_end();
    Decimal::from_str(digits)
        .map_err(|_| format!("`{value}` is not a percent. Use a number such as 5 or 5%"))
}

fn require_positive(value: Decimal, flag: &'static str) -> Result<(), CliError> {
    if value <= Decimal::ZERO {
        return Err(CliError::Configuration(format!(
            "{flag} must be greater than zero"
        )));
    }
    Ok(())
}

/// The retracement the user asked for. Exactly one option must be set.
fn retracement_from_args(args: &TrailingStopArgs) -> Result<TrailingStopRetracement, CliError> {
    match (args.retracement_pct, args.retracement_px) {
        (Some(pct), None) => {
            if pct <= Decimal::ZERO || pct >= Decimal::ONE_HUNDRED {
                return Err(CliError::Configuration(
                    "--retracement-pct must be greater than 0 and less than 100".to_string(),
                ));
            }
            Ok(TrailingStopRetracement::Pct(pct))
        }
        (None, Some(px)) => {
            require_positive(px, "--retracement-px")?;
            Ok(TrailingStopRetracement::Px(px))
        }
        _ => Err(CliError::Configuration(
            "orders trailing-stop requires exactly one of --retracement-pct or --retracement-px.\n  \
             hyperliquid --format json --dry-run orders trailing-stop --coin BTC --side sell --size 0.01 --retracement-pct 5 --reduce-only"
                .to_string(),
        )),
    }
}

/// Validate `orders trailing-stop` arguments before auth or network access.
pub fn validate_trailing_stop_args(args: &TrailingStopArgs) -> Result<(), CliError> {
    match parse_asset_query(&qualify_dex_asset(args.dex.as_deref(), &args.coin)) {
        AssetQuery::Spot(_) => {
            return Err(CliError::Configuration(format!(
                "orders trailing-stop supports perpetual markets only; {} is a spot market",
                args.coin
            )));
        }
        AssetQuery::Outcome(_) => {
            return Err(CliError::Configuration(format!(
                "orders trailing-stop supports perpetual markets only; {} is an outcome market",
                args.coin
            )));
        }
        AssetQuery::Perp(_) | AssetQuery::Hip3 { .. } => {}
    }
    require_positive(args.size, "--size")?;
    retracement_from_args(args)?;
    if let Some(activation) = args.activation_price {
        require_positive(activation, "--activation-price")?;
    }
    Ok(())
}

pub(crate) struct TrailingStopPlan {
    action: TrailingStop,
    coin: String,
    side: OrderSide,
    size: Decimal,
    retracement: TrailingStopRetracement,
    activation_px: Option<Decimal>,
}

impl TrailingStopPlan {
    fn reduce_only(&self) -> bool {
        self.action.reduce_only
    }

    fn dry_run_args(&self, args: &TrailingStopArgs) -> serde_json::Value {
        serde_json::json!({
            "coin": args.coin,
            "dex": args.dex,
            "resolved_asset": self.coin,
            "asset_id": self.action.asset,
            "side": self.side.to_string(),
            "size": self.size.to_string(),
            "reduce_only": self.reduce_only(),
            "retracement": retracement_json(&self.retracement),
            "activation_px": self.activation_px.map(|price| price.to_string()),
            "on_behalf_of": args.on_behalf_of,
        })
    }
}

fn retracement_json(retracement: &TrailingStopRetracement) -> serde_json::Value {
    match retracement {
        TrailingStopRetracement::Pct(pct) => {
            serde_json::json!({ "pct": pct.normalize().to_string() })
        }
        TrailingStopRetracement::Px(px) => serde_json::json!({ "px": px.normalize().to_string() }),
    }
}

fn retracement_label(retracement: &TrailingStopRetracement) -> String {
    match retracement {
        TrailingStopRetracement::Pct(pct) => format!("{}%", pct.normalize()),
        TrailingStopRetracement::Px(px) => px.normalize().to_string(),
    }
}

pub(crate) fn prepare_trailing_stop_plan(
    resolver: &AssetResolver,
    args: &TrailingStopArgs,
) -> Result<TrailingStopPlan, CliError> {
    validate_trailing_stop_args(args)?;
    let retracement = retracement_from_args(args)?;
    let query = qualify_dex_asset(args.dex.as_deref(), &args.coin);
    let ResolvedAsset::Perp {
        name,
        index,
        dex,
        sz_decimals,
        ..
    } = resolver.resolve_perp(&query)?
    else {
        return Err(CliError::Configuration(format!(
            "orders trailing-stop supports perpetual markets only; {} is not a perpetual market",
            args.coin
        )));
    };
    let coin = dex
        .as_ref()
        .map(|dex| format!("{dex}:{name}"))
        .unwrap_or(name);
    let size = args.size.round_dp(sz_decimals);
    if size <= Decimal::ZERO {
        return Err(CliError::Configuration(format!(
            "size is too small for {coin}; rounds to zero"
        )));
    }
    Ok(TrailingStopPlan {
        action: TrailingStop {
            asset: index,
            is_buy: args.side.is_buy(),
            sz: size,
            reduce_only: args.reduce_only,
            retracement,
            activation_px: args.activation_price,
        },
        coin,
        side: args.side,
        size,
        retracement,
        activation_px: args.activation_price,
    })
}

/// Build the `--dry-run` plan for `orders trailing-stop`.
pub fn trailing_stop_dry_run_plan(
    resolver: &AssetResolver,
    args: &TrailingStopArgs,
) -> Result<OrderDryRunPlan, CliError> {
    let plan = prepare_trailing_stop_plan(resolver, args)?;
    Ok(OrderDryRunPlan::new(
        "orders trailing-stop",
        "trailing_stop",
        plan.dry_run_args(args),
    ))
}

/// Place a native trailing stop through the signed L1 exchange action.
pub async fn trailing_stop(
    context: OrderSubmissionContext<'_>,
    resolver: &AssetResolver,
    args: &TrailingStopArgs,
    vault_address: Option<Address>,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let start = Instant::now();
    context.signer.ensure_can_attempt_live_signing()?;
    let plan = prepare_trailing_stop_plan(resolver, args)?;
    if context.require_mainnet_confirmation
        && !args.yes
        && !confirm_mainnet_trailing_stop(&plan, &ConsentTarget::new(context, vault_address, None))?
    {
        return Err(CliError::Configuration(
            "Mainnet trailing stop confirmation required; trailing stop placement cancelled. Rerun with --yes for deliberate automation."
                .to_string(),
        )
        .into());
    }
    let response = actions::send_l1_action_raw(
        context.api_base_url,
        context.chain,
        context.signer,
        Action::TrailingStop(plan.action.clone()),
        actions::nonce_now(),
        vault_address,
        "trailing stop rejected",
    )
    .await?;
    let oid = parse_trailing_stop_response(&response)?;

    output::print_data(
        &TrailingStopOutput {
            rows: vec![TrailingStopConfirmation {
                oid,
                coin: plan.coin.clone(),
                side: plan.side.to_string(),
                size: plan.size,
                reduce_only: plan.reduce_only(),
                retracement: retracement_json(&plan.retracement),
                activation_px: plan.activation_px.map(|price| price.to_string()),
                status: "waiting_for_trigger".to_string(),
                retracement_label: retracement_label(&plan.retracement),
            }],
        },
        format,
        start.elapsed(),
    );
    if format == OutputFormat::Pretty {
        print_cancel_hint(oid, vault_address, args.on_behalf_of.as_deref());
    }
    Ok(())
}

/// Tell a human how to remove the stop they just placed.
fn print_cancel_hint(oid: u64, vault_address: Option<Address>, on_behalf_of: Option<&str>) {
    let acting = match (vault_address, on_behalf_of) {
        (Some(_), Some(selector)) => format!(" --on-behalf-of {selector}"),
        _ => String::new(),
    };
    let mut stderr = io::stderr();
    // A closed stderr must not turn a placed order into a failure.
    let _ = writeln!(
        stderr,
        "\nCancel it:\n    hyperliquid orders cancel {oid}{acting}"
    );
}

/// Extract the order id from an `ok` trailing stop response.
fn parse_trailing_stop_response(response: &serde_json::Value) -> Result<u64, CliError> {
    if response.get("type").and_then(serde_json::Value::as_str) == Some("trailingStop")
        && let Some(oid) = response
            .pointer("/data/oid")
            .and_then(serde_json::Value::as_u64)
    {
        return Ok(oid);
    }
    if let Some(error) = response
        .pointer("/data/error")
        .or_else(|| response.pointer("/data/status/error"))
        .and_then(serde_json::Value::as_str)
    {
        return Err(rendering::status_error("place trailing stop", error));
    }
    Err(CliError::unknown_outcome(format!(
        "The exchange answered ok without an order ID ({}). Check `hyperliquid orders open` before placing another trailing stop",
        labelled_untrusted_text(&response.to_string())
    )))
}

fn confirm_mainnet_trailing_stop(
    plan: &TrailingStopPlan,
    target: &ConsentTarget<'_>,
) -> Result<bool, CliError> {
    let tracking = if plan.side.is_buy() {
        "lowest"
    } else {
        "highest"
    };
    let activation = plan
        .activation_px
        .map(|price| format!("Activation: {} (mark price)", price.normalize()))
        .unwrap_or_else(|| "Activation: starts tracking immediately".to_string());
    let details = [
        format!("Trailing stop: {} market order when it fires", plan.side),
        format!("Coin: {}", plan.coin),
        format!("Side: {}", plan.side),
        format!("Size: {}", plan.size.normalize()),
        format!(
            "Retracement: {} from the {tracking} mark price",
            retracement_label(&plan.retracement)
        ),
        activation,
        format!(
            "Reduce-only: {}",
            if plan.reduce_only() { "yes" } else { "no" }
        ),
    ];
    ask_confirmation(
        target,
        &details,
        &format!("Place this trailing stop on {}?", target.network()),
    )
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct TrailingStopConfirmation {
    oid: u64,
    coin: String,
    side: String,
    #[serde(with = "rust_decimal::serde::str")]
    size: Decimal,
    reduce_only: bool,
    retracement: serde_json::Value,
    activation_px: Option<String>,
    status: String,
    #[serde(skip)]
    retracement_label: String,
}

pub(super) struct TrailingStopOutput {
    rows: Vec<TrailingStopConfirmation>,
}

impl TableData for TrailingStopOutput {
    fn headers(&self) -> Vec<&str> {
        vec![
            "Coin",
            "Side",
            "Size",
            "Retracement",
            "Activation",
            "Reduce Only",
            "Status",
            "OID",
        ]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        self.rows
            .iter()
            .map(|row| {
                vec![
                    row.coin.clone(),
                    row.side.clone(),
                    row.size.normalize().to_string(),
                    row.retracement_label.clone(),
                    row.activation_px
                        .clone()
                        .unwrap_or_else(|| "immediate".to_string()),
                    row.reduce_only.to_string(),
                    row.status.clone(),
                    row.oid.to_string(),
                ]
            })
            .collect()
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(&self.rows).unwrap_or_else(|_| serde_json::json!([]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::{AssetMetadata, PerpAsset};
    use clap::Parser;

    #[derive(Parser)]
    struct Harness {
        #[command(flatten)]
        args: TrailingStopArgs,
    }

    fn parse(extra: &[&str]) -> Result<TrailingStopArgs, clap::Error> {
        let mut argv = vec![
            "harness", "--coin", "BTC", "--side", "sell", "--size", "0.01",
        ];
        argv.extend_from_slice(extra);
        Harness::try_parse_from(argv).map(|harness| harness.args)
    }

    fn resolver() -> AssetResolver {
        AssetResolver::new(AssetMetadata::from_assets(
            vec![
                PerpAsset {
                    name: "BTC".to_string(),
                    index: 0,
                    dex: None,
                    sz_decimals: 5,
                    collateral: "USDC".to_string(),
                },
                PerpAsset::hip3("testdex", "HYPE", 11_100),
            ],
            vec![SpotAsset {
                symbol: "HYPE/USDC".to_string(),
                index: 11_035,
                base: "HYPE".to_string(),
                quote: "USDC".to_string(),
                base_sz_decimals: 2,
            }],
        ))
    }

    #[test]
    fn percent_parser_accepts_plain_and_suffixed_values() {
        assert_eq!(parse_retracement_pct("5"), Ok(Decimal::from(5)));
        assert_eq!(parse_retracement_pct("5%"), Ok(Decimal::from(5)));
        assert_eq!(
            parse_retracement_pct(" 1.234 % "),
            Ok(Decimal::new(1234, 3))
        );
        assert!(parse_retracement_pct("five").is_err());
        assert!(parse_retracement_pct("%").is_err());
    }

    #[test]
    fn exactly_one_retracement_option_is_required_by_the_parser() {
        assert!(parse(&[]).is_err());
        assert!(parse(&["--retracement-pct", "5", "--retracement-px", "100"]).is_err());
        assert!(parse(&["--retracement-pct", "5"]).is_ok());
        assert!(parse(&["--retracement-px", "100"]).is_ok());
    }

    #[test]
    fn percent_must_be_between_zero_and_one_hundred_exclusive() {
        for bad in ["0", "-1", "100", "250"] {
            let args = parse(&["--retracement-pct", bad]).unwrap();
            let err = validate_trailing_stop_args(&args).unwrap_err();
            assert!(matches!(err, CliError::Configuration(_)), "{bad}: {err}");
            assert_eq!(err.exit_code(), 2);
        }
        for good in ["0.1", "5", "99.99"] {
            let args = parse(&["--retracement-pct", good]).unwrap();
            validate_trailing_stop_args(&args).unwrap();
        }
    }

    #[test]
    fn price_distances_and_activation_must_be_positive() {
        for extra in [["--retracement-px", "0"], ["--retracement-px", "-5"]] {
            let args = parse(&extra).unwrap();
            assert!(validate_trailing_stop_args(&args).is_err(), "{extra:?}");
        }
        let args = parse(&["--retracement-px", "5", "--activation-price", "0"]).unwrap();
        assert!(validate_trailing_stop_args(&args).is_err());
        let mut args = parse(&["--retracement-px", "5"]).unwrap();
        args.size = Decimal::ZERO;
        assert!(validate_trailing_stop_args(&args).is_err());
    }

    #[test]
    fn spot_and_outcome_markets_are_rejected_before_any_lookup() {
        for coin in ["HYPE/USDC", "#10", "+10"] {
            let mut args = parse(&["--retracement-pct", "5"]).unwrap();
            args.coin = coin.to_string();
            let err = validate_trailing_stop_args(&args).unwrap_err();
            assert!(matches!(err, CliError::Configuration(_)), "{coin}: {err}");
            assert!(err.to_string().contains("perpetual markets only"), "{err}");
        }
    }

    #[test]
    fn plan_serializes_the_documented_wire_shape() {
        let args = parse(&["--retracement-pct", "5.0", "--reduce-only"]).unwrap();
        let plan = prepare_trailing_stop_plan(&resolver(), &args).unwrap();
        assert_eq!(
            serde_json::to_value(Action::TrailingStop(plan.action.clone())).unwrap(),
            serde_json::json!({
                "type": "trailingStop",
                "asset": 0,
                "isBuy": false,
                "sz": "0.01",
                "reduceOnly": true,
                "retracement": { "pct": "5%" },
                "activationPx": null,
            })
        );

        let args = parse(&["--retracement-px", "150", "--activation-price", "60000.50"]).unwrap();
        let plan = prepare_trailing_stop_plan(&resolver(), &args).unwrap();
        assert_eq!(
            serde_json::to_value(Action::TrailingStop(plan.action.clone())).unwrap(),
            serde_json::json!({
                "type": "trailingStop",
                "asset": 0,
                "isBuy": false,
                "sz": "0.01",
                "reduceOnly": false,
                "retracement": { "px": "150" },
                "activationPx": "60000.5",
            })
        );
    }

    #[test]
    fn size_rounds_to_the_market_precision_and_rejects_zero() {
        let mut args = parse(&["--retracement-pct", "5"]).unwrap();
        args.size = Decimal::new(1_234_567, 6);
        let plan = prepare_trailing_stop_plan(&resolver(), &args).unwrap();
        assert_eq!(plan.size, Decimal::new(123_457, 5));

        args.size = Decimal::new(1, 8);
        let err = prepare_trailing_stop_plan(&resolver(), &args)
            .err()
            .expect("size below the lot rounds to zero");
        assert!(err.to_string().contains("rounds to zero"), "{err}");
    }

    #[test]
    fn hip3_perps_resolve_through_dex_or_qualified_coin() {
        let mut args = parse(&["--retracement-pct", "5"]).unwrap();
        args.coin = "HYPE".to_string();
        args.dex = Some("testdex".to_string());
        let plan = prepare_trailing_stop_plan(&resolver(), &args).unwrap();
        assert_eq!(plan.coin, "testdex:HYPE");
        assert_eq!(plan.action.asset, 11_100);

        args.coin = "testdex:HYPE".to_string();
        args.dex = None;
        assert_eq!(
            prepare_trailing_stop_plan(&resolver(), &args)
                .unwrap()
                .action
                .asset,
            11_100
        );
    }

    #[test]
    fn response_parsing_returns_oid_or_labelled_rejection() {
        let ok = serde_json::json!({"type": "trailingStop", "data": {"oid": 77738308}});
        assert_eq!(parse_trailing_stop_response(&ok).unwrap(), 77_738_308);

        let rejected = serde_json::json!({"type": "trailingStop", "data": {"error": "Bad stop"}});
        let err = parse_trailing_stop_response(&rejected).unwrap_err();
        assert!(err.to_string().contains("[untrusted remote data]"), "{err}");

        let odd = serde_json::json!({"type": "default"});
        assert!(matches!(
            parse_trailing_stop_response(&odd).unwrap_err(),
            CliError::UnknownOutcome(_)
        ));
    }
}
