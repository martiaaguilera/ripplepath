//! Risk model v1: a deterministic, versioned decomposition of review-relevant signals.
//!
//! The score is a sum of capped integer points. It is **not a probability of failure**: nothing
//! here is calibrated against outcomes, and it must never be presented as one. Weights, caps and
//! bands are part of the model version; changing any of them requires a new version
//! (docs/RISK_MODEL.md, docs/adr/0006-deterministic-risk-model.md). Pure.

use serde::{Deserialize, Serialize};

pub const RISK_MODEL_VERSION: u32 = 1;
pub const MAX_SCORE: u32 = 100;
pub const NOT_A_PROBABILITY: &str = "Risk score: a sum of capped, versioned signal points for prioritising review. \
It is NOT a probability of failure and is not calibrated against outcomes.";
const MAX_EVIDENCE: usize = 50;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

/// Inclusive lower bounds of MEDIUM, HIGH and CRITICAL.
pub const LEVEL_BANDS: [(u32, RiskLevel); 3] =
    [(15, RiskLevel::Medium), (40, RiskLevel::High), (70, RiskLevel::Critical)];

pub fn level(score: u32) -> RiskLevel {
    LEVEL_BANDS.iter().rev().find(|(min, _)| score >= *min).map_or(RiskLevel::Low, |(_, l)| *l)
}

/// How a measured value becomes units before weighting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scale {
    /// One unit per counted item.
    Linear,
    /// Units = how many thresholds the value reaches. Roughly logarithmic: each band is several
    /// times wider than the previous, so a huge change cannot dominate the total by size alone.
    Bands(&'static [u64]),
}

