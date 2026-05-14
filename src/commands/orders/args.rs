use super::validation::parse_relative_duration;
use super::*;

/// Arguments for `orders create`.
#[derive(Args, Debug, Clone)]
pub struct CreateArgs {
    /// Perpetual coin (for example: BTC, ETH, SOL)
    #[arg(long)]
    pub coin: String,

    /// HIP-3 DEX to trade on (equivalent to --coin dex:COIN)
    #[arg(long)]
    pub dex: Option<String>,

    /// Order side
    #[arg(long, value_enum)]
    pub side: OrderSide,

    /// Limit price. For stop-loss/take-profit market triggers, this is also accepted as a legacy trigger price.
    #[arg(long, allow_hyphen_values = true)]
    pub price: Option<Decimal>,

    /// Trigger price for stop-loss, take-profit, stop-limit, and take-limit orders.
    #[arg(long, allow_hyphen_values = true)]
    pub trigger_price: Option<Decimal>,

    /// Base-asset size. Required for limit and trigger orders.
    #[arg(long, allow_hyphen_values = true)]
    pub size: Option<Decimal>,

    /// Quote/collateral amount. Required for market orders (for example USDC, USDH, or the market's collateral token).
    #[arg(long, allow_hyphen_values = true)]
    pub amount: Option<Decimal>,

    /// Order type
    #[arg(long = "type", value_enum, default_value = "limit")]
    pub order_type: CreateOrderType,

    /// Time in force for limit orders
    #[arg(long, value_enum, default_value = "gtc")]
    pub tif: TifArg,

    /// Only reduce an existing perpetual position. Perps only; spot markets are not supported.
    /// Stop-loss and take-profit market triggers are always reduce-only.
    #[arg(long)]
    pub reduce_only: bool,

    /// Maximum slippage bound, in basis points, for market orders
    #[arg(
        long,
        default_value_t = DEFAULT_MARKET_ORDER_SLIPPAGE_BPS,
        value_parser = clap::value_parser!(u16).range(
            i64::from(MIN_MARKET_ORDER_SLIPPAGE_BPS)..=i64::from(MAX_MARKET_ORDER_SLIPPAGE_BPS)
        )
    )]
    pub max_slippage_bps: u16,

    /// Take-profit trigger price for a parent-attached TP/SL child order
    #[arg(long, allow_hyphen_values = true)]
    pub take_profit: Option<Decimal>,

    /// Stop-loss trigger price for a parent-attached TP/SL child order
    #[arg(long, allow_hyphen_values = true)]
    pub stop_loss: Option<Decimal>,

    /// Grouping for parent-attached TP/SL children
    #[arg(long, value_enum)]
    pub grouping: Option<CreateTpslGroupingArg>,

    /// Acting-account selector for vaultAddress: subaccount/vault address, stored account alias, or stored account id
    #[arg(long)]
    pub on_behalf_of: Option<String>,

    /// Margin mode intent for perpetual orders. Defaults to cross. Use `positions update-leverage` first to establish isolated mode for new entries.
    #[arg(long, value_enum)]
    pub margin_mode: Option<MarginModeArg>,

    /// Builder address to receive an additional order fee
    #[arg(long, requires = "builder_fee_rate")]
    pub builder: Option<String>,

    /// Builder fee rate as a percent string, e.g. 0.001%
    #[arg(long, requires = "builder")]
    pub builder_fee_rate: Option<String>,

    /// Client order ID (0x-prefixed 16-byte hex value)
    #[arg(long)]
    pub cloid: Option<String>,

    /// Skip the mainnet confirmation prompt for deliberate automation
    #[arg(short = 'y', long)]
    pub yes: bool,
}

/// Arguments for `orders scale`.
#[derive(Args, Debug, Clone)]
pub struct ScaleArgs {
    /// Perpetual coin (for example: BTC, ETH, SOL)
    #[arg(long)]
    pub coin: String,

    /// HIP-3 DEX to trade on (equivalent to --coin dex:COIN)
    #[arg(long)]
    pub dex: Option<String>,

    /// Order side
    #[arg(long, value_enum)]
    pub side: OrderSide,

