// FLAKINESS FIXTURE: nondeterministic on purpose. It stands in for a timing-dependent test (a
// deadline a slow CI machine sometimes misses): the code under test is deterministic, the
// "latency" is not, so one commit both passes and fails. scripts/collect-evidence-ts.sh runs the
// suite repeatedly to record that real flip for Ripplepath's history tests. Do not fix it.
import { expect, test } from "vitest";
import { money } from "./money";
import { applyDiscount } from "./pricing/discount";

test("applies a discount before the checkout deadline", () => {
  const simulatedLatencyMs = Math.random() * 100;
  expect(applyDiscount(money(100), "WELCOME10").amount).toBe(90);
  expect(simulatedLatencyMs).toBeLessThan(65);
});