impl Scale {
    pub fn units(self, value: u64) -> u64 {
        match self {
            Scale::Linear => value,
            Scale::Bands(thresholds) => thresholds.iter().filter(|t| value >= **t).count() as u64,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalId {
    PublicApiChanged,
    MigrationChanged,
    BuildOrDependencyChanged,
    BlastRadius,
    ChangedSymbolCentrality,
    CriticalPathChanged,
    NewArchitectureViolation,
    NewLayerCycle,
    ChangedCodeWithoutCoverage,
    FlakyImpactedTests,
    DeletedTests,
    ParserUncertainty,
    UnresolvedReferencesInChangedCode,
    ConfigChanged,
}

pub struct SignalSpec {
    pub id: SignalId,
    pub definition: &'static str,
    pub scale: Scale,
    pub weight: u32,
    pub cap: u32,
}

/// Model v1. Order is the report order.
pub const SIGNALS: [SignalSpec; 14] = [
    SignalSpec {
        id: SignalId::PublicApiChanged,
        definition: "Public API symbols removed, narrowed or with a changed signature",
        scale: Scale::Linear,
        weight: 8,
        cap: 24,
    },
    SignalSpec {
        id: SignalId::MigrationChanged,
        definition: "Database migration or schema files changed",
        scale: Scale::Linear,
        weight: 15,
        cap: 15,
    },
    SignalSpec {
        id: SignalId::BuildOrDependencyChanged,
        definition: "Build, lockfile, CI, container or runtime configuration files changed",
        scale: Scale::Linear,
        weight: 5,
        cap: 10,
    },
    SignalSpec {
        id: SignalId::BlastRadius,
        definition: "Impacted non-test symbols, banded 1 / 6 / 21 / 101",
        scale: Scale::Bands(&[1, 6, 21, 101]),
        weight: 5,
        cap: 20,
    },
    SignalSpec {
        id: SignalId::ChangedSymbolCentrality,
        definition: "Most direct dependents of any changed non-test symbol, banded 3 / 10 / 30",
        scale: Scale::Bands(&[3, 10, 30]),
        weight: 5,
        cap: 15,
    },
    SignalSpec {
        id: SignalId::CriticalPathChanged,
        definition: "Changed files matching the configured critical paths",
        scale: Scale::Linear,
        weight: 10,
        cap: 20,
    },
    SignalSpec {
        id: SignalId::NewArchitectureViolation,
        definition: "Layer-rule violations present in head but not in base",
        scale: Scale::Linear,
        weight: 10,
        cap: 20,
    },
    SignalSpec {
        id: SignalId::NewLayerCycle,
        definition: "Layer dependency cycles present in head but not in base",
        scale: Scale::Linear,
        weight: 10,
        cap: 10,
    },
    SignalSpec {
        id: SignalId::ChangedCodeWithoutCoverage,
        definition: "Added or modified non-test code with no measured test execution (needs coverage)",
        scale: Scale::Linear,
        weight: 3,
        cap: 15,
    },
    SignalSpec {
        id: SignalId::FlakyImpactedTests,
        definition: "Recommended tests classified FLAKY by recorded CI history",
        scale: Scale::Linear,
        weight: 3,
        cap: 9,
    },
    SignalSpec {
        id: SignalId::DeletedTests,
        definition: "Test units deleted by the change",
        scale: Scale::Linear,
        weight: 5,
        cap: 15,
    },
    SignalSpec {
        id: SignalId::ParserUncertainty,
        definition: "Changed files that could not be fully parsed (syntax errors, parse failures, size limit)",
        scale: Scale::Linear,
        weight: 5,
        cap: 15,
    },
    SignalSpec {
        id: SignalId::UnresolvedReferencesInChangedCode,
        definition: "References from changed symbols that could not be resolved",
        scale: Scale::Linear,
        weight: 1,
        cap: 5,
    },
    SignalSpec {
        id: SignalId::ConfigChanged,
        definition: "The change edits ripplepath.yml (rules take effect only after merge)",
        scale: Scale::Linear,
        weight: 5,
        cap: 5,
    },
];

/// One signal's input. `evaluated: false` means the input needed for it is absent (no coverage,
/// no layers configured, …): it contributes nothing, and the report says so.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Measurement {
    pub evaluated: bool,
    pub value: u64,
    /// Symbol ids or paths behind the value, sorted.
    pub evidence: Vec<String>,
    pub note: Option<String>,
}

impl Measurement {
    pub fn of(value: u64, mut evidence: Vec<String>) -> Self {
        evidence.sort();
        evidence.dedup();
        Self { evaluated: true, value, evidence, note: None }
    }

    pub fn count(mut evidence: Vec<String>) -> Self {
        evidence.sort();
        evidence.dedup();
        Self::of(evidence.len() as u64, evidence)
    }

    pub fn not_evaluated(note: impl Into<String>) -> Self {
        Self { evaluated: false, value: 0, evidence: Vec::new(), note: Some(note.into()) }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RiskSignal {
    pub id: SignalId,
    pub definition: String,
    pub evaluated: bool,
    /// The measured quantity (a count, or for banded signals the raw size).
    pub value: u64,
    /// `value` after scaling (equal to it for linear signals, the band index for banded ones).
    pub units: u64,
    /// Points per unit.
    pub weight: u32,
    pub cap: u32,
    /// min(units × weight, cap).
    pub points: u32,
    /// Sorted; at most 50 listed.
    pub evidence: Vec<String>,
    pub evidence_truncated: usize,
    pub note: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RiskReport {
    pub model_version: u32,
    /// Sum of signal points, capped at 100.
    pub score: u32,
    /// Sum before the cap.
    pub uncapped_total: u32,
    pub level: RiskLevel,
    pub interpretation: String,
    /// In model order.
    pub signals: Vec<RiskSignal>,
}

/// Scores measurements, given in any order; signals without a measurement are not evaluated.
pub fn score(measurements: &[(SignalId, Measurement)]) -> RiskReport {
    let mut signals = Vec::with_capacity(SIGNALS.len());
    let mut total: u32 = 0;
    for spec in &SIGNALS {
        let measurement = measurements
            .iter()
            .find(|(id, _)| *id == spec.id)
            .map(|(_, m)| m.clone())
            .unwrap_or_else(|| Measurement::not_evaluated("no input for this signal"));
        let units = if measurement.evaluated { spec.scale.units(measurement.value) } else { 0 };
        let points = u32::try_from(units.saturating_mul(u64::from(spec.weight))).unwrap_or(u32::MAX).min(spec.cap);
        total = total.saturating_add(points);
        let mut evidence = measurement.evidence;
        let evidence_truncated = evidence.len().saturating_sub(MAX_EVIDENCE);
        evidence.truncate(MAX_EVIDENCE);
        signals.push(RiskSignal {
            id: spec.id,
            definition: spec.definition.to_owned(),
            evaluated: measurement.evaluated,
            value: measurement.value,
            units,
            weight: spec.weight,
            cap: spec.cap,
            points,
            evidence,
            evidence_truncated,
            note: measurement.note,
        });
    }
    let score = total.min(MAX_SCORE);
    RiskReport {
        model_version: RISK_MODEL_VERSION,
        score,
        uncapped_total: total,
        level: level(score),
        interpretation: NOT_A_PROBABILITY.to_owned(),
        signals,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn points(report: &RiskReport) -> Vec<(SignalId, u64, u32)> {
        report.signals.iter().filter(|s| s.points > 0).map(|s| (s.id, s.units, s.points)).collect()
    }

    #[test]
    fn empty_change_scores_zero_and_marks_everything_unevaluated() {
        let report = score(&[]);
        assert_eq!(report.score, 0);
        assert_eq!(report.level, RiskLevel::Low);
        assert_eq!(report.signals.len(), SIGNALS.len());
        assert!(report.signals.iter().all(|s| !s.evaluated && s.points == 0));
        assert!(report.interpretation.contains("NOT a probability"));
    }

    #[test]
    fn exact_decomposition_for_a_constructed_change() {
        let m = |value: u64| Measurement::of(value, Vec::new());
        let report = score(&[
            (SignalId::PublicApiChanged, m(2)),
            (SignalId::MigrationChanged, m(1)),
            (SignalId::BlastRadius, m(7)),
            (SignalId::ChangedSymbolCentrality, m(2)),
            (SignalId::UnresolvedReferencesInChangedCode, m(3)),
            (SignalId::ChangedCodeWithoutCoverage, Measurement::not_evaluated("no coverage ingested")),
        ]);
        assert_eq!(
            points(&report),
            vec![
                (SignalId::PublicApiChanged, 2, 16),
                (SignalId::MigrationChanged, 1, 15),
                (SignalId::BlastRadius, 2, 10),
                (SignalId::UnresolvedReferencesInChangedCode, 3, 3),
            ]
        );
        assert_eq!(report.score, 44);
        assert_eq!(report.level, RiskLevel::High);
        let coverage = report.signals.iter().find(|s| s.id == SignalId::ChangedCodeWithoutCoverage).unwrap();
        assert!(!coverage.evaluated);
        assert_eq!(coverage.note.as_deref(), Some("no coverage ingested"));
    }

    #[test]
    fn caps_apply_per_signal_and_to_the_total() {
        let m = |value: u64| Measurement::of(value, Vec::new());
        let all: Vec<(SignalId, Measurement)> = SIGNALS.iter().map(|s| (s.id, m(1_000_000))).collect();
        let report = score(&all);
        for signal in &report.signals {
            assert_eq!(signal.points, signal.cap, "{:?}", signal.id);
        }
        assert_eq!(report.uncapped_total, SIGNALS.iter().map(|s| s.cap).sum::<u32>());
        assert_eq!(report.score, MAX_SCORE);
        assert_eq!(report.level, RiskLevel::Critical);
    }

    #[test]
    fn bands_and_levels_have_documented_edges() {
        let blast = Scale::Bands(&[1, 6, 21, 101]);
        let units: Vec<u64> = [0, 1, 5, 6, 20, 21, 100, 101, 10_000].iter().map(|v| blast.units(*v)).collect();
        assert_eq!(units, vec![0, 1, 1, 2, 2, 3, 3, 4, 4]);
        let levels: Vec<RiskLevel> = [0, 14, 15, 39, 40, 69, 70, 100].iter().map(|s| level(*s)).collect();
        use RiskLevel::*;
        assert_eq!(levels, vec![Low, Low, Medium, Medium, High, High, Critical, Critical]);
    }

    #[test]
    fn evidence_is_sorted_deduplicated_and_bounded() {
        let evidence: Vec<String> = (0..60).rev().map(|i| format!("e{i:02}")).chain(["e00".to_owned()]).collect();
        let report = score(&[(SignalId::DeletedTests, Measurement::count(evidence))]);
        let signal = report.signals.iter().find(|s| s.id == SignalId::DeletedTests).unwrap();
        assert_eq!(signal.value, 60);
        assert_eq!(signal.evidence.first().map(String::as_str), Some("e00"));
        assert_eq!(signal.evidence.len(), 50);
        assert_eq!(signal.evidence_truncated, 10);
        assert_eq!(signal.points, 15);
    }
}