    /// First limit price in the scale
    #[arg(long, allow_hyphen_values = true)]
    pub start_price: Decimal,

    /// Last limit price in the scale
    #[arg(long, allow_hyphen_values = true)]
    pub end_price: Decimal,

    /// Total base-asset size to split across all legs
    #[arg(long, allow_hyphen_values = true)]
    pub total_size: Decimal,

    /// Number of limit orders to generate
    #[arg(long = "orders")]
    pub order_count: usize,

    /// Time in force for generated limit orders
    #[arg(long, value_enum, default_value = "gtc")]
    pub tif: TifArg,

    /// Only reduce an existing perpetual position
    #[arg(long)]
    pub reduce_only: bool,

    /// Acting-account selector for vaultAddress: subaccount/vault address, stored account alias, or stored account id
    #[arg(long)]
    pub on_behalf_of: Option<String>,

    /// Margin mode intent for perpetual orders. Defaults to cross. Use `positions update-leverage` first to establish isolated mode for new entries.
    #[arg(long, value_enum)]
    pub margin_mode: Option<MarginModeArg>,

    /// Skip the mainnet confirmation prompt for deliberate automation
    #[arg(short = 'y', long)]
    pub yes: bool,
}

/// Arguments for `orders batch-create`.
#[derive(Args, Debug, Clone)]
pub struct BatchCreateArgs {
    /// JSON file containing an array of order legs
    #[arg(long)]
    pub orders_file: PathBuf,

    /// Acting-account selector for vaultAddress: subaccount/vault address, stored account alias, or stored account id
    #[arg(long)]
    pub on_behalf_of: Option<String>,

    /// Skip the mainnet confirmation prompt for deliberate automation
    #[arg(short = 'y', long)]
    pub yes: bool,
}

/// Arguments for `orders tpsl`.
#[derive(Args, Debug, Clone)]
pub struct TpslArgs {
    /// Perpetual coin (for example: BTC, ETH, SOL)
    #[arg(long)]
    pub coin: String,

    /// HIP-3 DEX to place TP/SL on (equivalent to --coin dex:COIN)
    #[arg(long)]
    pub dex: Option<String>,

    /// Take-profit trigger price
    #[arg(long, allow_hyphen_values = true)]
    pub take_profit: Option<Decimal>,

    /// Stop-loss trigger price
    #[arg(long, allow_hyphen_values = true)]
    pub stop_loss: Option<Decimal>,

    /// Position TP/SL grouping
    #[arg(long, value_enum, default_value = "position-tpsl")]
    pub grouping: PositionTpslGroupingArg,

    /// Fixed close side for dry-runs or explicit-size TP/SL orders; omitted live orders infer it from the current position
    #[arg(long, value_enum)]
    pub side: Option<OrderSide>,

    /// Fixed base-asset size; omitted live orders use the current position size
    #[arg(long, allow_hyphen_values = true)]
    pub size: Option<Decimal>,

    /// Margin mode intent for perpetual TP/SL attachment. Defaults to cross.
    #[arg(long, value_enum)]
    pub margin_mode: Option<MarginModeArg>,

    /// Skip the mainnet confirmation prompt for deliberate automation
    #[arg(short = 'y', long)]
    pub yes: bool,

    /// Client order ID (0x-prefixed 16-byte hex value)
    #[arg(long)]
    pub cloid: Option<String>,
}

/// Arguments for `orders cancel`.
#[derive(Args, Debug, Clone)]
pub struct CancelArgs {
    /// Exchange-assigned order ID
    #[arg(
        value_name = "ORDER_ID",
        required_unless_present = "cloid",
        conflicts_with = "cloid"
    )]
    pub order_id: Option<u64>,

    /// Client order ID (CLOID) to cancel
    #[arg(long, conflicts_with = "order_id")]
    pub cloid: Option<String>,
}

/// Arguments for `orders cancel-all`.
#[derive(Args, Debug, Clone)]
pub struct CancelAllArgs {
    /// Only cancel open orders for this perpetual coin
    #[arg(long)]
    pub coin: Option<String>,

