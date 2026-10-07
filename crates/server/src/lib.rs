//! Local HTTP API (`/api/v1`) plus the static web UI.
//!
//! The server is bound to one repository chosen at startup. Requests can choose revisions but never
//! a path: a browser tab (or anything else that can reach the port) must not be able to make
//! Ripplepath read arbitrary repositories on the machine.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{Query, Request, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use ripplepath_engine::{AnalysisError, AnalyzeOptions, GitError, analyze};
use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::set_header::SetResponseHeaderLayer;

/// Analyses are CPU-bound and run on the blocking pool; this bounds how many run at once so a
/// burst of requests queues instead of oversubscribing the machine.
const MAX_CONCURRENT_ANALYSES: usize = 2;
const MAX_REVISION_LEN: usize = 256;

#[derive(Clone, Debug)]
pub struct ServerConfig {
    pub repo: PathBuf,
    pub addr: SocketAddr,
    /// Built web UI (`web/dist`). When absent only the API is served.
    pub web_dir: Option<PathBuf>,
    pub default_base: String,
    pub default_head: String,
    /// Extra `Host` header values to accept besides loopback names (e.g. a LAN hostname when
    /// deliberately binding beyond localhost). Compared without the port, case-insensitively.
    pub allowed_hosts: Vec<String>,
}

struct AppState {
    config: ServerConfig,
    permits: Arc<Semaphore>,
}

pub fn router(config: ServerConfig) -> Router {
    let web_dir = config.web_dir.clone();
    let state = Arc::new(AppState { config, permits: Arc::new(Semaphore::new(MAX_CONCURRENT_ANALYSES)) });
    let api = Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v1/analysis", get(analysis))
        .with_state(Arc::clone(&state));

    let app = match web_dir {
        Some(dir) => {
            let index = dir.join("index.html");
            api.fallback_service(ServeDir::new(dir).fallback(ServeFile::new(index)))
        }
        None => api,
    };
    // Source code from the analysed repository is rendered by the UI; a strict CSP means a
    // rendering bug cannot turn a crafted identifier into script execution.
    app.layer(middleware::from_fn_with_state(state, require_known_host))
        .layer(SetResponseHeaderLayer::overriding(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static(
                "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; \
             connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'",
            ),
        ))
        .layer(SetResponseHeaderLayer::overriding(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff")))
        .layer(SetResponseHeaderLayer::overriding(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer")))
}

/// Rejects requests whose `Host` is not a loopback name or an explicitly allowed host.
///
/// Binding to 127.0.0.1 does not stop a web page from reaching the server: with DNS rebinding,
/// `attacker.example` first resolves to the attacker and then to 127.0.0.1, and the browser treats
/// the API as same-origin for the attacker's page. The `Host` header still says
/// `attacker.example`, so checking it closes that hole.
async fn require_known_host(State(state): State<Arc<AppState>>, request: Request, next: Next) -> Response {
    let host = request.headers().get(header::HOST).and_then(|h| h.to_str().ok()).map(host_without_port);
    let allowed = match host {
        Some(host) => {
            matches!(host.to_ascii_lowercase().as_str(), "localhost" | "127.0.0.1" | "[::1]")
                || state.config.allowed_hosts.iter().any(|h| h.eq_ignore_ascii_case(host))
        }
        None => false,
    };
    if allowed {
        next.run(request).await
    } else {
        tracing::warn!(host = ?host, "rejected request with unexpected Host header");
        ApiError::Forbidden("unexpected Host header; start the server with --allow-host to permit it".to_owned())
            .into_response()
    }
}

fn host_without_port(host: &str) -> &str {
    if host.starts_with('[') {
        // IPv6 literal: `[::1]:7878`
        return host.find(']').map_or(host, |end| &host[..=end]);
    }
    match host.rsplit_once(':') {
        Some((name, port)) if port.chars().all(|c| c.is_ascii_digit()) => name,
        _ => host,
    }
}

pub async fn serve(config: ServerConfig) -> std::io::Result<()> {
    let addr = config.addr;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, repo = %config.repo.display(), "serving");
    axum::serve(listener, router(config))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
}

#[derive(Serialize)]
struct Health {
    status: &'static str,
    tool_version: &'static str,
    schema_version: u32,
    default_base: String,
    default_head: String,
}

async fn health(State(state): State<Arc<AppState>>) -> Json<Health> {
    Json(Health {
        status: "ok",
        tool_version: ripplepath_engine::TOOL_VERSION,
        schema_version: ripplepath_core::ANALYSIS_SCHEMA_VERSION,
        default_base: state.config.default_base.clone(),
        default_head: state.config.default_head.clone(),
    })
}

#[derive(Deserialize)]
struct AnalysisQuery {
    base: Option<String>,
    head: Option<String>,
}

#[derive(Debug, thiserror::Error)]
enum ApiError {
    #[error("{0}")]
    BadRequest(String),
    #[error("{0}")]
    Forbidden(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Internal(String),
}

#[derive(Serialize)]
struct ErrorBody<'a> {
    error: ErrorDetail<'a>,
}

#[derive(Serialize)]
struct ErrorDetail<'a> {
    code: &'a str,
    message: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code) = match &self {
            Self::BadRequest(_) => (StatusCode::BAD_REQUEST, "bad_request"),
            Self::Forbidden(_) => (StatusCode::FORBIDDEN, "forbidden"),
            Self::NotFound(_) => (StatusCode::NOT_FOUND, "not_found"),
            Self::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal"),
        };
        (status, Json(ErrorBody { error: ErrorDetail { code, message: self.to_string() } })).into_response()
    }
}

/// Revision specs go to gix's rev-parse, never a shell, so this is not about injection; it keeps
/// error messages sane and rejects input no legitimate revspec contains.
fn validate_revision(raw: &str) -> Result<&str, ApiError> {
    // Control characters are checked before trimming: `main\n` is a malformed request, not `main`.
    let trimmed = raw.trim_matches(' ');
    if trimmed.is_empty() || trimmed.len() > MAX_REVISION_LEN || trimmed.chars().any(char::is_control) {
        return Err(ApiError::BadRequest("revision must be 1-256 printable characters".to_owned()));
    }
    Ok(trimmed)
}

async fn analysis(
    State(state): State<Arc<AppState>>,
    Query(query): Query<AnalysisQuery>,
) -> Result<Response, ApiError> {
    let base = validate_revision(query.base.as_deref().unwrap_or(&state.config.default_base))?.to_owned();
    let head = validate_revision(query.head.as_deref().unwrap_or(&state.config.default_head))?.to_owned();
    let permit = Arc::clone(&state.permits).acquire_owned().await.map_err(|e| ApiError::Internal(e.to_string()))?;
    let options = AnalyzeOptions::new(&state.config.repo, base, head);
    // The permit moves into the blocking task. If it stayed in this future, a client that
    // disconnects would drop the future and release the permit while the analysis keeps running,
    // letting connect/disconnect loops start unbounded concurrent analyses.
    let result = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        analyze(&options)
    })
    .await
    .map_err(|e| ApiError::Internal(format!("analysis task failed: {e}")))?;
    match result {
        Ok(report) => Ok(Json(report).into_response()),
        Err(AnalysisError::Git(error @ GitError::RevisionNotFound { .. })) => {
            Err(ApiError::NotFound(error.to_string()))
        }
        Err(AnalysisError::Git(error @ GitError::NotATree { .. })) => Err(ApiError::BadRequest(error.to_string())),
        Err(other) => Err(ApiError::Internal(other.to_string())),
    }
}
