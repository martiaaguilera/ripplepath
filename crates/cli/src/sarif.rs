//! SARIF 2.1.0 for findings with a source location (see [`crate::findings`]).
//!
//! Deterministic: rules in a fixed order, results sorted, object keys sorted by `serde_json`'s
//! default map. No timestamps, absolute paths or machine-specific values, so the same report always
//! yields the same bytes.

use serde_json::{Value, json};

use ripplepath_engine::AnalysisReport;

use crate::findings::{self, Level, Rule};

pub const SCHEMA_URI: &str =
    "https://docs.oasis-open.org/sarif/sarif/v2.1.0/errata01/os/schemas/sarif-schema-2.1.0.json";
const INFORMATION_URI: &str = "https://github.com/martiaaguilera/ripplepath";
const HELP_BASE: &str = "https://github.com/martiaaguilera/ripplepath/blob/main/docs/GITHUB_INTEGRATION.md";
/// Key under `partialFingerprints`. Versioned so that a future change of what is hashed starts a
/// new identity instead of silently re-keying existing alerts.
pub const FINGERPRINT_KEY: &str = "ripplepath/findingHash/v1";

fn level(level: Level) -> &'static str {
    match level {
        Level::Error => "error",
        Level::Warning => "warning",
        Level::Note => "note",
    }
}

