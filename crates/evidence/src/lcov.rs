//! LCOV tracefiles (`TN:`, `SF:`, `DA:<line>,<hits>`, `end_of_record`).
//!
//! A tracefile may hold several test names (`TN:`); each becomes its own report so per-test
//! coverage is kept per test. An empty `TN:` means aggregate coverage.

use std::collections::BTreeMap;

use crate::{CoverageReport, EvidenceError, FileCoverage, check_size};

const FORMAT: &str = "LCOV";
const MAX_LINES: usize = 20_000_000;

pub fn parse_lcov(input: &str) -> Result<Vec<CoverageReport>, EvidenceError> {
    check_size(input)?;
    let mut by_test: BTreeMap<Option<String>, BTreeMap<String, FileCoverage>> = BTreeMap::new();
    let mut test: Option<String> = None;
    let mut file: Option<FileCoverage> = None;
    for (index, raw) in input.lines().enumerate() {
        if index >= MAX_LINES {
            return Err(EvidenceError::LimitExceeded { format: FORMAT, what: "line count" });
        }
        let line = raw.trim();
        let malformed = |message: &str| EvidenceError::Malformed {
            format: FORMAT,
            message: format!("line {}: {message}", index + 1),
        };
        if let Some(name) = line.strip_prefix("TN:") {
            test = Some(name.trim().to_owned()).filter(|n| !n.is_empty());
        } else if let Some(path) = line.strip_prefix("SF:") {
            file = Some(FileCoverage { path: path.trim().replace('\\', "/"), lines: BTreeMap::new() });
        } else if let Some(data) = line.strip_prefix("DA:") {
            let current = file.as_mut().ok_or_else(|| malformed("DA outside a source file record"))?;
            let mut parts = data.split(',');
            let number =
                parts.next().and_then(|n| n.trim().parse::<u32>().ok()).ok_or_else(|| malformed("bad line number"))?;
            // Hit counts can be huge or written as floats by some tools; only "> 0" matters.
            let hits = parts.next().map(str::trim).ok_or_else(|| malformed("missing hit count"))?;
            let covered = hits.parse::<f64>().map_err(|_| malformed("bad hit count"))? > 0.0;
            *current.lines.entry(number).or_insert(false) |= covered;
        } else if line == "end_of_record" {
            let finished = file.take().ok_or_else(|| malformed("end_of_record without SF"))?;
            let files = by_test.entry(test.clone()).or_default();
            let entry = files
                .entry(finished.path.clone())
                .or_insert_with(|| FileCoverage { path: finished.path.clone(), ..Default::default() });
            for (number, covered) in finished.lines {
                *entry.lines.entry(number).or_insert(false) |= covered;
            }
        }
        // Other records (FN, FNDA, BRDA, LF, LH, …) carry nothing line coverage needs.
    }
    if file.is_some() {
        return Err(EvidenceError::Malformed { format: FORMAT, message: "missing end_of_record".into() });
    }
    Ok(by_test.into_iter().map(|(test, files)| CoverageReport { test, files: files.into_values().collect() }).collect())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn splits_reports_by_test_name() {
        let input =
            "TN:\nSF:src/a.ts\nDA:1,1\nDA:2,0\nend_of_record\nTN:cart\nSF:C:\\repo\\src\\b.ts\nDA:5,3\nend_of_record\n";
        let reports = parse_lcov(input).unwrap();
        assert_eq!(reports.len(), 2);
        assert_eq!(reports[0].test, None);
        assert_eq!(reports[0].files[0].lines, BTreeMap::from([(1, true), (2, false)]));
        assert_eq!(reports[1].test.as_deref(), Some("cart"));
        assert_eq!(reports[1].files[0].path, "C:/repo/src/b.ts");
    }

    #[test]
    fn rejects_malformed_records() {
        assert!(parse_lcov("DA:1,1\n").is_err());
        assert!(parse_lcov("SF:a\nDA:x,1\nend_of_record\n").is_err());
        assert!(parse_lcov("SF:a\nDA:1,1\n").is_err());
    }
}
