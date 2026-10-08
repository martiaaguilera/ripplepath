//! JUnit XML (`testsuites > testsuite > testcase`), as written by Surefire, Gradle, the JUnit
//! Platform console launcher, Jest/Vitest reporters and most CI tools.

use serde::{Deserialize, Serialize};

use crate::EvidenceError;
use crate::xml::{Node, walk};

const FORMAT: &str = "JUnit XML";
const MAX_FINGERPRINT: usize = 200;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Outcome {
    Passed,
    Failed,
    Error,
    Skipped,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestCaseResult {
    pub classname: String,
    pub name: String,
    /// Some reporters (Vitest, Jest) record the source file.
    pub file: Option<String>,
    pub duration_ms: u64,
    pub outcome: Outcome,
    /// Surefire `flakyFailure`/`flakyError`: failed attempts before a final pass in the same run.
    pub failed_attempts: u32,
    /// Exception type plus the first line of the message, truncated: groups failures with the same
    /// cause without storing whole stack traces (which may contain secrets or source).
    pub failure_fingerprint: Option<String>,
}

fn seconds_to_ms(raw: Option<&String>) -> u64 {
    raw.and_then(|t| t.trim().parse::<f64>().ok())
        .filter(|t| t.is_finite() && *t >= 0.0)
        .map_or(0, |t| (t * 1000.0).round().min(u64::MAX as f64) as u64)
}

fn fingerprint(kind: Option<&String>, message: Option<&String>) -> String {
    let first_line = message.and_then(|m| m.lines().next()).unwrap_or("");
    let text = match kind {
        Some(k) if !k.is_empty() => format!("{k}: {first_line}"),
        _ => first_line.to_owned(),
    };
    text.chars().take(MAX_FINGERPRINT).collect()
}

pub fn parse_junit(input: &str) -> Result<Vec<TestCaseResult>, EvidenceError> {
    let mut results = Vec::new();
    let mut current: Option<TestCaseResult> = None;
    let mut saw_root = false;
    walk(input, FORMAT, |node| {
        match node {
            Node::Open { name, attrs } => match name.as_str() {
                "testsuites" | "testsuite" => saw_root = true,
                "testcase" => {
                    current = Some(TestCaseResult {
                        classname: attrs.get("classname").cloned().unwrap_or_default(),
                        name: attrs.get("name").cloned().unwrap_or_default(),
                        file: attrs.get("file").cloned(),
                        duration_ms: seconds_to_ms(attrs.get("time")),
                        outcome: Outcome::Passed,
                        failed_attempts: 0,
                        failure_fingerprint: None,
                    });
                }
                "failure" | "error" | "skipped" | "flakyFailure" | "flakyError" | "rerunFailure" | "rerunError" => {
                    if let Some(case) = current.as_mut() {
                        match name.as_str() {
                            "failure" => case.outcome = Outcome::Failed,
                            "error" => case.outcome = Outcome::Error,
                            "skipped" if case.outcome == Outcome::Passed => case.outcome = Outcome::Skipped,
                            "skipped" => {}
                            // A rerun failure means the final attempt failed too; the element only
                            // records an extra attempt.
                            _ => case.failed_attempts = case.failed_attempts.saturating_add(1),
                        }
                        if name != "skipped" && case.failure_fingerprint.is_none() {
                            case.failure_fingerprint = Some(fingerprint(attrs.get("type"), attrs.get("message")));
                        }
                    }
                }
                _ => {}
            },
            Node::Close { name } if name == "testcase" => {
                if let Some(case) = current.take() {
                    results.push(case);
                }
            }
            _ => {}
        }
        Ok(())
    })?;
    if !saw_root {
        return Err(EvidenceError::Malformed {
            format: FORMAT,
            message: "no <testsuite> or <testsuites> element".into(),
        });
    }
    Ok(results)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn reads_outcomes_durations_and_flaky_attempts() {
        let xml = r#"<?xml version="1.0"?>
<testsuites><testsuite name="s" tests="4">
  <testcase classname="a.B" name="ok()" time="0.012"/>
  <testcase classname="a.B" name="bad()" time="1.5"><failure type="org.opentest4j.AssertionFailedError" message="expected: &lt;1&gt; but was: &lt;2&gt;&#10;at line">trace</failure></testcase>
  <testcase classname="a.B" name="skip()"><skipped/></testcase>
  <testcase classname="a.B" name="flaky()" time="0.2"><flakyFailure type="X" message="boom"/></testcase>
</testsuite></testsuites>"#;
        let results = parse_junit(xml).unwrap();
        assert_eq!(results.len(), 4);
        assert_eq!((results[0].outcome, results[0].duration_ms), (Outcome::Passed, 12));
        assert_eq!(results[1].outcome, Outcome::Failed);
        assert_eq!(
            results[1].failure_fingerprint.as_deref(),
            Some("org.opentest4j.AssertionFailedError: expected: <1> but was: <2>")
        );
        assert_eq!(results[2].outcome, Outcome::Skipped);
        assert_eq!((results[3].outcome, results[3].failed_attempts), (Outcome::Passed, 1));
    }

    /// Excerpt of real Vitest 5.0.3 output (fixtures/typescript-checkout/evidence/v1/junit/run-1.xml):
    /// `classname` is the test file, `name` the describe chain joined with " > ".
    #[test]
    fn reads_vitest_reports() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8" ?>
<testsuites name="vitest tests" tests="5" failures="1" errors="0" time="0.0509029">
    <testsuite name="src/cart.test.ts" timestamp="2026-10-08T13:24:35.419Z" hostname="redacted" tests="1" failures="0" errors="0" skipped="0" time="0.0119194">
        <testcase classname="src/cart.test.ts" name="Cart &gt; totals line items" time="0.0073712">
        </testcase>
    </testsuite>
    <testsuite name="src/clock.test.ts" timestamp="2026-10-08T13:24:35.425Z" hostname="redacted" tests="1" failures="1" errors="0" skipped="0" time="0.0183348">
        <testcase classname="src/clock.test.ts" name="applies a discount before the checkout deadline" time="0.0145434">
            <failure message="expected 91.06748597396812 to be less than 65" type="AssertionError">
AssertionError: expected 91.06748597396812 to be less than 65
 ❯ src/clock.test.ts:12:30
            </failure>
        </testcase>
    </testsuite>
</testsuites>"#;
        let results = parse_junit(xml).unwrap();
        assert_eq!(
            (results[0].classname.as_str(), results[0].name.as_str(), results[0].duration_ms),
            ("src/cart.test.ts", "Cart > totals line items", 7)
        );
        assert_eq!(results[1].outcome, Outcome::Failed);
        assert_eq!(
            results[1].failure_fingerprint.as_deref(),
            Some("AssertionError: expected 91.06748597396812 to be less than 65")
        );
    }

    #[test]
    fn hostile_input_is_refused() {
        assert!(parse_junit("<html/>").is_err());
        let bomb = r#"<!DOCTYPE t [<!ENTITY x "x">]><testsuite>&x;</testsuite>"#;
        assert!(matches!(parse_junit(bomb), Err(EvidenceError::EntityDeclaration { .. })));
        assert_eq!(seconds_to_ms(Some(&"NaN".to_owned())), 0);
        assert_eq!(seconds_to_ms(Some(&"-3".to_owned())), 0);
    }
}
