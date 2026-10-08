//! Merge-policy gates. Pure: findings and the configured policy in, a verdict out.
//!
//! Each gate is a yes/no question about concrete findings. The risk score is deliberately *not* a
//! gate: a threshold on an uncalibrated sum would turn a prioritisation aid into an arbitrary
//! blocker (docs/SPEC.md, merge policy).

use serde::{Deserialize, Serialize};

use crate::config::{CycleMode, Gate, Policy};

pub const POLICY_NOTE: &str = "Gates evaluate concrete findings; the risk score is never a gate.";
const MAX_EVIDENCE: usize = 50;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PolicyResult {
    Pass,
    Warn,
    Fail,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum GateLevel {
    Off,
    Warn,
    Fail,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum GateStatus {
    Pass,
    Warn,
    Fail,
    /// Triggered, but the policy does not enforce this gate.
    Off,
    /// The input this gate needs is absent (e.g. no layers or critical paths configured).
    NotEvaluated,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GateResult {
    pub gate: Gate,
    pub level: GateLevel,
    pub status: GateStatus,
    pub detail: String,
    /// Sorted; at most 50.
    pub evidence: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyReport {
    pub result: PolicyResult,
    /// In gate order.
    pub gates: Vec<GateResult>,
    pub note: String,
}

/// What one gate found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GateFinding {
    pub gate: Gate,
    /// `None` when the gate cannot be evaluated.
    pub triggered: Option<bool>,
    pub detail: String,
    pub evidence: Vec<String>,
    /// When the gate would fail but every finding rests on inferred (not exactly resolved)
    /// evidence, it warns instead: heuristic evidence is not allowed to block a merge.
    pub inferred_only: bool,
}

impl GateFinding {
    pub fn new(gate: Gate, triggered: bool, detail: impl Into<String>, evidence: Vec<String>) -> Self {
        Self { gate, triggered: Some(triggered), detail: detail.into(), evidence, inferred_only: false }
    }

    pub fn not_evaluated(gate: Gate, detail: impl Into<String>) -> Self {
        Self { gate, triggered: None, detail: detail.into(), evidence: Vec::new(), inferred_only: false }
    }
}

fn level_of(policy: &Policy, gate: Gate, cycles: CycleMode) -> GateLevel {
    if gate == Gate::ConfigInvalid
        || policy.fail_on.contains(&gate)
        || (gate == Gate::NewCycle && cycles == CycleMode::Forbid)
    {
        GateLevel::Fail
    } else if policy.warn_on.contains(&gate) {
        GateLevel::Warn
    } else {
        GateLevel::Off
    }
}

pub fn evaluate(policy: &Policy, cycles: CycleMode, findings: Vec<GateFinding>) -> PolicyReport {
    let mut gates: Vec<GateResult> = findings
        .into_iter()
        .map(|finding| {
            let level = level_of(policy, finding.gate, cycles);
            let mut detail = finding.detail;
            let status = match (finding.triggered, level) {
                (None, _) => GateStatus::NotEvaluated,
                (Some(false), _) => GateStatus::Pass,
                (Some(true), GateLevel::Off) => GateStatus::Off,
                (Some(true), GateLevel::Warn) => GateStatus::Warn,
                (Some(true), GateLevel::Fail) if finding.inferred_only => {
                    detail.push_str(" (warning only: every finding rests on inferred edges)");
                    GateStatus::Warn
                }
                (Some(true), GateLevel::Fail) => GateStatus::Fail,
            };
            let mut evidence = finding.evidence;
            evidence.sort();
            evidence.dedup();
            evidence.truncate(MAX_EVIDENCE);
            GateResult { gate: finding.gate, level, status, detail, evidence }
        })
        .collect();
    gates.sort_by_key(|g| g.gate);
    let result = if gates.iter().any(|g| g.status == GateStatus::Fail) {
        PolicyResult::Fail
    } else if gates.iter().any(|g| g.status == GateStatus::Warn) {
        PolicyResult::Warn
    } else {
        PolicyResult::Pass
    };
    PolicyReport { result, gates, note: POLICY_NOTE.to_owned() }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::config::parse;

    #[test]
    fn levels_come_from_the_policy_and_unlisted_gates_are_off() {
        let policy =
            parse("version: 1\npolicy:\n  fail_on: [new_cycle]\n  warn_on: [config_changed]\n").unwrap().policy;
        let report = evaluate(
            &policy,
            CycleMode::Warn,
            vec![
                GateFinding::new(Gate::NewCycle, false, "no new cycle", vec![]),
                GateFinding::new(Gate::ConfigChanged, true, "edited", vec!["ripplepath.yml".into()]),
                GateFinding::new(Gate::BreakingApiChange, true, "1 removed", vec!["A#m".into()]),
                GateFinding::not_evaluated(Gate::RemovedTestOnCriticalPath, "no critical paths configured"),
            ],
        );
        let statuses: Vec<(Gate, GateLevel, GateStatus)> =
            report.gates.iter().map(|g| (g.gate, g.level, g.status)).collect();
        assert_eq!(
            statuses,
            vec![
                (Gate::NewCycle, GateLevel::Fail, GateStatus::Pass),
                (Gate::RemovedTestOnCriticalPath, GateLevel::Off, GateStatus::NotEvaluated),
                (Gate::BreakingApiChange, GateLevel::Off, GateStatus::Off),
                (Gate::ConfigChanged, GateLevel::Warn, GateStatus::Warn),
            ]
        );
        assert_eq!(report.result, PolicyResult::Warn);
    }

    #[test]
    fn fail_wins_inferred_only_findings_warn_and_forbid_forces_new_cycle() {
        let policy = parse("version: 1\npolicy:\n  fail_on: [new_architecture_violation]\n").unwrap().policy;
        let mut inferred = GateFinding::new(Gate::NewArchitectureViolation, true, "1 new violation", vec![]);
        inferred.inferred_only = true;
        let report = evaluate(&policy, CycleMode::Forbid, vec![inferred.clone()]);
        assert_eq!(report.gates[0].status, GateStatus::Warn);
        assert_eq!(report.result, PolicyResult::Warn);

        let cycle = GateFinding::new(Gate::NewCycle, true, "api <-> domain", vec![]);
        let report = evaluate(&policy, CycleMode::Forbid, vec![inferred, cycle]);
        assert_eq!(report.result, PolicyResult::Fail);
        assert_eq!(report.gates[1].level, GateLevel::Fail);
    }

    #[test]
    fn invalid_config_always_fails() {
        let report = evaluate(
            &Policy::default(),
            CycleMode::Off,
            vec![GateFinding::new(Gate::ConfigInvalid, true, "unknown field", vec![])],
        );
        assert_eq!(report.result, PolicyResult::Fail);
    }
}
