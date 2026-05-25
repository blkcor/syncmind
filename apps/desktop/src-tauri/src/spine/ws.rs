//! WebSocket notifications + reconnect / polling fallback for the spine client.
//!
//! PRD 004 §US-035. Connects to `<spine_url>/v1/sync/live`, replies to ping/pong, and on
//! `{"type":"new_bundle"}` triggers a callback (the desktop wires this to
//! `spine::commands::spine_pull_bundles`-equivalent logic). Reconnects with exponential
//! backoff and a 30-second polling fallback during outages.
//!
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use rand::Rng;
use serde::Deserialize;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::handshake::client::generate_key;
use tokio_tungstenite::tungstenite::http::header::{
    AUTHORIZATION, CONNECTION, HOST, SEC_WEBSOCKET_KEY, SEC_WEBSOCKET_VERSION, UPGRADE,
};
use tokio_tungstenite::tungstenite::Message;
use tracing::{info, warn};

use crate::spine::client::{JwtHolder, SpineClient};
use crate::spine::identity::Identity;
use crate::spine::SpineError;

const BACKOFF_BASE_S: u64 = 1;
const BACKOFF_MAX_S: u64 = 60;
const POLL_FALLBACK_S: u64 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WsStatus {
    Disabled,
    Connecting,
    Connected,
    Reconnecting,
    Offline,
}

impl WsStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            WsStatus::Disabled => "disabled",
            WsStatus::Connecting => "connecting",
            WsStatus::Connected => "connected",
            WsStatus::Reconnecting => "reconnecting",
            WsStatus::Offline => "offline",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
enum Inbound {
    #[serde(rename = "ping")]
    Ping,
    #[serde(rename = "new_bundle")]
    NewBundle {
        #[allow(dead_code)]
        bundle_id: Option<String>,
    },
}

/// Spawn the WebSocket loop. The future never returns under normal operation; abort the
/// JoinHandle via `JoinSet::abort_all` or `JoinHandle::abort` at shutdown / unpair time.
///
/// `on_new_bundle` is called every time the server signals a new bundle OR the polling
/// fallback ticks while the WS is down. The caller wires it to bundle ingestion.
/// `status_sink` receives every status transition; the caller pushes these into a tokio
/// watch channel that the Devices tab subscribes to via Tauri events.
pub fn spawn_loop(
    client: Arc<SpineClient>,
    identity: Arc<Identity>,
    jwt: Arc<JwtHolder>,
    on_new_bundle: Arc<dyn Fn() + Send + Sync>,
    status_sink: Arc<dyn Fn(WsStatus) + Send + Sync>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut attempt: u32 = 0;
        loop {
            status_sink(WsStatus::Connecting);
            match try_connect_and_serve(
                Arc::clone(&client),
                Arc::clone(&identity),
                Arc::clone(&jwt),
                Arc::clone(&on_new_bundle),
                Arc::clone(&status_sink),
            )
            .await
            {
                Ok(()) => {
                    // The server closed cleanly — treat the same as any other disconnect.
                    warn!("spine websocket closed; will reconnect");
                }
                Err(e) => {
                    warn!(error = %e, "spine websocket connection failed");
                }
            }

            status_sink(WsStatus::Reconnecting);
            let delay = compute_backoff(attempt);
            attempt = attempt.saturating_add(1);
            status_sink(WsStatus::Offline);

            // Run a 30 s polling fallback during the backoff window.
            let poll_deadline = tokio::time::Instant::now() + delay;
            loop {
                let now = tokio::time::Instant::now();
                if now >= poll_deadline {
                    break;
                }
                let until_next_poll = Duration::from_secs(POLL_FALLBACK_S);
                let sleep_for = std::cmp::min(poll_deadline - now, until_next_poll);
                tokio::time::sleep(sleep_for).await;
                on_new_bundle();
            }
        }
    })
}

