use super::*;
use crate::commands::AssetResolver;
use crate::errors::http_response_indicates_rate_limit;
use crate::http_api::{decode_json, post_info_raw};
use crate::output::JsonValueOutput;

#[derive(Debug, Clone, PartialEq)]
pub struct OrderStatusResult {
    pub output: JsonValueOutput,
    pub elapsed: Duration,
}

/// List authenticated user's open orders.
pub async fn open(
    client: &HttpClient,
    resolver: &AssetResolver,
    user: Address,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let start = Instant::now();
    let output = open_snapshot(client, resolver, user).await?;

    output::print_data(&output, format, start.elapsed());
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
        .map_err(map_api_error)?
        .into_iter()
        .map(|order| OrderListRow::from(normalize_order_display_coin(resolver, order)))
        .collect();

    Ok(OrderListOutput {
        rows,
        empty_message: "No open orders",
    })
}

/// Show a public order status by OID or CLOID.
pub async fn status(
    api_base_url: &str,
    user: Address,
    args: &StatusArgs,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let result = status_query(api_base_url, user, args).await?;
    output::print_data(&result.output, format, result.elapsed);
    Ok(())
}

/// Fetch a public order status by OID or CLOID without printing it.
pub async fn status_query(
    api_base_url: &str,
    user: Address,
    args: &StatusArgs,
) -> Result<OrderStatusResult, anyhow::Error> {
    let start = Instant::now();
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
    let value =
        post_info_json::<serde_json::Value>(api_base_url, &request, "loading order status").await?;
    let output = JsonValueOutput::new(value, "no order status found");
    Ok(OrderStatusResult {
        output,
        elapsed: start.elapsed(),
    })
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
    client: &HttpClient,
    resolver: &AssetResolver,
    user: Address,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let start = Instant::now();
    let rows = historical_orders(client, user)
        .await?
        .into_iter()
        .map(|order| OrderListRow::from(normalize_order_display_coin(resolver, order)))
        .collect();

    output::print_data(
        &OrderListOutput {
            rows,
            empty_message: "No historical orders",
        },
        format,
        start.elapsed(),
    );
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

async fn historical_orders(
    client: &HttpClient,
    user: Address,
) -> Result<Vec<BasicOrder>, CliError> {
    match client.historical_orders(user).await {
        Ok(orders) => Ok(orders),
        Err(err) if should_fallback_to_raw_historical_orders(&err) => {
            historical_orders_raw(client, user).await
        }
        Err(err) => Err(map_api_error(err)),
    }
}

fn normalize_order_display_coin(resolver: &AssetResolver, mut order: BasicOrder) -> BasicOrder {
    order.coin = resolver.display_coin(&order.coin);
    order
}

fn should_fallback_to_raw_historical_orders(err: &anyhow::Error) -> bool {
    let message = err.to_string();
    message.contains("decode failed") && message.contains("missing field `timestamp`")
}

async fn historical_orders_raw(
    client: &HttpClient,
    user: Address,
) -> Result<Vec<BasicOrder>, CliError> {
    let api_url = crate::commands::raw_info_base_url(client)?;
    let request = HistoricalOrdersRequest {
        request_type: "historicalOrders",
        user,
    };
    let response = post_info_raw(api_url.as_str(), &request)
        .await
        .map_err(|err| match err {
            CliError::Unavailable(message) => map_api_error(anyhow::anyhow!(message)),
            other => other,
        })?;
    let status = response.status;
    let body = response.body;

    if http_response_indicates_rate_limit(status.as_u16(), &body) {
        return Err(CliError::RateLimited);
    }

    if !status.is_success() {
        return Err(map_api_error(anyhow::anyhow!("HTTP {status} body={body}")));
    }

    let items = decode_json::<Vec<HistoricalOrderItem>>(&body, "loading historical orders")?;
    Ok(items
        .into_iter()
        .map(HistoricalOrderItem::into_order)
        .collect())
}
