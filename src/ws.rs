//! Hyperliquid WebSocket construction and one-shot reads.

use std::time::Duration;

use futures::StreamExt;
use hypersdk::hypercore::{self, Chain, Incoming, Subscription, WebSocket, ws::Event};

use crate::errors::CliError;

pub fn websocket_for_chain(chain: Chain) -> WebSocket {
    match chain {
        Chain::Mainnet => hypercore::mainnet_ws(),
        Chain::Testnet => hypercore::testnet_ws(),
    }
}

/// What a one-shot WebSocket read is waiting for, as phrased in its errors.
pub struct Awaited {
    /// Completes "WebSocket closed before receiving ...".
    pub closed: &'static str,
    /// Completes "Timed out waiting for Hyperliquid ...".
    pub timed_out: &'static str,
}

/// Subscribe on a fresh socket and return the first message `pick` accepts.
pub async fn first_matching_message<T>(
    chain: Chain,
    subscription: Subscription,
    timeout: Duration,
    awaited: &Awaited,
    mut pick: impl FnMut(Incoming) -> Option<T>,
) -> Result<T, CliError> {
    let mut ws = websocket_for_chain(chain);
    ws.subscribe(subscription);

    tokio::time::timeout(timeout, async {
        while let Some(event) = ws.next().await {
            if let Event::Message(message) = event
                && let Some(found) = pick(message)
            {
                return Ok(found);
            }
        }

        Err(CliError::Unavailable(format!(
            "WebSocket closed before receiving {}.",
            awaited.closed
        )))
    })
    .await
    .map_err(|_| {
        CliError::Unavailable(format!(
            "Timed out waiting for Hyperliquid {}. Check your network connection.",
            awaited.timed_out
        ))
    })?
}