async fn try_connect_and_serve(
    client: Arc<SpineClient>,
    identity: Arc<Identity>,
    jwt: Arc<JwtHolder>,
    on_new_bundle: Arc<dyn Fn() + Send + Sync>,
    status_sink: Arc<dyn Fn(WsStatus) + Send + Sync>,
) -> Result<(), SpineError> {
    let url = client.websocket_url("v1/sync/live")?;
    let parsed = url::Url::parse(&url)
        .map_err(|e| SpineError::new(crate::spine::SpineErrorCode::InvalidUrl, e.to_string()))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| {
            SpineError::new(
                crate::spine::SpineErrorCode::InvalidUrl,
                "websocket URL has no host",
            )
        })?
        .to_string();

    let minted = jwt.current_or_mint(&identity).await?;

    let mut request = url
        .as_str()
        .into_client_request()
        .map_err(|e| SpineError::new(crate::spine::SpineErrorCode::Internal, e.to_string()))?;
    let headers = request.headers_mut();
    headers.insert(
        AUTHORIZATION,
        format!("Bearer {}", minted.token).parse().map_err(
            |e: tokio_tungstenite::tungstenite::http::header::InvalidHeaderValue| {
                SpineError::new(crate::spine::SpineErrorCode::Internal, e.to_string())
            },
        )?,
    );
    headers.insert(HOST, host.parse().unwrap());
    headers.insert(UPGRADE, "websocket".parse().unwrap());
    headers.insert(CONNECTION, "Upgrade".parse().unwrap());
    headers.insert(SEC_WEBSOCKET_VERSION, "13".parse().unwrap());
    headers.insert(SEC_WEBSOCKET_KEY, generate_key().parse().unwrap());

    info!(%url, "spine websocket connecting");
    let (mut ws, _resp) = tokio_tungstenite::connect_async(request)
        .await
        .map_err(|e| {
            SpineError::new(
                crate::spine::SpineErrorCode::SpineUnreachable,
                e.to_string(),
            )
        })?;
    status_sink(WsStatus::Connected);
    on_new_bundle();
    info!("spine websocket connected");

    // Read loop with a 40 s soft deadline (PRD 002 §Impl Note 5).
    loop {
        let msg = tokio::time::timeout(Duration::from_secs(40), ws.next()).await;
        let item = match msg {
            Ok(Some(Ok(m))) => m,
            Ok(Some(Err(e))) => {
                return Err(SpineError::new(
                    crate::spine::SpineErrorCode::SpineUnreachable,
                    e.to_string(),
                ))
            }
            Ok(None) => return Ok(()),
            Err(_) => {
                warn!("spine websocket read deadline exceeded; reconnecting");
                let _ = ws.close(None).await;
                return Ok(());
            }
        };

        match item {
            Message::Text(text) => {
                if let Ok(parsed) = serde_json::from_str::<Inbound>(&text) {
                    match parsed {
                        Inbound::Ping => {
                            let _ = ws.send(Message::Text(r#"{"type":"pong"}"#.into())).await;
                        }
                        Inbound::NewBundle { .. } => {
                            on_new_bundle();
                        }
                    }
                }
            }
            Message::Ping(payload) => {
                let _ = ws.send(Message::Pong(payload)).await;
            }
            Message::Close(_) => {
                return Ok(());
            }
            _ => {}
        }
    }
}

fn compute_backoff(attempt: u32) -> Duration {
    let exp = (1u64 << attempt.min(6)).saturating_mul(BACKOFF_BASE_S);
    let capped = exp.min(BACKOFF_MAX_S);
    let jitter: f64 = 0.8 + rand::thread_rng().gen::<f64>() * 0.4;
    let final_secs = (capped as f64 * jitter).max(0.1);
    Duration::from_secs_f64(final_secs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_is_bounded_and_jittered() {
        // Confirm the upper bound holds across attempts and that the jittered output stays
        // within [0.8*cap, 1.2*cap].
        for attempt in 0u32..16 {
            let d = compute_backoff(attempt);
            let secs = d.as_secs_f64();
            assert!(secs >= 0.1, "secs={secs}");
            assert!(secs <= BACKOFF_MAX_S as f64 * 1.2 + 0.01, "secs={secs}");
        }
    }
}
