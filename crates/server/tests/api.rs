#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use ripplepath_engine::fixture::build_fixture_repo;
use ripplepath_server::{ServerConfig, router};
use tower::ServiceExt;

fn app(repo: &Path) -> axum::Router {
    app_with_hosts(repo, Vec::new())
}

fn app_with_hosts(repo: &Path, allowed_hosts: Vec<String>) -> axum::Router {
    router(ServerConfig {
        repo: repo.to_owned(),
        addr: "127.0.0.1:0".parse().unwrap(),
        web_dir: None,
        default_base: "main~1".to_owned(),
        default_head: "main".to_owned(),
        allowed_hosts,
        db: None,
    })
}

fn banking_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/java-banking");
    build_fixture_repo(&[&root.join("v1"), &root.join("v2")], dir.path()).unwrap();
    dir
}

async fn get(app: axum::Router, uri: &str) -> (StatusCode, axum::http::HeaderMap, serde_json::Value) {
    get_with_host(app, uri, Some("localhost:7878")).await
}

async fn get_with_host(
    app: axum::Router,
    uri: &str,
    host: Option<&str>,
) -> (StatusCode, axum::http::HeaderMap, serde_json::Value) {
    let mut request = Request::get(uri);
    if let Some(host) = host {
        request = request.header(header::HOST, host);
    }
    let response = app.oneshot(request.body(Body::empty()).unwrap()).await.unwrap();
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

#[tokio::test]
async fn dns_rebinding_hosts_are_rejected() {
    let repo = banking_repo();
    for host in [Some("attacker.example"), Some("attacker.example:7878"), Some("127.0.0.1.attacker.example"), None] {
        let (status, _, body) = get_with_host(app(repo.path()), "/api/v1/analysis", host).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{host:?}");
        assert_eq!(body["error"]["code"], "forbidden");
    }
    for host in ["localhost", "LOCALHOST:7878", "127.0.0.1:7878", "[::1]:7878"] {
        let (status, _, _) = get_with_host(app(repo.path()), "/api/v1/health", Some(host)).await;
        assert_eq!(status, StatusCode::OK, "{host}");
    }
}

const ACCOUNT: &str = "src/main/java/com/acme/bank/domain/Account.java";
const MIGRATION_V2: &str = "src/main/resources/db/migration/V2__account_frozen.sql";

#[tokio::test]
async fn file_returns_the_blob_of_the_requested_revision() {
    let repo = banking_repo();
    let (status, headers, head) = get(app(repo.path()), &format!("/api/v1/file?rev=main&path={ACCOUNT}")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(headers.get(header::CONTENT_SECURITY_POLICY).is_some());
    assert_eq!(head["path"], ACCOUNT);
    assert_eq!(head["content"]["kind"], "text");
    let head_text = head["content"]["text"].as_str().unwrap();
    assert!(head_text.contains("class Account"));
    assert_eq!(head["max_bytes"], 1024 * 1024);

    let (status, _, base) = get(app(repo.path()), &format!("/api/v1/file?rev=main~1&path={ACCOUNT}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_ne!(base["content"]["text"], head["content"]["text"], "base and head differ in the fixture");
    assert_ne!(base["blob"], head["blob"]);

    // Added in head: absent at base is a 404, not an empty file.
    let (status, _, body) = get(app(repo.path()), &format!("/api/v1/file?rev=main~1&path={MIGRATION_V2}")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "not_found");
}

#[tokio::test]
async fn file_rejects_paths_that_could_escape_the_tree() {
    let repo = banking_repo();
    for path in [
        "../../etc/passwd",
        "src/../../x",
        "/etc/passwd",
        "C:/Windows/win.ini",
        ".git/config",
        "src/.GIT/HEAD",
        "src%5C..%5C..%5Cx",
        "src//main",
        "./ripplepath.yml",
        "a%0Ab",
        "",
    ] {
        let (status, _, body) = get(app(repo.path()), &format!("/api/v1/file?rev=main&path={path}")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{path}");
        assert_eq!(body["error"]["code"], "bad_request", "{path}");
    }
    let (status, _, _) = get(app(repo.path()), "/api/v1/file?rev=main").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "path is required");
    let long = "a/".repeat(2100) + "b";
    let (status, _, _) = get(app(repo.path()), &format!("/api/v1/file?rev=main&path={long}")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "overlong path");
}

#[tokio::test]
async fn file_reports_unknown_revisions_directories_and_missing_files_as_404() {
    let repo = banking_repo();
    let (status, _, body) = get(app(repo.path()), &format!("/api/v1/file?rev=no-such-rev&path={ACCOUNT}")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body["error"]["message"].as_str().unwrap().contains("no-such-rev"));
    for path in [
        "src/main/java",
        "src/main/java/com/acme/bank/domain/Nope.java",
        "src/main/java/com/acme/bank/domain/Account.java/x",
    ] {
        let (status, _, _) = get(app(repo.path()), &format!("/api/v1/file?rev=main&path={path}")).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
    }
    let (status, _, _) = get(app(repo.path()), &format!("/api/v1/file?rev=main%0A&path={ACCOUNT}")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn file_does_not_ship_oversized_or_binary_blobs() {
    let snapshot = tempfile::tempdir().unwrap();
    std::fs::write(snapshot.path().join("big.txt"), "x".repeat(1024 * 1024 + 1)).unwrap();
    std::fs::write(snapshot.path().join("image.bin"), [0x89, b'P', b'N', b'G', 0, 1, 2]).unwrap();
    let repo = tempfile::tempdir().unwrap();
    build_fixture_repo(&[snapshot.path()], repo.path()).unwrap();

    let (status, _, body) = get(app(repo.path()), "/api/v1/file?rev=main&path=big.txt").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["content"]["kind"], "too_large");
    assert_eq!(body["content"]["size"], 1024 * 1024 + 1);
    assert!(body["content"].get("text").is_none());

    let (status, _, body) = get(app(repo.path()), "/api/v1/file?rev=main&path=image.bin").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["content"]["kind"], "binary");
}

#[tokio::test]
async fn file_endpoint_applies_the_host_check() {
    let repo = banking_repo();
    let uri = format!("/api/v1/file?rev=main&path={ACCOUNT}");
    for host in [Some("attacker.example"), Some("attacker.example:7878"), None] {
        let (status, _, body) = get_with_host(app(repo.path()), &uri, host).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{host:?}");
        assert!(body["content"].is_null(), "no source text leaks with the rejection");
    }
}

#[tokio::test]
async fn explicitly_allowed_hosts_are_accepted() {
    let repo = banking_repo();
    let app = app_with_hosts(repo.path(), vec!["devbox.lan".to_owned()]);
    let (status, _, _) = get_with_host(app, "/api/v1/health", Some("devbox.lan:7878")).await;
    assert_eq!(status, StatusCode::OK);
}
