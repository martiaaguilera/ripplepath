#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Protocol-level tests: JSON-RPC lines in, JSON-RPC lines out, against the java-banking fixture
//! (v1 → v2) built as a real Git repository.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use ripplepath_engine::fixture::build_fixture_repo;
use ripplepath_mcp::{MAX_MESSAGE_BYTES, McpConfig, TOOL_NAMES, serve};
use serde_json::{Value, json};

const MONEY_MINUS: &str = "java:com.acme.bank.domain.Money#minus(Money)";
const WITHDRAW: &str = "java:com.acme.bank.domain.Account#withdraw(Money)";
const TRANSFER: &str = "java:com.acme.bank.application.TransferService#transfer(String,String,Money)";

fn repo() -> &'static Path {
    static REPO: OnceLock<(tempfile::TempDir, PathBuf)> = OnceLock::new();
    &REPO
        .get_or_init(|| {
            let dir = tempfile::tempdir().unwrap();
            let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/java-banking");
            build_fixture_repo(&[&root.join("v1"), &root.join("v2")], dir.path()).unwrap();
            let path = dir.path().to_owned();
            (dir, path)
        })
        .1
}

fn config() -> McpConfig {
    McpConfig { repo: repo().to_owned(), db: None, default_base: "main~1".into(), default_head: "main".into() }
}

/// Runs one session over in-memory pipes and returns every response line, parsed.
fn session(lines: &[String]) -> Vec<Value> {
    let mut input = lines.join("\n");
    input.push('\n');
    let mut output = Vec::new();
    serve(config(), input.as_bytes(), &mut output).unwrap();
    let text = String::from_utf8(output).unwrap();
    text.lines().map(|l| serde_json::from_str(l).expect("every output line is one JSON message")).collect()
}

fn initialize() -> String {
    json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}).to_string()
}

fn initialized() -> String {
    json!({"jsonrpc":"2.0","method":"notifications/initialized"}).to_string()
}

fn call(id: u64, name: &str, arguments: Value) -> String {
    json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":arguments}}).to_string()
}

/// Calls one tool in a fresh legacy session and returns its result object.
fn tool(name: &str, arguments: Value) -> Value {
    let responses = session(&[initialize(), initialized(), call(1, name, arguments)]);
    assert_eq!(responses.len(), 2, "{responses:?}");
    let result = responses[1]["result"].clone();
    assert!(!result.is_null(), "{:?}", responses[1]);
    result
}

fn structured(name: &str, arguments: Value) -> Value {
    let result = tool(name, arguments);
    assert_eq!(result["isError"], json!(false), "{result}");
    // The text block mirrors the structured content for clients that only read text.
    let text: Value = serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(text, result["structuredContent"]);
    result["structuredContent"].clone()
}

fn ids(list: &Value) -> Vec<&str> {
    list.as_array().unwrap().iter().map(|v| v["id"].as_str().unwrap()).collect()
}

#[test]
fn legacy_handshake_lists_read_only_tools_with_schemas() {
    let responses = session(&[
        initialize(),
        initialized(),
        json!({"jsonrpc":"2.0","id":"p","method":"ping"}).to_string(),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}).to_string(),
    ]);
    assert_eq!(responses.len(), 3, "notifications get no response");
    let init = &responses[0]["result"];
    assert_eq!(init["protocolVersion"], "2025-11-25");
    assert_eq!(init["serverInfo"]["name"], "ripplepath");
    assert!(init["capabilities"]["tools"].is_object());
    assert_eq!(responses[1], json!({"jsonrpc":"2.0","id":"p","result":{}}));

    let tools = responses[2]["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(names, TOOL_NAMES);
    for tool in tools {
        assert_eq!(tool["inputSchema"]["type"], "object");
        assert_eq!(tool["inputSchema"]["additionalProperties"], false);
        assert_eq!(tool["annotations"]["readOnlyHint"], true);
        assert_eq!(tool["annotations"]["destructiveHint"], false);
    }
    let what_if = tools.iter().find(|t| t["name"] == "what_if").unwrap();
    assert_eq!(what_if["inputSchema"]["required"], json!(["symbol"]));
}

#[test]
fn unknown_legacy_version_is_answered_with_the_newest_supported() {
    let request = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2023-01-01","capabilities":{}}});
    let responses = session(&[request.to_string()]);
    assert_eq!(responses[0]["result"]["protocolVersion"], "2025-11-25");
}

