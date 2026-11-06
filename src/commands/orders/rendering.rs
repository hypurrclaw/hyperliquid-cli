use super::*;

/// Error for a rejected exchange status. The exchange text is remote data, so
/// it is sanitized and labelled before it reaches a terminal or an agent.
pub(super) fn status_error(action: &str, err: &str) -> CliError {
    CliError::Unsupported(format!(
        "Couldn't {action}. Exchange said: {}",
        labelled_untrusted_text(err)
    ))
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct OrderConfirmation {
    coin: String,
    side: String,
    order_type: String,
    status: String,
    order_id: Option<u64>,
    price: String,
    trigger_price: Option<String>,
    size: String,
    amount: Option<String>,
    amount_unit: Option<String>,
    tif: Option<String>,
    reduce_only: bool,
    warning: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl OrderConfirmation {
    fn from_prepared(
        prepared: &PreparedOrder,
        status: &str,
        order_id: Option<u64>,
        price: Decimal,
        size: Decimal,
    ) -> Self {
        Self {
            coin: prepared.coin.clone(),
            side: prepared.side.to_string(),
            order_type: prepared.order_type.to_string(),
            status: status.to_string(),
            order_id,
            price: price.to_string(),
            trigger_price: prepared.trigger_price.map(|price| price.to_string()),
            size: size.to_string(),
            amount: prepared.amount.map(|amount| amount.to_string()),
            amount_unit: prepared
                .amount
                .as_ref()
                .map(|_| prepared.amount_unit.clone()),
            tif: prepared.tif.map(|tif| tif.to_string()),
            reduce_only: prepared.request.reduce_only,
            warning: prepared.warning.clone(),
            error: None,
        }
    }

    /// Build the row for one leg. A rejected leg becomes a `rejected` row that
    /// carries the sanitized exchange error, so one bad leg never hides the
    /// legs that were placed.
    pub(super) fn from_leg_status(prepared: &PreparedOrder, status: OrderResponseStatus) -> Self {
        match status {
            OrderResponseStatus::Success => {
                Self::from_prepared(prepared, "accepted", None, prepared.price, prepared.size)
            }
            OrderResponseStatus::WaitingForTrigger => Self::from_prepared(
                prepared,
                "waiting_for_trigger",
                None,
                prepared.price,
                prepared.size,
            ),
            OrderResponseStatus::WaitingForFill => Self::from_prepared(
                prepared,
                "waiting_for_fill",
                None,
                prepared.price,
                prepared.size,
            ),
            OrderResponseStatus::Resting { oid, .. } => Self::from_prepared(
                prepared,
                "resting",
                Some(oid),
                prepared.price,
                prepared.size,
            ),
            OrderResponseStatus::Filled {
                oid,
                total_sz,
                avg_px,
            } => Self::from_prepared(prepared, "filled", Some(oid), avg_px, total_sz),
            OrderResponseStatus::Error(err) => {
                let mut row =
                    Self::from_prepared(prepared, "rejected", None, prepared.price, prepared.size);
                row.error = Some(labelled_untrusted_text(&err));
                row
            }
        }
    }

    /// Build the row for a single order; a rejection is an error.
    pub(super) fn from_status(
        prepared: &PreparedOrder,
        status: OrderResponseStatus,
    ) -> Result<Self, CliError> {
        match status {
            OrderResponseStatus::Error(err) => Err(status_error("place order", &err)),
            other => Ok(Self::from_leg_status(prepared, other)),
        }
    }
}

pub(super) struct OrderConfirmationOutput {
    pub(super) rows: Vec<OrderConfirmation>,
}

impl OrderConfirmationOutput {
    fn has_errors(&self) -> bool {
        self.rows.iter().any(|row| row.error.is_some())
    }

    pub(super) fn rejection_errors(&self) -> Vec<String> {
        rejection_errors(self.rows.iter().map(|row| row.error.as_deref()))
    }
}

impl TableData for OrderConfirmationOutput {
    fn headers(&self) -> Vec<&str> {
        let mut headers = vec![
            "Coin",
            "Side",
            "Type",
            "Status",
            "Order ID",
            "Price",
            "Trigger Price",
            "Size",
            "Amount",
            "Amount Unit",
            "TIF",
            "Reduce Only",
            "Warning",
        ];
        if self.has_errors() {
            headers.push("Error");
        }
        headers
    }

    fn rows(&self) -> Vec<Vec<String>> {
        let with_errors = self.has_errors();
        self.rows
            .iter()
            .map(|row| {
                let mut cells = vec![
                    row.coin.clone(),
                    row.side.clone(),
                    row.order_type.clone(),
                    row.status.clone(),
                    row.order_id.map(|oid| oid.to_string()).unwrap_or_default(),
                    row.price.clone(),
                    row.trigger_price.clone().unwrap_or_default(),
                    row.size.clone(),
                    row.amount.clone().unwrap_or_default(),
                    row.amount_unit.clone().unwrap_or_default(),
                    row.tif.clone().unwrap_or_default(),
                    row.reduce_only.to_string(),
                    row.warning.clone().unwrap_or_default(),
                ];
                if with_errors {
                    cells.push(row.error.clone().unwrap_or_default());
                }
                cells
            })
            .collect()
    }

    fn pretty_rows(&self) -> Vec<Vec<String>> {
        self.rows()
            .into_iter()
            .map(|mut row| {
                if let Some(warning) = row.get_mut(12)
                    && !warning.is_empty()
                {
                    *warning = output::colors::yellow(warning);
                }
                row
            })
            .collect()
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(&self.rows).unwrap_or_else(|_| serde_json::json!([]))
    }
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct TpslOrderConfirmation {
    grouping: String,
    grouping_wire: String,
    leg: String,
    coin: String,
    side: String,
    order_type: String,
    status: String,
    order_id: Option<u64>,
    price: String,
    size: String,
    reduce_only: bool,
    tif: Option<String>,
    warning: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl TpslOrderConfirmation {
    /// Build the row for one leg. A rejected leg becomes a `rejected` row that
    /// carries the sanitized exchange error, so one bad leg never hides the
    /// legs that were placed.
    pub(super) fn from_tpsl_status(
        prepared: &PreparedTpslLeg,
        grouping: TpslGroupingArg,
        status: TpslResponseStatus,
    ) -> Self {
        let mut row = Self {
            grouping: grouping.to_string(),
            grouping_wire: grouping.wire_value().to_string(),
            leg: prepared.leg.to_string(),
            coin: prepared.coin.clone(),
            side: prepared.side.to_string(),
            order_type: prepared.order_type.to_string(),
            status: String::new(),
            order_id: None,
            price: prepared.price.to_string(),
            size: prepared.size.to_string(),
            reduce_only: prepared.reduce_only,
            tif: prepared.tif.map(|tif| tif.to_string()),
            warning: prepared.warning.clone(),
            error: None,
        };

        match status {
            TpslResponseStatus::Success => {
                row.status = "accepted".to_string();
            }
            TpslResponseStatus::Resting { oid, .. } => {
                row.status = "resting".to_string();
                row.order_id = Some(oid);
            }
            TpslResponseStatus::Filled {
                oid,
                total_sz,
                avg_px,
            } => {
                row.status = "filled".to_string();
                row.order_id = Some(oid);
                row.price = avg_px.to_string();
                row.size = total_sz.to_string();
            }
            TpslResponseStatus::WaitingForFill => {
                row.status = "waiting_for_fill".to_string();
            }
            TpslResponseStatus::WaitingForTrigger => {
                row.status = "waiting_for_trigger".to_string();
            }
            TpslResponseStatus::Error(err) => {
                row.status = "rejected".to_string();
                row.error = Some(labelled_untrusted_text(&err));
            }
        }
        row
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum TpslResponseStatus {
    Success,
    Resting {
        oid: u64,
    },
    Filled {
        #[serde(rename = "totalSz")]
        total_sz: Decimal,
        #[serde(rename = "avgPx")]
        avg_px: Decimal,
        oid: u64,
    },
    Error(String),
    WaitingForFill,
    WaitingForTrigger,
}

pub(super) struct TpslOrderConfirmationOutput {
    pub(super) rows: Vec<TpslOrderConfirmation>,
}

impl TpslOrderConfirmationOutput {
    fn has_errors(&self) -> bool {
        self.rows.iter().any(|row| row.error.is_some())
    }

    pub(super) fn rejection_errors(&self) -> Vec<String> {
        rejection_errors(self.rows.iter().map(|row| row.error.as_deref()))
    }
}

impl TableData for TpslOrderConfirmationOutput {
    fn headers(&self) -> Vec<&str> {
        let mut headers = vec![
            "Grouping",
            "Leg",
            "Coin",
            "Side",
            "Type",
            "Status",
            "Order ID",
            "Price",
            "Size",
            "Reduce Only",
            "TIF",
            "Warning",
        ];
        if self.has_errors() {
            headers.push("Error");
        }
        headers
    }

    fn rows(&self) -> Vec<Vec<String>> {
        let with_errors = self.has_errors();
        self.rows
            .iter()
            .map(|row| {
                let mut cells = vec![
                    row.grouping.clone(),
                    row.leg.clone(),
                    row.coin.clone(),
                    row.side.clone(),
                    row.order_type.clone(),
                    row.status.clone(),
                    row.order_id.map(|oid| oid.to_string()).unwrap_or_default(),
                    row.price.clone(),
                    row.size.clone(),
                    row.reduce_only.to_string(),
                    row.tif.clone().unwrap_or_default(),
                    row.warning.clone().unwrap_or_default(),
                ];
                if with_errors {
                    cells.push(row.error.clone().unwrap_or_default());
                }
                cells
            })
            .collect()
    }

    fn pretty_rows(&self) -> Vec<Vec<String>> {
        self.rows()
            .into_iter()
            .map(|mut row| {
                if let Some(warning) = row.get_mut(11)
                    && !warning.is_empty()
                {
                    *warning = output::colors::yellow(warning);
                }
                row
            })
            .collect()
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(&self.rows).unwrap_or_else(|_| serde_json::json!([]))
    }
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct CancelConfirmation {
    pub(super) coin: String,
    pub(super) status: String,
    pub(super) order_id: Option<u64>,
    pub(super) cloid: Option<String>,
}

pub(super) struct CancelConfirmationOutput {
    pub(super) rows: Vec<CancelConfirmation>,
}

impl TableData for CancelConfirmationOutput {
    fn headers(&self) -> Vec<&str> {
        vec!["Coin", "Status", "Order ID", "CLOID"]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        self.rows
            .iter()
            .map(|row| {
                vec![
                    row.coin.clone(),
                    row.status.clone(),
                    row.order_id.map(|oid| oid.to_string()).unwrap_or_default(),
                    row.cloid.clone().unwrap_or_default(),
                ]
            })
            .collect()
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(&self.rows).unwrap_or_else(|_| serde_json::json!([]))
    }
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct CancelAllSummary {
    pub(super) coin: String,
    pub(super) cancelled_orders: usize,
}

pub(super) struct CancelAllSummaryOutput {
    pub(super) rows: Vec<CancelAllSummary>,
}

impl TableData for CancelAllSummaryOutput {
    fn headers(&self) -> Vec<&str> {
        vec!["Coin", "Cancelled Orders"]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        self.rows
            .iter()
            .map(|row| vec![row.coin.clone(), row.cancelled_orders.to_string()])
            .collect()
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(&self.rows).unwrap_or_else(|_| serde_json::json!([]))
    }
}

/// One order in a cancel-all result.
#[derive(Debug, Clone, Serialize)]
pub(super) struct CancelLeg {
    pub(super) order_id: u64,
    pub(super) status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) error: Option<String>,
}

impl CancelLeg {
    pub(super) fn from_status(order_id: u64, status: &OrderResponseStatus) -> Self {
        match status {
            OrderResponseStatus::Error(err) => Self {
                order_id,
                status: "failed".to_string(),
                error: Some(labelled_untrusted_text(err)),
            },
            _ => Self {
                order_id,
                status: "cancelled".to_string(),
                error: None,
            },
        }
    }
}

pub(super) struct CancelLegsOutput {
    pub(super) rows: Vec<CancelLeg>,
}

impl TableData for CancelLegsOutput {
    fn headers(&self) -> Vec<&str> {
        vec!["Order ID", "Status", "Error"]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        self.rows
            .iter()
            .map(|row| {
                vec![
                    row.order_id.to_string(),
                    row.status.clone(),
                    row.error.clone().unwrap_or_default(),
                ]
            })
            .collect()
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(&self.rows).unwrap_or_else(|_| serde_json::json!([]))
    }
}

/// Command to run after a partial result to see what is live on the exchange.
pub(super) const CHECK_LIVE_COMMAND: &str = "hyperliquid orders open";

/// Per-leg result of a batch where some legs succeeded and some failed.
///
/// In JSON it is one object that also carries the standard error keys
/// (`error`, `category`, `exit_code`), so the caller prints one document and
/// the top-level error path stays silent for `PartialResults`.
pub(super) struct PartialBatchOutput {
    pub(super) legs: Box<dyn TableData>,
    pub(super) legs_key: &'static str,
    pub(super) succeeded: usize,
    pub(super) failed: usize,
    pub(super) headline: String,
}

impl PartialBatchOutput {
    /// Error returned after the result document is printed. Pretty and table
    /// runs show it on stderr; JSON runs print nothing more.
    pub(super) fn error(&self) -> CliError {
        CliError::PartialResults(format!(
            "{}\n\nCheck what is live:\n    {CHECK_LIVE_COMMAND}",
            self.headline
        ))
    }
}

impl TableData for PartialBatchOutput {
    fn headers(&self) -> Vec<&str> {
        self.legs.headers()
    }

    fn rows(&self) -> Vec<Vec<String>> {
        self.legs.rows()
    }

    fn pretty_rows(&self) -> Vec<Vec<String>> {
        self.legs.pretty_rows()
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::json!({
            "status": "partial",
            "error": self.headline,
            "category": "partial_failure",
            "exit_code": 15,
            "atomic": false,
            "succeeded": self.succeeded,
            "failed": self.failed,
            "total": self.succeeded + self.failed,
            self.legs_key: self.legs.to_json_value(),
            "next_command": CHECK_LIVE_COMMAND,
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct ModifyConfirmation {
    pub(super) coin: String,
    pub(super) status: String,
    pub(super) order_id: Option<u64>,
    pub(super) cloid: Option<String>,
    pub(super) price: String,
    pub(super) size: String,
}

pub(super) struct ModifyConfirmationOutput {
    pub(super) rows: Vec<ModifyConfirmation>,
}

impl TableData for ModifyConfirmationOutput {
    fn headers(&self) -> Vec<&str> {
        vec!["Coin", "Status", "Order ID", "CLOID", "Price", "Size"]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        self.rows
            .iter()
            .map(|row| {
                vec![
                    row.coin.clone(),
                    row.status.clone(),
                    row.order_id.map(|oid| oid.to_string()).unwrap_or_default(),
                    row.cloid.clone().unwrap_or_default(),
                    row.price.clone(),
                    row.size.clone(),
                ]
            })
            .collect()
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(&self.rows).unwrap_or_else(|_| serde_json::json!([]))
    }
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct OrderListRow {
    coin: String,
    side: String,
    #[serde(with = "rust_decimal::serde::str")]
    limit_price: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    size: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    original_size: Decimal,
    oid: u64,
    order_type: String,
    tif: Option<String>,
    reduce_only: bool,
    timestamp: u64,
    cloid: Option<String>,
}

impl From<BasicOrder> for OrderListRow {
    fn from(order: BasicOrder) -> Self {
        Self {
            coin: order.coin,
            side: order.side.to_string(),
            limit_price: order.limit_px,
            size: order.sz,
            original_size: order.orig_sz,
            oid: order.oid,
            order_type: format!("{:?}", order.order_type),
            tif: order.tif.map(|tif| format!("{tif:?}")),
            reduce_only: order.reduce_only,
            timestamp: order.timestamp,
            cloid: order.cloid.map(|cloid| cloid.to_string()),
        }
    }
}

pub struct OrderListOutput {
    pub(super) rows: Vec<OrderListRow>,
    pub(super) empty_message: &'static str,
}

impl TableData for OrderListOutput {
    fn headers(&self) -> Vec<&str> {
        vec![
            "Coin",
            "Side",
            "Limit Price",
            "Size",
            "Original Size",
            "OID",
            "Type",
            "TIF",
            "Reduce Only",
            "Timestamp",
        ]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        self.rows
            .iter()
            .map(|order| {
                vec![
                    order.coin.clone(),
                    order.side.clone(),
                    order.limit_price.to_string(),
                    order.size.to_string(),
                    order.original_size.to_string(),
                    order.oid.to_string(),
                    order.order_type.clone(),
                    order.tif.clone().unwrap_or_else(|| "n/a".to_string()),
                    order.reduce_only.to_string(),
                    order.timestamp.to_string(),
                ]
            })
            .collect()
    }

    fn empty_message(&self) -> Option<&str> {
        Some(self.empty_message)
    }

    fn pretty_rows(&self) -> Vec<Vec<String>> {
        self.rows()
            .into_iter()
            .map(|mut row| {
                if let Some(tif) = row.get_mut(7)
                    && tif == "n/a"
                {
                    *tif = output::colors::gray(tif);
                }
                row
            })
            .collect()
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(&self.rows).unwrap_or_else(|_| serde_json::json!([]))
    }
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct TwapCreateConfirmation {
    pub(super) coin: String,
    pub(super) side: String,
    #[serde(with = "rust_decimal::serde::str")]
    pub(super) size: Decimal,
    pub(super) duration_seconds: u64,
    pub(super) duration_minutes: u64,
    pub(super) status: String,
    pub(super) twap_id: u64,
}

pub(super) struct TwapCreateOutput {
    pub(super) rows: Vec<TwapCreateConfirmation>,
}

impl TableData for TwapCreateOutput {
    fn headers(&self) -> Vec<&str> {
        vec![
            "Coin",
            "Side",
            "Size",
            "Duration Seconds",
            "Duration Minutes",
            "Status",
            "TWAP ID",
        ]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        self.rows
            .iter()
            .map(|row| {
                vec![
                    row.coin.clone(),
                    row.side.clone(),
                    row.size.to_string(),
                    row.duration_seconds.to_string(),
                    row.duration_minutes.to_string(),
                    row.status.clone(),
                    row.twap_id.to_string(),
                ]
            })
            .collect()
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(&self.rows).unwrap_or_else(|_| serde_json::json!([]))
    }
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct TwapCancelConfirmation {
    pub(super) coin: String,
    pub(super) status: String,
    pub(super) twap_id: u64,
}

pub(super) struct TwapCancelOutput {
    pub(super) rows: Vec<TwapCancelConfirmation>,
}

impl TableData for TwapCancelOutput {
    fn headers(&self) -> Vec<&str> {
        vec!["Coin", "Status", "TWAP ID"]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        self.rows
            .iter()
            .map(|row| {
                vec![
                    row.coin.clone(),
                    row.status.clone(),
                    row.twap_id.to_string(),
                ]
            })
            .collect()
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(&self.rows).unwrap_or_else(|_| serde_json::json!([]))
    }
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct ScheduleCancelConfirmation {
    pub(super) status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) scheduled_time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) scheduled_time_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) in_seconds: Option<u64>,
}

pub(super) struct ScheduleCancelOutput {
    pub(super) rows: Vec<ScheduleCancelConfirmation>,
}

impl TableData for ScheduleCancelOutput {
    fn headers(&self) -> Vec<&str> {
        vec![
            "Status",
            "Scheduled Time",
            "Scheduled Time MS",
            "In Seconds",
        ]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        self.rows
            .iter()
            .map(|row| {
                vec![
                    row.status.clone(),
                    row.scheduled_time.clone().unwrap_or_default(),
                    row.scheduled_time_ms
                        .map(|value| value.to_string())
                        .unwrap_or_default(),
                    row.in_seconds
                        .map(|value| value.to_string())
                        .unwrap_or_default(),
                ]
            })
            .collect()
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(&self.rows).unwrap_or_else(|_| serde_json::json!([]))
    }
}
