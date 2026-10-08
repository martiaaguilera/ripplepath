//! JSON-RPC 2.0 framing and MCP method dispatch for the stdio transport.
//!
//! Dual-era (docs/adr/0007-read-only-mcp.md): a request whose `params._meta` carries
//! `io.modelcontextprotocol/protocolVersion` is served statelessly under the 2026-07-28 revision;
//! anything else follows the handshake-based revisions (`initialize` first). Both are a small,
//! fixed set of methods; everything else is `Method not found`.

use std::io::{BufRead, Write};

use serde_json::{Map, Value, json};

use crate::McpError;
use crate::tools::{CallOutcome, Tools};

/// Largest accepted message. Tool arguments are a few short strings; anything near this size is a
/// broken or hostile client, and reading it unbounded would let it exhaust memory.
pub const MAX_MESSAGE_BYTES: usize = 1024 * 1024;
/// Per-request-metadata revisions served statelessly.
pub const MODERN_VERSIONS: &[&str] = &["2026-07-28"];
/// Handshake revisions accepted in `initialize`, newest first.
pub const LEGACY_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;
const INTERNAL_ERROR: i64 = -32603;
const UNSUPPORTED_PROTOCOL_VERSION: i64 = -32022;

const META_VERSION: &str = "io.modelcontextprotocol/protocolVersion";
const META_CAPABILITIES: &str = "io.modelcontextprotocol/clientCapabilities";
const META_SERVER_INFO: &str = "io.modelcontextprotocol/serverInfo";

const INSTRUCTIONS: &str = "Ripplepath answers questions about one Git repository, fixed when this server started: \
what a change touches, what depends on a symbol, which tests have evidence of exercising it, and whether a change \
breaks configured architecture rules. Answers come from a deterministic static dependency graph plus ingested \
coverage/CI evidence; every edge carries its kind, evidence class and file:line. Only committed revisions are \
visible (no working-tree edits). Use find_symbols to get exact symbol ids, then symbol_info, what_if or \
dependency_path. Read the `uncertainty` field: unresolved references, parse failures and truncation mean the \
answer is a lower bound.";

enum Frame {
    Message(Vec<u8>),
    Oversized(usize),
    Eof,
}

/// Reads one newline-terminated message, never buffering more than [`MAX_MESSAGE_BYTES`].
fn read_frame(reader: &mut impl BufRead, buf: &mut Vec<u8>) -> std::io::Result<Frame> {
    buf.clear();
    let mut total = 0usize;
    let mut oversized = false;
    let mut started = false;
    loop {
        let (take, newline) = {
            let chunk = reader.fill_buf()?;
            if chunk.is_empty() {
                return Ok(match (started, oversized) {
                    (false, _) => Frame::Eof,
                    (true, true) => Frame::Oversized(total),
                    (true, false) => Frame::Message(std::mem::take(buf)),
                });
            }
            started = true;
            let (take, newline) = match chunk.iter().position(|&b| b == b'\n') {
                Some(i) => (i, true),
                None => (chunk.len(), false),
            };
            total = total.saturating_add(take);
            if !oversized {
                if buf.len() + take > MAX_MESSAGE_BYTES {
                    // Keep consuming to the newline so the stream stays in sync, but drop the bytes.
                    oversized = true;
                    buf.clear();
                } else {
                    buf.extend_from_slice(&chunk[..take]);
                }
            }
            (take, newline)
        };
        reader.consume(take + usize::from(newline));
        if newline {
            if oversized {
                return Ok(Frame::Oversized(total));
            }
            if buf.last() == Some(&b'\r') {
                buf.pop();
            }
            return Ok(Frame::Message(std::mem::take(buf)));
        }
    }
}

pub(crate) fn run(mut tools: Tools, mut input: impl BufRead, mut output: impl Write) -> Result<(), McpError> {
    let mut session = Session::default();
    let mut buf = Vec::new();
    loop {
        let response = match read_frame(&mut input, &mut buf)? {
            Frame::Eof => return Ok(()),
            Frame::Oversized(size) => Some(error(
                Value::Null,
                INVALID_REQUEST,
                format!("message is {size} bytes, above the {MAX_MESSAGE_BYTES}-byte limit"),
                None,
            )),
            Frame::Message(bytes) if bytes.iter().all(u8::is_ascii_whitespace) => None,
            Frame::Message(bytes) => session.handle(&mut tools, &bytes),
        };
        if let Some(response) = response {
            // serde_json escapes control characters, so a serialized message never contains a raw
            // newline and stays one frame.
            let line = serde_json::to_string(&response).unwrap_or_else(|e| {
                serde_json::to_string(&error(Value::Null, INTERNAL_ERROR, e.to_string(), None))
                    .unwrap_or_else(|_| String::from("{}"))
            });
            output.write_all(line.as_bytes())?;
            output.write_all(b"\n")?;
            output.flush()?;
        }
    }
}