    /// HIP-3 DEX for the --coin filter
    #[arg(long, requires = "coin")]
    pub dex: Option<String>,

    /// Skip confirmation prompt
    #[arg(short = 'y', long)]
    pub yes: bool,
}

/// Arguments for `orders modify`.
#[derive(Args, Debug, Clone)]
pub struct ModifyArgs {
    /// Exchange-assigned order ID
    #[arg(
        value_name = "ORDER_ID",
        required_unless_present = "cloid",
        conflicts_with = "cloid"
    )]
    pub order_id: Option<u64>,

    /// Client order ID (CLOID) to modify
    #[arg(long, conflicts_with = "order_id")]
    pub cloid: Option<String>,

    /// Replacement limit price. For legacy market trigger orders, this is accepted as the trigger price.
    #[arg(long, allow_hyphen_values = true)]
    pub price: Option<Decimal>,

    /// Replacement trigger price for trigger orders. Required when modifying stop-limit/take-limit orders.
    #[arg(long, allow_hyphen_values = true)]
    pub trigger_price: Option<Decimal>,

    /// Replacement order size
    #[arg(long, allow_hyphen_values = true)]
    pub size: Option<Decimal>,
}

/// Arguments for `orders twap-create`.
#[derive(Args, Debug, Clone)]
pub struct TwapCreateArgs {
    /// Perpetual coin (for example: BTC, ETH, SOL)
    #[arg(long)]
    pub coin: String,

    /// HIP-3 DEX to create the TWAP on (equivalent to --coin dex:COIN)
    #[arg(long)]
    pub dex: Option<String>,

    /// TWAP side
    #[arg(long, value_enum)]
    pub side: OrderSide,

    /// Total base-asset size to execute across TWAP slices
    #[arg(long, allow_hyphen_values = true)]
    pub size: Decimal,

    /// TWAP duration in seconds, as a whole-minute value (300, 600, ...)
    #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
    pub duration: u64,

    /// Margin mode intent for perpetual TWAP orders. Defaults to cross. Use `positions update-leverage` first to establish isolated mode for new entries.
    #[arg(long, value_enum)]
    pub margin_mode: Option<MarginModeArg>,

    /// Skip the mainnet confirmation prompt for deliberate automation
    #[arg(short = 'y', long)]
    pub yes: bool,
}

/// Arguments for `orders twap-cancel`.
#[derive(Args, Debug, Clone)]
pub struct TwapCancelArgs {
    /// TWAP ID returned by twap-create
    pub twap_id: u64,

    /// Perpetual coin for the TWAP being cancelled (for example: BTC, ETH, SOL)
    #[arg(long)]
    pub coin: String,

    /// HIP-3 DEX for the TWAP (equivalent to --coin dex:COIN)
    #[arg(long)]
    pub dex: Option<String>,
}

/// Arguments for `orders schedule-cancel`.
#[derive(Args, Debug, Clone)]
pub struct ScheduleCancelArgs {
    /// Relative duration before cancel-all triggers (for example: 5m, 30s, 1h)
    #[arg(long = "in", value_parser = parse_relative_duration)]
    pub in_duration: Option<Duration>,

    /// Remove an existing scheduled cancel trigger instead of setting one
    #[arg(long, conflicts_with = "in_duration")]
    pub clear: bool,
}

/// Arguments for `orders status`.
#[derive(Args, Debug, Clone)]
pub struct StatusArgs {
    /// User address, stored account alias, or stored account id
    #[arg(long)]
    pub user: String,

    /// Exchange-assigned order ID
    #[arg(long, required_unless_present = "cloid", conflicts_with = "cloid")]
    pub oid: Option<u64>,

    /// Client order ID (CLOID)
    #[arg(long, conflicts_with = "oid")]
    pub cloid: Option<String>,
}

/// Buy/sell order side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, ValueEnum, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum OrderSide {
    Buy,
    Sell,
}

/// Explicit perpetual margin-mode intent for order flows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, ValueEnum, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MarginModeArg {
    Cross,
    Isolated,
}

impl MarginModeArg {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cross => "cross",
            Self::Isolated => "isolated",
        }
    }
}

