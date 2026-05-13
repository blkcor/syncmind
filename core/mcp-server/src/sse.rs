use std::collections::HashMap;
use std::convert::Infallible;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};

use axum::extract::{Path, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::{get, post};
use axum::Router;
use futures_core::Stream;
use tokio::sync::{mpsc, RwLock};

use crate::protocol::*;
use crate::server::McpServer;

type Sessions = Arc<RwLock<HashMap<u64, mpsc::Sender<Event>>>>;

#[derive(Clone)]
struct AppState {
    server: Arc<McpServer>,
    sessions: Sessions,
    counter: Arc<AtomicU64>,
}

/// Run the MCP server over SSE.
///
/// Binds an HTTP server to `bind_addr` exposing:
/// - `GET /sse`   – SSE stream for receiving server messages
/// - `POST /messages/:session_id` – endpoint for sending client messages
pub async fn run_sse_server(
    server: Arc<McpServer>,
    bind_addr: &str,
) -> anyhow::Result<()> {
    let sessions: Sessions = Arc::new(RwLock::new(HashMap::new()));
    let counter = Arc::new(AtomicU64::new(1));

    let state = AppState {
        server,
        sessions,
        counter,
    };

    let app = Router::new()
        .route("/sse", get(sse_handler))
        .route("/messages/:session_id", post(messages_handler))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(bind_addr).await?;
    tracing::info!(addr = %bind_addr, "MCP SSE server listening");
    axum::serve(listener, app).await?;
    Ok(())
}

// ── Handlers ────────────────────────────────────────────────────────────────

async fn sse_handler(State(state): State<AppState>) -> Sse<EventStream> {
    let session_id = state.counter.fetch_add(1, Ordering::SeqCst);
    let (tx, rx) = mpsc::channel::<Event>(32);
    state.sessions.write().await.insert(session_id, tx.clone());

    // Send the endpoint event so the client knows where to POST.
    let endpoint = format!("/messages/{}", session_id);
    let _ = tx
        .send(Event::default().event("endpoint").data(endpoint))
        .await;

    Sse::new(EventStream {
        rx,
        sessions: state.sessions.clone(),
        session_id,
    })
    .keep_alive(KeepAlive::new())
}

async fn messages_handler(
    State(state): State<AppState>,
    Path(session_id): Path<u64>,
    body: String,
) -> axum::http::StatusCode {
    match serde_json::from_str::<JsonRpcRequest>(&body) {
        Ok(req) => {
            if let Some(resp) = state.server.handle_request(req).await {
                let json = serde_json::to_string(&resp).unwrap_or_default();
                if let Some(tx) = state.sessions.read().await.get(&session_id) {
                    let event = Event::default().event("message").data(json);
                    if let Err(e) = tx.send(event).await {
                        tracing::warn!(session_id, error = %e, "failed to send SSE event");
                    }
                } else {
                    tracing::warn!(session_id, "SSE session not found");
                }
            }
            axum::http::StatusCode::ACCEPTED
        }
        Err(e) => {
            tracing::warn!(error = %e, "failed to parse JSON-RPC request in SSE message");
            axum::http::StatusCode::BAD_REQUEST
        }
    }
}

// ── Stream wrapper ──────────────────────────────────────────────────────────

struct EventStream {
    rx: mpsc::Receiver<Event>,
    sessions: Sessions,
    session_id: u64,
}

impl Stream for EventStream {
    type Item = Result<Event, Infallible>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.rx.poll_recv(cx).map(|opt| opt.map(Ok))
    }
}

impl Drop for EventStream {
    fn drop(&mut self) {
        let sessions = self.sessions.clone();
        let session_id = self.session_id;
        tokio::spawn(async move {
            sessions.write().await.remove(&session_id);
        });
    }
}
