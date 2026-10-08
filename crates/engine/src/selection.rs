//! Test selection: order recommended tests and decide whether evidence allows running fewer than
//! all of them. Pure. Uncertainty widens the selection; it never narrows it.

use std::collections::BTreeSet;

use ripplepath_core::SymbolId;

use crate::history::Reliability;
use crate::report::{
    EvidenceTier, FallbackReason, SelectionDecision, SelectionMode, Severity, TestRecommendation, TestSelection,
};

/// Fast-feedback mode runs at most this many tests first.
pub const FAST_FEEDBACK_LIMIT: usize = 10;

/// The test units a recommendation runs: itself, or every unit inside a container.
pub type UnitsOf<'a> = dyn Fn(&SymbolId) -> Vec<(SymbolId, Option<u64>)> + 'a;

pub struct SelectionInput<'a> {
    pub mode: SelectionMode,
    pub tests: &'a [TestRecommendation],
    pub reasons: Vec<FallbackReason>,
    /// Every test unit in head with its median recorded duration (`None` without history).
    pub all_units: Vec<(SymbolId, Option<u64>)>,
    pub units_of: &'a UnitsOf<'a>,
}

fn rank_key(test: &TestRecommendation) -> (EvidenceTier, bool, u32, u64, &SymbolId) {
    let flaky = test.history.as_ref().is_some_and(|h| h.reliability == Reliability::Flaky);
    let duration = test.history.as_ref().and_then(|h| h.median_duration_ms).unwrap_or(u64::MAX);
    // Strongest evidence first; a flaky test's result says less, so it runs after reliable ones of
    // the same tier; then shorter paths; then known-fast tests; id as the final tiebreak.
    (test.tier, flaky, test.depth, duration, &test.id)
}

fn sum_known(durations: impl IntoIterator<Item = Option<u64>>) -> Option<u64> {
    durations.into_iter().try_fold(0u64, |acc, d| d.map(|d| acc.saturating_add(d)))
}

/// Units run by `tests`, each once even when a container and one of its units are both listed.
fn units(tests: &[&TestRecommendation], units_of: &UnitsOf<'_>) -> Vec<Option<u64>> {
    let mut seen = BTreeSet::new();
    tests.iter().flat_map(|t| units_of(&t.id)).filter(|(id, _)| seen.insert(id.clone())).map(|(_, d)| d).collect()
}

pub fn select(input: SelectionInput<'_>) -> TestSelection {
    let mut ranked: Vec<&TestRecommendation> = input.tests.iter().collect();
    ranked.sort_by(|a, b| rank_key(a).cmp(&rank_key(b)));
    let mut reasons = input.reasons;
    reasons.sort();
    reasons.dedup();

    let any_high = reasons.iter().any(|r| r.severity == Severity::High);
    let any = !reasons.is_empty();
    let mut notes = Vec::new();
    let all_durations: Vec<Option<u64>> = input.all_units.iter().map(|(_, d)| *d).collect();
    let total_units = all_durations.len();
    let full_runtime_ms = sum_known(all_durations.iter().copied());

    let (decision, ordered, selected_durations): (_, Vec<&TestRecommendation>, Vec<Option<u64>>) = match input.mode {
        SelectionMode::Conservative | SelectionMode::Balanced => {
            let fallback = if input.mode == SelectionMode::Conservative { any } else { any_high };
            if fallback {
                notes.push("Evidence is not strong enough to skip tests: run the full suite. Listed tests are the best place to start.".to_owned());
                (SelectionDecision::FullSuite, ranked, all_durations.clone())
            } else {
                let durations = units(&ranked, input.units_of);
                (SelectionDecision::Selected, ranked, durations)
            }
        }
        SelectionMode::FastFeedback => {
            let prefix: Vec<&TestRecommendation> = ranked.into_iter().take(FAST_FEEDBACK_LIMIT).collect();
            let durations = units(&prefix, input.units_of);
            notes.push("Fast feedback: these tests run first. Passing them is not complete validation.".to_owned());
            if any {
                notes.push("Fallback reasons apply: run the full suite after these.".to_owned());
            }
            (SelectionDecision::Selected, prefix, durations)
        }
    };

    if input.tests.is_empty() && decision == SelectionDecision::Selected {
        notes.push("No test has evidence of exercising this change.".to_owned());
    }
    let selected_runtime_ms = match decision {
        SelectionDecision::FullSuite => full_runtime_ms,
        SelectionDecision::Selected => sum_known(selected_durations.iter().copied()),
    };
    if selected_runtime_ms.is_none() && !selected_durations.is_empty() {
        notes.push("Runtime is not estimated: some selected tests have no recorded duration.".to_owned());
    }
    let selected_units = match decision {
        SelectionDecision::FullSuite => total_units,
        SelectionDecision::Selected => selected_durations.len(),
    };

    TestSelection {
        mode: input.mode,
        decision,
        fallback_reasons: reasons,
        ordered: ordered.into_iter().map(|t| t.id.clone()).collect(),
        selected_units,
        total_units,
        selected_runtime_ms,
        full_runtime_ms,
        notes,
    }
}

#[cfg(test)]
mod tests {
    use ripplepath_core::Evidence;