fn error(id: Value, code: i64, message: String, data: Option<Value>) -> Value {
    let mut err = json!({ "code": code, "message": message });
    if let (Some(data), Some(map)) = (data, err.as_object_mut()) {
        map.insert("data".to_owned(), data);
    }
    json!({ "jsonrpc": "2.0", "id": id, "error": err })
}

fn success(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn server_info() -> Value {
    json!({ "name": "ripplepath", "title": "Ripplepath", "version": ripplepath_engine::TOOL_VERSION })
}

fn all_versions() -> Vec<&'static str> {
    MODERN_VERSIONS.iter().chain(LEGACY_VERSIONS).copied().collect()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Era {
    Modern,
    Legacy,
}

/// Legacy-era connection state; modern requests never read it.
#[derive(Default)]
struct Session {
    /// The revision agreed in `initialize`.
    legacy_version: Option<&'static str>,
}

impl Session {
    fn handle(&mut self, tools: &mut Tools, bytes: &[u8]) -> Option<Value> {
        let message: Value = match serde_json::from_slice(bytes) {
            Ok(value) => value,
            Err(e) => return Some(error(Value::Null, PARSE_ERROR, format!("parse error: {e}"), None)),
        };
        let Value::Object(message) = message else {
            let what = if message.is_array() { "batches are not supported" } else { "message must be an object" };
            return Some(error(Value::Null, INVALID_REQUEST, format!("invalid request: {what}"), None));
        };
        let id = message.get("id").cloned();
        let valid_id = match &id {
            Some(Value::String(_)) => true,
            Some(Value::Number(n)) => n.is_i64() || n.is_u64(),
            _ => false,
        };
        let reply_id = if valid_id { id.clone().unwrap_or(Value::Null) } else { Value::Null };
        if message.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
            return Some(error(reply_id, INVALID_REQUEST, "invalid request: jsonrpc must be \"2.0\"".to_owned(), None));
        }
        let Some(method) = message.get("method") else {
            // A response to a request we never send (this server issues no requests): ignore it.
            return None;
        };
        let Some(method) = method.as_str() else {
            return Some(error(reply_id, INVALID_REQUEST, "invalid request: method must be a string".to_owned(), None));
        };
        if id.is_none() {
            self.notification(method);
            return None;
        }
        if !valid_id {
            return Some(error(
                Value::Null,
                INVALID_REQUEST,
                "invalid request: id must be a string or an integer".to_owned(),
                None,
            ));
        }
        let params = match message.get("params") {
            None => Map::new(),
            Some(Value::Object(map)) => map.clone(),
            Some(_) => return Some(error(reply_id, INVALID_PARAMS, "params must be an object".to_owned(), None)),
        };
        Some(self.request(tools, reply_id, method, &params))
    }

    fn notification(&mut self, method: &str) {
        // `notifications/initialized` needs no action: requests are accepted once `initialize` has
        // been answered. Cancellation cannot interrupt a sequential server; other notifications
        // carry nothing this server uses.
        tracing::debug!(method, "notification");
    }

    fn request(&mut self, tools: &mut Tools, id: Value, method: &str, params: &Map<String, Value>) -> Value {
        let era = match self.era(method, params) {
            Ok(era) => era,
            Err((code, message, data)) => return error(id, code, message, data),
        };
        let result = match (era, method) {
            (Era::Legacy, "initialize") => Ok(self.initialize(params)),
            (_, "ping") => Ok(json!({})),
            (Era::Modern, "server/discover") => Ok(json!({
                "supportedVersions": all_versions(),
                "capabilities": { "tools": { "listChanged": false } },
                "instructions": INSTRUCTIONS,
            })),
            (Era::Legacy, "server/discover") => Err((
                INVALID_PARAMS,
                format!("missing _meta[\"{META_VERSION}\"]; this server supports {}", all_versions().join(", ")),
                None,
            )),
            (Era::Legacy, "tools/list" | "tools/call") if self.legacy_version.is_none() => Err((
                INVALID_REQUEST,
                "server not initialized: send `initialize` first, or use per-request _meta (2026-07-28)".to_owned(),
                None,
            )),
            (_, "tools/list") => Ok(json!({ "tools": tools.definitions() })),
            (_, "tools/call") => Self::call(tools, params),
            (_, other) => Err((METHOD_NOT_FOUND, format!("method not found: {other}"), None)),
        };
        match result {
            Ok(mut result) => {
                if era == Era::Modern
                    && let Some(map) = result.as_object_mut()
                {
                    map.insert("resultType".to_owned(), json!("complete"));
                    map.insert("_meta".to_owned(), json!({ META_SERVER_INFO: server_info() }));
                }
                success(id, result)
            }
            Err((code, message, data)) => error(id, code, message, data),
        }
    }

    /// Classifies a request and validates modern per-request metadata.
    fn era(&self, method: &str, params: &Map<String, Value>) -> Result<Era, (i64, String, Option<Value>)> {
        if method == "initialize" {
            return Ok(Era::Legacy);
        }
        let meta = params.get("_meta").and_then(Value::as_object);
        let Some(version) = meta.and_then(|m| m.get(META_VERSION)) else {
            return Ok(Era::Legacy);
        };
        let Some(version) = version.as_str() else {
            return Err((INVALID_PARAMS, format!("_meta[\"{META_VERSION}\"] must be a string"), None));
        };
        if !MODERN_VERSIONS.contains(&version) {
            return Err((
                UNSUPPORTED_PROTOCOL_VERSION,
                "Unsupported protocol version".to_owned(),
                Some(json!({ "supported": all_versions(), "requested": version })),
            ));
        }
        if !meta.and_then(|m| m.get(META_CAPABILITIES)).is_some_and(Value::is_object) {
            return Err((INVALID_PARAMS, format!("missing required _meta[\"{META_CAPABILITIES}\"]"), None));
        }
        Ok(Era::Modern)
    }

    fn initialize(&mut self, params: &Map<String, Value>) -> Value {
        let requested = params.get("protocolVersion").and_then(Value::as_str);
        // Echo a supported requested version; otherwise offer our newest and let the client decide.
        let version = LEGACY_VERSIONS.iter().copied().find(|v| Some(*v) == requested).unwrap_or(LEGACY_VERSIONS[0]);
        self.legacy_version = Some(version);
        json!({
            "protocolVersion": version,
            "capabilities": { "tools": { "listChanged": false } },
            "serverInfo": server_info(),
            "instructions": INSTRUCTIONS,
        })
    }

    fn call(tools: &mut Tools, params: &Map<String, Value>) -> Result<Value, (i64, String, Option<Value>)> {
        let Some(name) = params.get("name").and_then(Value::as_str) else {
            return Err((INVALID_PARAMS, "tools/call requires a string `name`".to_owned(), None));
        };
        let arguments = match params.get("arguments") {
            None | Some(Value::Null) => Map::new(),
            Some(Value::Object(map)) => map.clone(),
            Some(_) => return Err((INVALID_PARAMS, "tools/call `arguments` must be an object".to_owned(), None)),
        };
        match tools.call(name, &arguments) {
            CallOutcome::UnknownTool => Err((INVALID_PARAMS, format!("Unknown tool: {name}"), None)),
            CallOutcome::Done(Ok(structured)) => {
                let text = serde_json::to_string(&structured).unwrap_or_default();
                if text.len() > crate::tools::MAX_RESULT_BYTES {
                    return Ok(tool_error(format!(
                        "result is {} bytes, above the {}-byte limit; narrow the query (smaller `limit` or \
                         `max_depth`)",
                        text.len(),
                        crate::tools::MAX_RESULT_BYTES
                    )));
                }
                Ok(json!({
                    "content": [{ "type": "text", "text": text }],
                    "structuredContent": structured,
                    "isError": false,
                }))
            }
            CallOutcome::Done(Err(message)) => Ok(tool_error(message)),
        }
    }
}