#[test]
fn modern_requests_are_stateless_and_carry_result_type() {
    let meta =
        json!({"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}});
    let responses = session(&[
        json!({"jsonrpc":"2.0","id":1,"method":"server/discover","params":{"_meta":meta}}).to_string(),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{"_meta":meta}}).to_string(),
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"_meta":meta,"name":"find_symbols","arguments":{"query":"Money#minus"}}}).to_string(),
        json!({"jsonrpc":"2.0","id":4,"method":"tools/list","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"1900-01-01","io.modelcontextprotocol/clientCapabilities":{}}}}).to_string(),
        json!({"jsonrpc":"2.0","id":5,"method":"tools/list","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28"}}}).to_string(),
    ]);
    let discover = &responses[0]["result"];
    assert_eq!(discover["resultType"], "complete");
    assert!(discover["supportedVersions"].as_array().unwrap().contains(&json!("2026-07-28")));
    assert_eq!(discover["_meta"]["io.modelcontextprotocol/serverInfo"]["name"], "ripplepath");
    assert_eq!(responses[1]["result"]["tools"].as_array().unwrap().len(), TOOL_NAMES.len());
    assert_eq!(responses[2]["result"]["resultType"], "complete");
    assert_eq!(ids(&responses[2]["result"]["structuredContent"]["symbols"]), vec![MONEY_MINUS]);
    assert_eq!(responses[3]["error"]["code"], -32022);
    assert_eq!(responses[3]["error"]["data"]["requested"], "1900-01-01");
    assert_eq!(responses[4]["error"]["code"], -32602, "clientCapabilities is required");
}

#[test]
fn protocol_errors_use_json_rpc_codes() {
    let oversized =
        format!("{{\"jsonrpc\":\"2.0\",\"id\":9,\"method\":\"ping\",\"pad\":\"{}\"}}", "x".repeat(MAX_MESSAGE_BYTES));
    let responses = session(&[
        json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}).to_string(),
        "{not json".to_owned(),
        "[]".to_owned(),
        oversized,
        initialize(),
        json!({"jsonrpc":"2.0","id":2,"method":"resources/list"}).to_string(),
        call(3, "run_shell", json!({})),
        json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"arguments":{}}}).to_string(),
        json!({"jsonrpc":"1.0","id":5,"method":"ping"}).to_string(),
        json!({"jsonrpc":"2.0","id":null,"method":"ping"}).to_string(),
        json!({"jsonrpc":"2.0","id":6,"method":"ping"}).to_string(),
    ]);
    let codes: Vec<(Value, Value)> = responses.iter().map(|r| (r["id"].clone(), r["error"]["code"].clone())).collect();
    assert_eq!(
        codes,
        vec![
            (json!(1), json!(-32600)), // tools/list before initialize
            (Value::Null, json!(-32700)),
            (Value::Null, json!(-32600)),
            (Value::Null, json!(-32600)), // oversized; the stream stays in sync afterwards
            (json!(0), Value::Null),
            (json!(2), json!(-32601)),
            (json!(3), json!(-32602)),
            (json!(4), json!(-32602)),
            (json!(5), json!(-32600)),
            (Value::Null, json!(-32600)),
            (json!(6), Value::Null),
        ]
    );
    assert!(responses[6]["error"]["message"].as_str().unwrap().contains("Unknown tool: run_shell"));
}

#[test]
fn invalid_arguments_and_revisions_are_tool_errors_the_model_can_read() {
    let cases = [
        ("what_if", json!({})),
        ("what_if", json!({"symbol": MONEY_MINUS, "depth": 3})),
        ("what_if", json!({"symbol": MONEY_MINUS, "max_depth": 99})),
        ("what_if", json!({"symbol": MONEY_MINUS, "rev": "main\nHEAD"})),
        ("what_if", json!({"symbol": MONEY_MINUS, "rev": "no-such-branch"})),
        ("analyze_change", json!({"base": "", "head": "main"})),
        ("analyze_change", json!({"mode": "yolo"})),
        ("architecture_status", json!({"rev": "main", "base": "main~1"})),
    ];
    for (name, arguments) in cases {
        let result = tool(name, arguments.clone());
        assert_eq!(result["isError"], json!(true), "{name} {arguments} → {result}");
        assert!(!result["content"][0]["text"].as_str().unwrap().is_empty());
    }
    let missing = tool("what_if", json!({"symbol": "java:com.acme.bank.domain.Money#minuss(Money)"}));
    let message = missing["content"][0]["text"].as_str().unwrap();
    assert!(message.contains("not found") && message.contains(MONEY_MINUS), "{message}");
}

#[test]
fn what_if_on_money_minus_reaches_withdraw_and_transfer_with_evidence() {
    let out = structured("what_if", json!({"symbol": MONEY_MINUS}));
    assert_eq!(out["root"]["id"], MONEY_MINUS);
    assert_eq!(out["root"]["layer"], "domain");
    let impacted = out["impacted_symbols"].as_array().unwrap();
    let find = |id: &str| impacted.iter().find(|i| i["id"] == id).unwrap_or_else(|| panic!("{id} in {out}"));

    let withdraw = find(WITHDRAW);
    assert_eq!(withdraw["depth"], 1);
    let hop = &withdraw["path"][0];
    assert_eq!(hop["symbol"], WITHDRAW);
    assert_eq!(hop["edge"]["from"], WITHDRAW);
    assert_eq!(hop["edge"]["to"], MONEY_MINUS);
    assert_eq!(hop["edge"]["kind"], "CALLS");
    assert!(hop["edge"]["evidence"].is_string());
    assert!(hop["edge"]["file"].as_str().unwrap().ends_with("domain/Account.java"));
    assert!(hop["edge"]["line"].as_u64().unwrap() > 0);

    let transfer = find(TRANSFER);
    assert_eq!(transfer["depth"], 2);
    let hops: Vec<&str> = transfer["path"].as_array().unwrap().iter().map(|h| h["symbol"].as_str().unwrap()).collect();
    assert_eq!(hops, vec![WITHDRAW, TRANSFER]);

    let layers = out["summary"]["layers"].as_array().unwrap();
    assert!(layers.contains(&json!("domain")) && layers.contains(&json!("application")), "{layers:?}");
    let tests = out["tests"].as_array().unwrap();
    assert!(!tests.is_empty(), "tests reach Money#minus through withdraw/transfer");
    assert!(tests.iter().all(|t| t["tier"].is_string() && t["reason"] == "STATIC_PATH" && t["path"].is_array()));
    assert!(out["note"].as_str().unwrap().starts_with("Hypothetical"));

    // Deterministic: the same call answers byte-identically.
    assert_eq!(out, structured("what_if", json!({"symbol": MONEY_MINUS})));
}

