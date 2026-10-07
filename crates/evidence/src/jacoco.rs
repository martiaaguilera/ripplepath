//! JaCoCo XML report (`report > package > sourcefile > line`).
//!
//! Per-line `ci` (covered instructions) > 0 means the line ran. Files are named
//! `<package path>/<sourcefile>`, e.g. `com/acme/bank/domain/Money.java`, without the source root.

use std::collections::BTreeMap;

use crate::xml::{Node, walk};
use crate::{CoverageReport, EvidenceError, FileCoverage};

const FORMAT: &str = "JaCoCo XML";

pub fn parse_jacoco(input: &str) -> Result<CoverageReport, EvidenceError> {
    let mut package: Option<String> = None;
    let mut current: Option<FileCoverage> = None;
    let mut files: BTreeMap<String, FileCoverage> = BTreeMap::new();
    let mut saw_report = false;
    walk(input, FORMAT, |node| {
        match node {
            Node::Open { name, attrs } => match name.as_str() {
                "report" => saw_report = true,
                "package" => package = attrs.get("name").cloned(),
                "sourcefile" => {
                    let file = attrs.get("name").cloned().unwrap_or_default();
                    let path = match package.as_deref() {
                        Some(p) if !p.is_empty() => format!("{p}/{file}"),
                        _ => file,
                    };
                    current = Some(FileCoverage { path, lines: BTreeMap::new() });
                }
                "line" => {
                    if let Some(file) = current.as_mut() {
                        let number = attrs.get("nr").and_then(|v| v.parse::<u32>().ok());
                        let covered = attrs.get("ci").and_then(|v| v.parse::<u64>().ok()).unwrap_or(0) > 0;
                        let Some(number) = number else {
                            return Err(EvidenceError::Malformed { format: FORMAT, message: "line without nr".into() });
                        };
                        let entry = file.lines.entry(number).or_insert(false);
                        *entry |= covered;
                    }
                }
                _ => {}
            },
            Node::Close { name } => match name.as_str() {
                "sourcefile" => {
                    if let Some(file) = current.take() {
                        let entry = files
                            .entry(file.path.clone())
                            .or_insert_with(|| FileCoverage { path: file.path.clone(), ..Default::default() });
                        for (line, covered) in file.lines {
                            *entry.lines.entry(line).or_insert(false) |= covered;
                        }
                    }
                }
                "package" => package = None,
                _ => {}
            },
        }
        Ok(())
    })?;
    if !saw_report {
        return Err(EvidenceError::Malformed { format: FORMAT, message: "no <report> element".into() });
    }
    Ok(CoverageReport { test: None, files: files.into_values().collect() })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><!DOCTYPE report PUBLIC "-//JACOCO//DTD Report 1.1//EN" "report.dtd"><report name="x"><package name="com/acme"><class name="com/acme/A" sourcefilename="A.java"><method name="f" desc="()V" line="3"><counter type="LINE" missed="0" covered="1"/></method></class><sourcefile name="A.java"><line nr="3" mi="0" ci="2" mb="0" cb="0"/><line nr="4" mi="3" ci="0" mb="0" cb="0"/></sourcefile></package></report>"#;

    #[test]
    fn reads_line_coverage_per_source_file() {
        let report = parse_jacoco(SAMPLE).unwrap();
        assert_eq!(report.files.len(), 1);
        assert_eq!(report.files[0].path, "com/acme/A.java");
        assert_eq!(report.files[0].lines.get(&3), Some(&true));
        assert_eq!(report.files[0].lines.get(&4), Some(&false));
    }

    #[test]
    fn refuses_entity_declarations_and_malformed_input() {
        let bomb = r#"<?xml version="1.0"?><!DOCTYPE report [<!ENTITY a "aaaaaaaa"><!ENTITY b "&a;&a;&a;">]><report>&b;</report>"#;
        assert_eq!(parse_jacoco(bomb), Err(EvidenceError::EntityDeclaration { format: FORMAT }));
        let external =
            r#"<?xml version="1.0"?><!DOCTYPE r [<!ENTITY x SYSTEM "file:///etc/passwd">]><report>&x;</report>"#;
        assert_eq!(parse_jacoco(external), Err(EvidenceError::EntityDeclaration { format: FORMAT }));
        assert!(matches!(parse_jacoco("<report><package"), Err(EvidenceError::Malformed { .. })));
        assert!(matches!(parse_jacoco("<notjacoco/>"), Err(EvidenceError::Malformed { .. })));
        let undeclared = r#"<report><package name="&undefined;"/></report>"#;
        assert!(matches!(parse_jacoco(undeclared), Err(EvidenceError::Malformed { .. })));
    }

    #[test]
    fn deep_nesting_is_refused() {
        let deep = format!("<report>{}{}</report>", "<a>".repeat(1000), "</a>".repeat(1000));
        assert!(matches!(parse_jacoco(&deep), Err(EvidenceError::LimitExceeded { what: "nesting depth", .. })));
    }
}