    use super::*;
    use crate::history::TestHistory;
    use crate::report::TestReason;

    fn test(
        id: &str,
        tier: EvidenceTier,
        depth: u32,
        duration: Option<u64>,
        reliability: Reliability,
    ) -> TestRecommendation {
        TestRecommendation {
            id: SymbolId::new(id),
            name: id.into(),
            file: "f".into(),
            reason: TestReason::StaticPath,
            depth,
            root: SymbolId::new("root"),
            weakest_evidence: Evidence::ResolvedExact,
            path: Vec::new(),
            tier,
            coverage_observed: false,
            history: duration.map(|d| TestHistory {
                runs: 5,
                failures: 0,
                flaky_commits: 0,
                median_duration_ms: Some(d),
                last_outcome: Some("PASSED".into()),
                reliability,
            }),
        }
    }

    fn reason(severity: Severity) -> FallbackReason {
        FallbackReason { severity, code: "X".into(), detail: "x".into() }
    }

    fn all_units(n: usize, duration: Option<u64>) -> Vec<(SymbolId, Option<u64>)> {
        (0..n).map(|i| (SymbolId::new(format!("u{i}")), duration)).collect()
    }

    fn run(mode: SelectionMode, tests: &[TestRecommendation], reasons: Vec<FallbackReason>) -> TestSelection {
        let units_of = |id: &SymbolId| vec![(id.clone(), Some(1))];
        select(SelectionInput { mode, tests, reasons, all_units: all_units(10, Some(1)), units_of: &units_of })
    }

    #[test]
    fn orders_by_tier_then_reliability_then_depth_then_duration() {
        let tests = [
            test("weak", EvidenceTier::Weak, 1, Some(1), Reliability::Stable),
            test("flaky", EvidenceTier::Strong, 1, Some(1), Reliability::Flaky),
            test("slow", EvidenceTier::Strong, 1, Some(900), Reliability::Stable),
            test("fast", EvidenceTier::Strong, 1, Some(10), Reliability::Stable),
            test("deep", EvidenceTier::Strong, 3, Some(1), Reliability::Stable),
        ];
        let selection = run(SelectionMode::Balanced, &tests, Vec::new());
        let order: Vec<&str> = selection.ordered.iter().map(|i| i.as_str()).collect();
        assert_eq!(order, vec!["fast", "slow", "deep", "flaky", "weak"]);
        assert_eq!(selection.decision, SelectionDecision::Selected);
    }

    #[test]
    fn uncertainty_widens_by_mode() {
        let tests = [test("a", EvidenceTier::Medium, 1, Some(1), Reliability::Stable)];
        assert_eq!(
            run(SelectionMode::Balanced, &tests, vec![reason(Severity::Medium)]).decision,
            SelectionDecision::Selected
        );
        assert_eq!(
            run(SelectionMode::Balanced, &tests, vec![reason(Severity::High)]).decision,
            SelectionDecision::FullSuite
        );
        let conservative = run(SelectionMode::Conservative, &tests, vec![reason(Severity::Medium)]);
        assert_eq!(conservative.decision, SelectionDecision::FullSuite);
        assert_eq!(conservative.selected_units, 10);
        let fast = run(SelectionMode::FastFeedback, &tests, vec![reason(Severity::High)]);
        assert_eq!(fast.decision, SelectionDecision::Selected);
        assert!(fast.notes.iter().any(|n| n.contains("full suite")));
    }

    #[test]
    fn a_container_and_its_unit_count_once() {
        let tests = [
            test("file:t.test.ts", EvidenceTier::Strong, 0, None, Reliability::Stable),
            test("ts:t.test.ts#test:a", EvidenceTier::Strong, 1, Some(3), Reliability::Stable),
        ];
        let units_of = |id: &SymbolId| match id.as_str() {
            "file:t.test.ts" => {
                vec![(SymbolId::new("ts:t.test.ts#test:a"), Some(3)), (SymbolId::new("ts:t.test.ts#test:b"), Some(4))]
            }
            _ => vec![(id.clone(), Some(3))],
        };
        let selection = select(SelectionInput {
            mode: SelectionMode::Balanced,
            tests: &tests,
            reasons: Vec::new(),
            all_units: all_units(5, Some(1)),
            units_of: &units_of,
        });
        assert_eq!(selection.selected_units, 2);
        assert_eq!(selection.selected_runtime_ms, Some(7));
    }

    #[test]
    fn runtime_is_only_estimated_when_fully_known() {
        let units_of = |id: &SymbolId| vec![(id.clone(), None), (SymbolId::new("other"), Some(5))];
        let tests = [test("a", EvidenceTier::Strong, 1, None, Reliability::Stable)];
        let mut all = all_units(2, Some(5));
        all.push((SymbolId::new("x"), None));
        let selection = select(SelectionInput {
            mode: SelectionMode::Balanced,
            tests: &tests,
            reasons: Vec::new(),
            all_units: all,
            units_of: &units_of,
        });
        assert_eq!(selection.selected_runtime_ms, None);
        assert_eq!(selection.full_runtime_ms, None);
        assert!(selection.notes.iter().any(|n| n.contains("not estimated")));
    }
}