#[test]
fn dependency_path_explains_each_hop_and_falls_back_to_reverse() {
    let forward = structured("dependency_path", json!({"from": TRANSFER, "to": MONEY_MINUS}));
    assert_eq!(forward["found"], true);
    assert_eq!(forward["direction"], "forward");
    let hops = forward["hops"].as_array().unwrap();
    assert_eq!(hops.len(), 2);
    assert_eq!(hops[0]["edge"]["from"], TRANSFER);
    assert_eq!(hops[0]["edge"]["to"], WITHDRAW);
    assert_eq!(hops[1]["edge"]["to"], MONEY_MINUS);
    assert!(hops.iter().all(|h| h["edge"]["file"].is_string() && h["edge"]["rule"].is_string()));

    let reverse = structured("dependency_path", json!({"from": MONEY_MINUS, "to": TRANSFER}));
    assert_eq!(reverse["direction"], "reverse");
    assert_eq!(reverse["hops"].as_array().unwrap().len(), 2);

    let none = structured(
        "dependency_path",
        json!({"from": MONEY_MINUS, "to": "java:com.acme.bank.domain.Money#isNegative()"}),
    );
    assert_eq!(none["found"], false);
    assert!(none["note"].as_str().unwrap().contains("not proof"));
}

#[test]
fn symbol_info_lists_edges_layer_tests_and_violations() {
    let info = structured("symbol_info", json!({"symbol": WITHDRAW}));
    assert_eq!(info["symbol"]["kind"], "method");
    assert_eq!(info["layer"], "domain");
    let incoming = info["incoming"].as_array().unwrap();
    assert!(incoming.iter().any(|e| e["from"] == TRANSFER && e["kind"] == "CALLS"));
    let outgoing = info["outgoing"].as_array().unwrap();
    assert!(outgoing.iter().any(|e| e["to"] == MONEY_MINUS));
    assert!(!info["tests"].as_array().unwrap().is_empty());

    // v2's Account depends on api.ApiErrors: a domain → api violation in this revision.
    let account = structured("symbol_info", json!({"symbol": WITHDRAW, "rev": "main"}));
    let violations = account["architecture_violations"].as_array().unwrap();
    assert!(violations.iter().any(|v| v["from_layer"] == "domain" && v["to_layer"] == "api"), "{account}");
    let before = structured("symbol_info", json!({"symbol": WITHDRAW, "rev": "main~1"}));
    assert!(before["architecture_violations"].as_array().unwrap().is_empty());
}

#[test]
fn change_tools_summarise_the_fixture_change() {
    let summary = structured("analyze_change", json!({}));
    assert_eq!(summary["base"]["spec"], "main~1");
    assert!(summary["risk"]["interpretation"].as_str().unwrap().contains("NOT a probability"));
    assert!(ids(&summary["changed_symbols"]).contains(&WITHDRAW));
    let new = summary["architecture"]["new_violations"].as_array().unwrap();
    assert_eq!(new.len(), 2);
    assert!(new.iter().all(|v| v["status"] == "NEW" && v["edge"]["file"].is_string()));
    assert!(summary["impacted_symbols"].as_array().unwrap().iter().all(|i| i["path"].is_array()));

    let impacted = structured("impacted_symbols", json!({"limit": 1}));
    assert_eq!(impacted["impacted_symbols"].as_array().unwrap().len(), 1);
    assert!(impacted["truncation"][0].as_str().unwrap().starts_with("impacted_symbols: showing 1 of"));

    let tests = structured("tests_for_change", json!({"mode": "conservative"}));
    assert_eq!(tests["test_selection"]["mode"], "CONSERVATIVE");
    assert!(!tests["tests"].as_array().unwrap().is_empty());

    let arch = structured("architecture_status", json!({"base": "main~1", "head": "main"}));
    assert_eq!(arch["summary"]["new_violations"], 2);
    let state = structured("architecture_status", json!({"rev": "main~1"}));
    assert_eq!(state["violations_total"], 0);
}
