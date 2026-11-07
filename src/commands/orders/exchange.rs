//! One reading of an exchange order/cancel/modify status list.
//!
//! Placement rows, chase, and bracket all ask [`Placement`] what a status
//! means. The wire decode stays here so a new waiting status is handled once.

use hypersdk::Decimal;
use hypersdk::hypercore::types::OrderResponseStatus;
use serde::Deserialize;

use crate::errors::CliError;
use crate::response_sanitization::labelled_untrusted_text;

use super::rendering;

/// What an order-placement status means for a row or a live tracker.
pub(crate) enum Placement {
    /// Accepted, waiting, or resting. `oid` is set only when the exchange
    /// assigned one (`resting`).
    Open {
        label: &'static str,
        oid: Option<u64>,
    },
    Filled {
        oid: u64,
        total_sz: Decimal,
        avg_px: Decimal,
    },
    Rejected(String),
}

impl Placement {
    pub(crate) fn classify(status: OrderResponseStatus) -> Self {
        match status {
            OrderResponseStatus::Success => Self::Open {
                label: "accepted",
                oid: None,
            },
            OrderResponseStatus::WaitingForTrigger => Self::Open {
                label: "waiting_for_trigger",
                oid: None,
            },
            OrderResponseStatus::WaitingForFill => Self::Open {
                label: "waiting_for_fill",
                oid: None,
            },
            OrderResponseStatus::Resting { oid, .. } => Self::Open {
                label: "resting",
                oid: Some(oid),
            },
            OrderResponseStatus::Filled {
                oid,
                total_sz,
                avg_px,
            } => Self::Filled {
                oid,
                total_sz,
                avg_px,
            },
            OrderResponseStatus::Error(err) => Self::Rejected(err),
        }
    }

    pub(crate) fn label(&self) -> &'static str {
        match self {
            Self::Open { label, .. } => label,
            Self::Filled { .. } => "filled",
            Self::Rejected(_) => "rejected",
        }
    }

    pub(crate) fn oid(&self) -> Option<u64> {
        match self {
            Self::Open { oid, .. } => *oid,
            Self::Filled { oid, .. } => Some(*oid),
            Self::Rejected(_) => None,
        }
    }
}

pub(crate) fn parse_order_statuses(
    response: serde_json::Value,
) -> Result<Vec<OrderResponseStatus>, CliError> {
    let body = || labelled_untrusted_text(&response.to_string());
    if response.get("type").and_then(serde_json::Value::as_str) != Some("order") {
        return Err(CliError::Internal(anyhow::anyhow!(
            "order placement response had unexpected type: {}",
            body()
        )));
    }
    let statuses = response
        .get("data")
        .and_then(|data| data.get("statuses"))
        .cloned()
        .ok_or_else(|| {
            CliError::Internal(anyhow::anyhow!(
                "order placement response missing statuses: {}",
                body()
            ))
        })?;
    serde_json::from_value(statuses).map_err(|err| {
        CliError::Internal(anyhow::anyhow!(
            "order placement response decode failed: {err}; body={}",
            body()
        ))
    })
}

pub(crate) fn order_statuses_from_response(
    response: serde_json::Value,
    action: &'static str,
) -> Result<Vec<OrderResponseStatus>, CliError> {
    #[derive(Deserialize)]
    struct StatusesResponse {
        statuses: Vec<OrderResponseStatus>,
    }

    #[derive(Deserialize)]
    #[serde(tag = "type", content = "data", rename_all = "camelCase")]
    enum TaggedResponse {
        Cancel(StatusesResponse),
        Order(StatusesResponse),
    }

    if let Ok(parsed) = serde_json::from_value::<StatusesResponse>(response.clone()) {
        return Ok(parsed.statuses);
    }

    serde_json::from_value::<TaggedResponse>(response)
        .map(|parsed| match parsed {
            TaggedResponse::Cancel(data) | TaggedResponse::Order(data) => data.statuses,
        })
        .map_err(|err| {
            CliError::Internal(anyhow::anyhow!(
                "{action}: unexpected response shape: {err}"
            ))
        })
}

/// Fail on the first rejected status, with the exchange text sanitized and
/// labelled. `action` completes "Couldn't ...", such as `cancel order`.
pub(crate) fn ensure_action_statuses_ok(
    statuses: &[OrderResponseStatus],
    action: &str,
) -> Result<usize, CliError> {
    for status in statuses {
        if let Some(err) = status.error() {
            return Err(rendering::status_error(action, err));
        }
    }
    Ok(statuses.len())
}

pub(crate) fn modification_result_oid(statuses: &[OrderResponseStatus]) -> Option<u64> {
    statuses.iter().find_map(OrderResponseStatus::oid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placement_labels_match_the_row_vocabulary() {
        let cases = [
            (OrderResponseStatus::Success, "accepted", None),
            (
                OrderResponseStatus::WaitingForTrigger,
                "waiting_for_trigger",
                None,
            ),
            (
                OrderResponseStatus::WaitingForFill,
                "waiting_for_fill",
                None,
            ),
            (
                OrderResponseStatus::Resting {
                    oid: 9,
                    cloid: None,
                },
                "resting",
                Some(9),
            ),
            (
                OrderResponseStatus::Filled {
                    oid: 4,
                    total_sz: Decimal::ONE,
                    avg_px: Decimal::from(2),
                },
                "filled",
                Some(4),
            ),
            (
                OrderResponseStatus::Error("no".to_string()),
                "rejected",
                None,
            ),
        ];
        for (status, label, oid) in cases {
            let placed = Placement::classify(status);
            assert_eq!(placed.label(), label);
            assert_eq!(placed.oid(), oid);
        }
    }

    #[test]
    fn order_parser_accepts_resting_and_waiting_without_a_second_status_type() {
        let response = serde_json::json!({
            "type": "order",
            "data": {
                "statuses": [
                    {"resting": {"oid": 12345}},
                    "waitingForFill",
                    "waitingForTrigger"
                ]
            }
        });
        let statuses = parse_order_statuses(response).unwrap();
        assert!(matches!(
            statuses.as_slice(),
            [
                OrderResponseStatus::Resting { oid: 12345, .. },
                OrderResponseStatus::WaitingForFill,
                OrderResponseStatus::WaitingForTrigger
            ]
        ));
        assert_eq!(
            ensure_action_statuses_ok(&statuses, "place order").unwrap(),
            3
        );
        assert_eq!(modification_result_oid(&statuses), Some(12345));
    }
}