impl fmt::Display for MarginModeArg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl OrderSide {
    pub(super) fn is_buy(self) -> bool {
        matches!(self, Self::Buy)
    }

    pub(super) fn opposite(self) -> Self {
        match self {
            Self::Buy => Self::Sell,
            Self::Sell => Self::Buy,
        }
    }
}

impl fmt::Display for OrderSide {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Buy => write!(f, "buy"),
            Self::Sell => write!(f, "sell"),
        }
    }
}

/// Supported `orders create --type` values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CreateOrderType {
    Limit,
    Market,
    StopLoss,
    TakeProfit,
    StopLimit,
    TakeLimit,
}

impl fmt::Display for CreateOrderType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Limit => write!(f, "limit"),
            Self::Market => write!(f, "market"),
            Self::StopLoss => write!(f, "stop-loss"),
            Self::TakeProfit => write!(f, "take-profit"),
            Self::StopLimit => write!(f, "stop-limit"),
            Self::TakeLimit => write!(f, "take-limit"),
        }
    }
}

impl CreateOrderType {
    pub(super) fn is_trigger(self) -> bool {
        self.tpsl().is_some()
    }

    pub(super) fn uses_implicit_reduce_only(self) -> bool {
        matches!(self, Self::StopLoss | Self::TakeProfit)
    }

    pub(super) fn tpsl(self) -> Option<TpSl> {
        match self {
            Self::StopLoss | Self::StopLimit => Some(TpSl::Sl),
            Self::TakeProfit | Self::TakeLimit => Some(TpSl::Tp),
            Self::Limit | Self::Market => None,
        }
    }
}

/// CLI-facing TIF values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, ValueEnum, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TifArg {
    Gtc,
    Alo,
    Ioc,
}

impl From<TifArg> for TimeInForce {
    fn from(value: TifArg) -> Self {
        match value {
            TifArg::Gtc => Self::Gtc,
            TifArg::Alo => Self::Alo,
            TifArg::Ioc => Self::Ioc,
        }
    }
}

impl fmt::Display for TifArg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Gtc => write!(f, "gtc"),
            Self::Alo => write!(f, "alo"),
            Self::Ioc => write!(f, "ioc"),
        }
    }
}

/// CLI-facing `orders create` TP/SL grouping values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CreateTpslGroupingArg {
    NormalTpsl,
}

impl fmt::Display for CreateTpslGroupingArg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NormalTpsl => write!(f, "normal-tpsl"),
        }
    }
}

/// CLI-facing `orders tpsl` TP/SL grouping values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PositionTpslGroupingArg {
    PositionTpsl,
}

impl PositionTpslGroupingArg {
    pub(super) fn wire_value(self) -> &'static str {
        match self {
            Self::PositionTpsl => "positionTpsl",
        }
    }
}

impl fmt::Display for PositionTpslGroupingArg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PositionTpsl => write!(f, "position-tpsl"),
        }
    }
}

/// Internal TP/SL grouping values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TpslGroupingArg {
    NormalTpsl,
    PositionTpsl,
}

impl TpslGroupingArg {
    pub(super) fn to_sdk(self) -> OrderGrouping {
        match self {
            Self::NormalTpsl => OrderGrouping::NormalTpsl,
            Self::PositionTpsl => OrderGrouping::PositionTpsl,
        }
    }

    pub(super) fn wire_value(self) -> &'static str {
        match self {
            Self::NormalTpsl => "normalTpsl",
            Self::PositionTpsl => "positionTpsl",
        }
    }
}

impl fmt::Display for TpslGroupingArg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NormalTpsl => write!(f, "normal-tpsl"),
            Self::PositionTpsl => write!(f, "position-tpsl"),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct BatchCreateOrder {
    pub(super) coin: String,
    #[serde(default)]
    pub(super) dex: Option<String>,
    pub(super) side: OrderSide,
    pub(super) price: Decimal,
    pub(super) size: Decimal,
    #[serde(default)]
    pub(super) tif: Option<TifArg>,
    #[serde(default)]
    pub(super) reduce_only: bool,
    #[serde(default)]
    pub(super) cloid: Option<String>,
}
