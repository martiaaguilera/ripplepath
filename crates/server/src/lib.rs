//! Local HTTP API (`/api/v1`) plus the static web UI.
//!
//! The server is bound to one repository chosen at startup. Requests can choose revisions but never
//! a path: a browser tab (or anything else that can reach the port) must not be able to make
//! Ripplepath read arbitrary repositories on the machine.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::{HeaderValue, StatusCode, header};
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
}

struct AppState {
    config: ServerConfig,
    permits: Semaphore,
}

pub fn router(config: ServerConfig) -> Router {
    let web_dir = config.web_dir.clone();
    let state = Arc::new(AppState { config, permits: Semaphore::new(MAX_CONCURRENT_ANALYSES) });
    let api =
        Router::new().route("/api/v1/health", get(health)).route("/api/v1/analysis", get(analysis)).with_state(state);

    let app = match web_dir {
        Some(dir) => {
            let index = dir.join("index.html");
            api.fallback_service(ServeDir::new(dir).fallback(ServeFile::new(index)))
        }
        None => api,
    };
    // Source code from the analysed repository is rendered by the UI; a strict CSP means a
    // rendering bug cannot turn a crafted identifier into script execution.
    app.layer(SetResponseHeaderLayer::overriding(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; \
             connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'",
        ),
    ))
    .layer(SetResponseHeaderLayer::overriding(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff")))
    .layer(SetResponseHeaderLayer::overriding(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer")))
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
    let _permit = state.permits.acquire().await.map_err(|e| ApiError::Internal(e.to_string()))?;
    let options = AnalyzeOptions::new(&state.config.repo, base, head);
    let result = tokio::task::spawn_blocking(move || analyze(&options))
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
