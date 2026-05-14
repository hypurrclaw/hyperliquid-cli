use super::*;

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
}

impl OrderConfirmation {
    pub(super) fn from_status(
        prepared: &PreparedOrder,
        status: OrderResponseStatus,
    ) -> Result<Self, CliError> {
        match status {
            OrderResponseStatus::Success => Ok(Self {
                coin: prepared.coin.clone(),
                side: prepared.side.to_string(),
                order_type: prepared.order_type.to_string(),
                status: "accepted".to_string(),
                order_id: None,
                price: prepared.price.to_string(),
                trigger_price: prepared.trigger_price.map(|price| price.to_string()),
                size: prepared.size.to_string(),
                amount: prepared.amount.map(|amount| amount.to_string()),
                amount_unit: prepared
                    .amount
                    .as_ref()
                    .map(|_| prepared.amount_unit.clone()),
                tif: prepared.tif.map(|tif| tif.to_string()),
                reduce_only: prepared.request.reduce_only,
                warning: prepared.warning.clone(),
            }),
            OrderResponseStatus::Resting { oid, .. } => Ok(Self {
                coin: prepared.coin.clone(),
                side: prepared.side.to_string(),
                order_type: prepared.order_type.to_string(),
                status: "resting".to_string(),
                order_id: Some(oid),
                price: prepared.price.to_string(),
                trigger_price: prepared.trigger_price.map(|price| price.to_string()),
                size: prepared.size.to_string(),
                amount: prepared.amount.map(|amount| amount.to_string()),
                amount_unit: prepared
                    .amount
                    .as_ref()
                    .map(|_| prepared.amount_unit.clone()),
                tif: prepared.tif.map(|tif| tif.to_string()),
                reduce_only: prepared.request.reduce_only,
                warning: prepared.warning.clone(),
            }),
            OrderResponseStatus::Filled {
                oid,
                total_sz,
                avg_px,
            } => Ok(Self {
                coin: prepared.coin.clone(),
                side: prepared.side.to_string(),
                order_type: prepared.order_type.to_string(),
                status: "filled".to_string(),
                order_id: Some(oid),
                price: avg_px.to_string(),
                trigger_price: prepared.trigger_price.map(|price| price.to_string()),
                size: total_sz.to_string(),
                amount: prepared.amount.map(|amount| amount.to_string()),
                amount_unit: prepared
                    .amount
                    .as_ref()
                    .map(|_| prepared.amount_unit.clone()),
                tif: prepared.tif.map(|tif| tif.to_string()),
                reduce_only: prepared.request.reduce_only,
                warning: prepared.warning.clone(),
            }),
            OrderResponseStatus::Error(err) => {
                Err(CliError::Unsupported(format!("order rejected: {err}")))
            }
        }
    }
}

pub(super) struct OrderConfirmationOutput {
    pub(super) rows: Vec<OrderConfirmation>,
}

impl TableData for OrderConfirmationOutput {
    fn headers(&self) -> Vec<&str> {
        vec![
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
        ]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        self.rows
            .iter()
            .map(|row| {
                vec![
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
                ]
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
}

impl TpslOrderConfirmation {
    pub(super) fn from_tpsl_status(
        prepared: &PreparedTpslLeg,
        grouping: TpslGroupingArg,
        status: TpslResponseStatus,
    ) -> Result<Self, CliError> {
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
        };

        match status {
            TpslResponseStatus::Success => {
                row.status = "accepted".to_string();
                Ok(row)
            }
            TpslResponseStatus::Resting { oid, .. } => {
                row.status = "resting".to_string();
                row.order_id = Some(oid);
                Ok(row)
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
                Ok(row)
            }
            TpslResponseStatus::WaitingForFill => {
                row.status = "waiting_for_fill".to_string();
                Ok(row)
            }
            TpslResponseStatus::WaitingForTrigger => {
                row.status = "waiting_for_trigger".to_string();
                Ok(row)
            }
            TpslResponseStatus::Error(err) => {
                Err(CliError::Unsupported(format!("order rejected: {err}")))
            }
        }
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

impl TableData for TpslOrderConfirmationOutput {
    fn headers(&self) -> Vec<&str> {
        vec![
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
        ]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        self.rows
            .iter()
            .map(|row| {
                vec![
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
                ]
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
        if self.rows.is_empty() {
            vec!["Message"]
        } else {
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
    }

    fn rows(&self) -> Vec<Vec<String>> {
        if self.rows.is_empty() {
            return vec![vec![self.empty_message.to_string()]];
        }

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

    fn pretty_rows(&self) -> Vec<Vec<String>> {
        self.rows()
            .into_iter()
            .map(|mut row| {
                if self.rows.is_empty() {
                    row[0] = output::colors::gray(&row[0]);
                    return row;
                }

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
