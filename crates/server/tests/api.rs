#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use ripplepath_engine::fixture::build_fixture_repo;
use ripplepath_server::{ServerConfig, router};
use tower::ServiceExt;

fn app(repo: &Path) -> axum::Router {
    router(ServerConfig {
        repo: repo.to_owned(),
        addr: "127.0.0.1:0".parse().unwrap(),
        web_dir: None,
        default_base: "main~1".to_owned(),
        default_head: "main".to_owned(),
    })
}

fn banking_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/java-banking");
    build_fixture_repo(&[&root.join("v1"), &root.join("v2")], dir.path()).unwrap();
    dir
}

async fn get(app: axum::Router, uri: &str) -> (StatusCode, axum::http::HeaderMap, serde_json::Value) {
    let response = app.oneshot(Request::get(uri).body(Body::empty()).unwrap()).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, headers, serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null))
}

#[tokio::test]
async fn health_reports_versions_and_security_headers() {
    let repo = banking_repo();
    let (status, headers, body) = get(app(repo.path()), "/api/v1/health").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ok");
    assert_eq!(body["schema_version"], 1);
    let csp = headers.get(header::CONTENT_SECURITY_POLICY).unwrap().to_str().unwrap();
    assert!(csp.contains("default-src 'self'"));
    assert_eq!(headers.get(header::X_CONTENT_TYPE_OPTIONS).unwrap(), "nosniff");
}

#[tokio::test]
async fn analysis_uses_defaults_and_returns_the_report() {
    let repo = banking_repo();
    let (status, _, body) = get(app(repo.path()), "/api/v1/analysis").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["schema_version"], 1);
    assert_eq!(body["base"]["spec"], "main~1");
    assert!(body["summary"]["symbols_changed"].as_u64().unwrap() > 0);
}

#[tokio::test]
async fn unknown_revision_is_a_structured_404() {
    let repo = banking_repo();
    let (status, _, body) = get(app(repo.path()), "/api/v1/analysis?base=does-not-exist&head=main").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "not_found");
    assert!(body["error"]["message"].as_str().unwrap().contains("does-not-exist"));
}

#[tokio::test]
async fn control_characters_in_revisions_are_rejected() {
    let repo = banking_repo();
    let (status, _, body) = get(app(repo.path()), "/api/v1/analysis?base=main%0A&head=main").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "bad_request");
}