/// A tool execution error: reported in the result so the model can read it and correct its call.
fn tool_error(message: String) -> Value {
    json!({ "content": [{ "type": "text", "text": message }], "isError": true })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frames(input: &[u8]) -> Vec<Result<Vec<u8>, usize>> {
        let mut reader = std::io::BufReader::with_capacity(7, input);
        let mut buf = Vec::new();
        let mut out = Vec::new();
        loop {
            match read_frame(&mut reader, &mut buf).unwrap() {
                Frame::Eof => return out,
                Frame::Message(m) => out.push(Ok(m)),
                Frame::Oversized(n) => out.push(Err(n)),
            }
        }
    }

    #[test]
    fn splits_lines_across_small_reads_and_strips_cr() {
        let got = frames(b"{\"a\":1}\r\n{\"b\":22}\nlast");
        assert_eq!(got, vec![Ok(b"{\"a\":1}".to_vec()), Ok(b"{\"b\":22}".to_vec()), Ok(b"last".to_vec())]);
    }

    #[test]
    fn oversized_lines_are_dropped_and_the_stream_resyncs() {
        let mut input = vec![b'x'; MAX_MESSAGE_BYTES + 10];
        input.extend_from_slice(b"\n{}\n");
        let got = frames(&input);
        assert_eq!(got, vec![Err(MAX_MESSAGE_BYTES + 10), Ok(b"{}".to_vec())]);
    }
}
