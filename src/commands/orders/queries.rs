use super::*;
use crate::assets::AssetResolver;
use crate::http_api::ApiClient;
use crate::output::JsonValueOutput;

/// List authenticated user's open orders.
pub async fn open(
    inv: &Invocation,
    resolver: &AssetResolver,
    user: Address,
) -> Result<(), anyhow::Error> {
    inv.print(&open_snapshot(inv.remote()?.api().sdk(), resolver, user).await?);
    Ok(())
}

/// Fetch authenticated user's open orders without printing them.
pub async fn open_snapshot(
    client: &HttpClient,
    resolver: &AssetResolver,
    user: Address,
) -> Result<OrderListOutput, anyhow::Error> {
    let rows = client
        .open_orders(user, None)
        .await
        .map_err(from_sdk)?
        .into_iter()
        .map(|order| OrderListRow::from(normalize_order_display_coin(resolver, order)))
        .collect();

    Ok(OrderListOutput {
        rows,
        empty_message: "No open orders.",
    })
}

/// Show a public order status by OID or CLOID.
pub async fn status(
    inv: &Invocation,
    user: Address,
    args: &StatusArgs,
) -> Result<(), anyhow::Error> {
    let oid = match (args.oid, args.cloid.as_deref()) {
        (Some(oid), None) => serde_json::json!(oid),
        (None, Some(cloid)) => {
            let cloid = parse_lookup_cloid(cloid)?;
            serde_json::json!(format!("{cloid:#x}"))
        }
        _ => {
            return Err(CliError::Configuration(
                "orders status requires --oid or --cloid".to_string(),
            )
            .into());
        }
    };
    let request = OrderStatusRequest {
        request_type: "orderStatus",
        user,
        oid,
    };
    let value = inv
        .remote()?
        .api()
        .info::<serde_json::Value>(&request, "loading order status")
        .await?;
    inv.print(&JsonValueOutput::new(value, "No order status."));
    Ok(())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct OrderStatusRequest {
    #[serde(rename = "type")]
    request_type: &'static str,
    user: Address,
    oid: serde_json::Value,
}

/// List authenticated user's historical orders.
pub async fn history(
    inv: &Invocation,
    resolver: &AssetResolver,
    user: Address,
) -> Result<(), anyhow::Error> {
    let rows = historical_orders(inv.remote()?.api(), user)
        .await?
        .into_iter()
        .map(|order| OrderListRow::from(normalize_order_display_coin(resolver, order)))
        .collect();

    inv.print(&OrderListOutput {
        rows,
        empty_message: "No historical orders.",
    });
    Ok(())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct HistoricalOrdersRequest {
    #[serde(rename = "type")]
    request_type: &'static str,
    user: Address,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum HistoricalOrderItem {
    Wrapped { order: BasicOrder },
    Plain(BasicOrder),
}

impl HistoricalOrderItem {
    fn into_order(self) -> BasicOrder {
        match self {
            Self::Wrapped { order, .. } | Self::Plain(order) => order,
        }
    }
}

async fn historical_orders(api: &ApiClient, user: Address) -> Result<Vec<BasicOrder>, CliError> {
    match api.sdk().historical_orders(user).await {
        Ok(orders) => Ok(orders.into_iter().map(|update| update.order).collect()),
        Err(err) if should_fallback_to_raw_historical_orders(&err) => {
            historical_orders_raw(api, user).await
        }
        Err(err) => Err(from_sdk(err)),
    }
}

fn normalize_order_display_coin(resolver: &AssetResolver, mut order: BasicOrder) -> BasicOrder {
    order.coin = resolver.display_coin(&order.coin);
    order
}

/// hypersdk decodes `historicalOrders` as `OrderUpdate<BasicOrder>`; when the
/// payload is a bare order list the decode fails with a serde error, and the raw
/// path accepts both shapes.
fn should_fallback_to_raw_historical_orders(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| cause.is::<serde_json::Error>())
}

async fn historical_orders_raw(
    api: &ApiClient,
    user: Address,
) -> Result<Vec<BasicOrder>, CliError> {
    let request = HistoricalOrdersRequest {
        request_type: "historicalOrders",
        user,
    };
    let items = api
        .info::<Vec<HistoricalOrderItem>>(&request, "loading historical orders")
        .await?;
    Ok(items
        .into_iter()
        .map(HistoricalOrderItem::into_order)
        .collect())
}