/// Relative URI reference for a repository path: every byte outside RFC 3986 "unreserved" and `/`
/// is percent-encoded, so spaces, `#`, `?` or `%` in file names cannot change the URI's meaning.
pub fn path_to_uri(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'/') {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn rule_descriptor(rule: Rule) -> Value {
    let default_level = match rule {
        Rule::ConfigInvalid => "error",
        _ => "warning",
    };
    json!({
        "id": rule.id(),
        "name": rule.name(),
        "shortDescription": { "text": rule.short_description() },
        "fullDescription": { "text": rule.full_description() },
        "help": {
            "text": format!(
                "{} The result's level follows the merge-policy gate `{}` (error = FAIL, warning = WARN, note = not enforced).",
                rule.full_description(),
                rule.gate().name()
            )
        },
        "helpUri": format!("{HELP_BASE}#{}", rule.help_anchor()),
        "defaultConfiguration": { "level": default_level },
        "properties": { "tags": ["ripplepath", rule.gate().name()] },
    })
}

pub fn render(report: &AnalysisReport, path_prefix: &str) -> Result<String, String> {
    let rules: Vec<Value> = Rule::ALL.iter().map(|r| rule_descriptor(*r)).collect();
    let results: Vec<Value> = findings::collect(report, path_prefix)
        .into_iter()
        .map(|f| {
            let rule_index = Rule::ALL.iter().position(|r| *r == f.rule).unwrap_or(0);
            // Code scanning requires a region; a file-level finding (configuration errors) is
            // anchored at line 1, which the message and docs state.
            let line = f.line.unwrap_or(1);
            json!({
                "ruleId": f.rule.id(),
                "ruleIndex": rule_index,
                "level": level(f.level),
                "message": { "text": format!("{}: {}", f.title, f.message) },
                "locations": [{
                    "physicalLocation": {
                        "artifactLocation": { "uri": path_to_uri(&f.file) },
                        "region": { "startLine": line },
                    }
                }],
                "partialFingerprints": { FINGERPRINT_KEY: f.fingerprint },
                "properties": { "fileLevel": f.line.is_none() },
            })
        })
        .collect();
    let mut driver = json!({
        "name": "Ripplepath",
        "informationUri": INFORMATION_URI,
        "version": report.tool_version,
        "rules": rules,
    });
    // `semanticVersion` must be SemVer; the tool version is, but it is not checked here, so only
    // a plausible value is copied.
    if report.tool_version.split('.').count() == 3 {
        driver["semanticVersion"] = json!(report.tool_version);
    }
    // No `versionControlProvenance`: it requires a repository URI, which a local analysis does not
    // know; the uploader (codeql-action) records commit and ref itself.
    let run = json!({
        "tool": { "driver": driver },
        "columnKind": "unicodeCodePoints",
        "results": results,
    });
    let sarif = json!({
        "$schema": SCHEMA_URI,
        "version": "2.1.0",
        "runs": [run],
    });
    let mut out = serde_json::to_string_pretty(&sarif).map_err(|e| e.to_string())?;
    out.push('\n');
    Ok(out)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    /// Structural checks of the SARIF 2.1.0 constraints code scanning depends on. (The OASIS
    /// schema is not vendored; CI validates the self-test output against it.)
    #[test]
    fn demo_sarif_is_structurally_valid_stable_and_deterministic() {
        let report = crate::test_support::java_banking_report();
        let text = render(&report, "").unwrap();
        assert_eq!(text, render(&report, "").unwrap(), "byte-identical across runs");
        let sarif: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(sarif["version"], "2.1.0");
        assert_eq!(sarif["$schema"], SCHEMA_URI);
        let runs = sarif["runs"].as_array().unwrap();
        assert_eq!(runs.len(), 1);
        let driver = &runs[0]["tool"]["driver"];
        assert_eq!(driver["name"], "Ripplepath");
        let rule_ids: Vec<&str> =
            driver["rules"].as_array().unwrap().iter().map(|r| r["id"].as_str().unwrap()).collect();
        assert_eq!(rule_ids, Rule::ALL.map(Rule::id).to_vec());

        let results = runs[0]["results"].as_array().unwrap();
        let count = |id: &str| results.iter().filter(|r| r["ruleId"] == id).count();
        assert_eq!(count("ripplepath/new-architecture-violation"), 2);
        assert_eq!(count("ripplepath/breaking-api-change"), 3);
        let mut fingerprints = std::collections::BTreeSet::new();
        for result in results {
            let index = usize::try_from(result["ruleIndex"].as_u64().unwrap()).unwrap();
            assert_eq!(result["ruleId"].as_str().unwrap(), rule_ids[index]);
            assert!(["error", "warning", "note"].contains(&result["level"].as_str().unwrap()));
            assert!(!result["message"]["text"].as_str().unwrap().is_empty());
            let location = &result["locations"][0]["physicalLocation"];
            let uri = location["artifactLocation"]["uri"].as_str().unwrap();
            assert!(!uri.starts_with('/') && !uri.contains(':') && !uri.contains('\\'), "relative: {uri}");
            assert!(location["region"]["startLine"].as_u64().unwrap() >= 1);
            assert!(fingerprints.insert(result["partialFingerprints"][FINGERPRINT_KEY].as_str().unwrap().to_owned()));
        }
        // Levels follow the policy gates: new_architecture_violation FAILs, breaking_api_change WARNs.
        assert!(
            results
                .iter()
                .filter(|r| r["ruleId"] == "ripplepath/new-architecture-violation")
                .all(|r| r["level"] == "error")
        );
        assert!(
            results.iter().filter(|r| r["ruleId"] == "ripplepath/breaking-api-change").all(|r| r["level"] == "warning")
        );
    }

    #[test]
    fn fingerprints_ignore_line_moves_and_prefix_applies_to_uris() {
        let report = crate::test_support::java_banking_report();
        let mut moved = report.clone();
        for v in &mut moved.architecture.violations {
            v.edge.line += 10;
        }
        let prints = |r: &AnalysisReport| -> Vec<String> {
            findings::collect(r, "")
                .into_iter()
                .map(|f| f.fingerprint)
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect()
        };
        assert_eq!(prints(&report), prints(&moved));
        let sarif: Value = serde_json::from_str(&render(&report, "services/bank/").unwrap()).unwrap();
        let uri = sarif["runs"][0]["results"][0]["locations"][0]["physicalLocation"]["artifactLocation"]["uri"]
            .as_str()
            .unwrap();
        assert!(uri.starts_with("services/bank/src/"), "{uri}");
    }

    #[test]
    fn uris_are_relative_and_percent_encoded() {
        assert_eq!(path_to_uri("src/main/A.java"), "src/main/A.java");
        assert_eq!(path_to_uri("src/my dir/a#b?c%d.ts"), "src/my%20dir/a%23b%3Fc%25d.ts");
        assert_eq!(path_to_uri("src/ñ.ts"), "src/%C3%B1.ts");
    }
}
