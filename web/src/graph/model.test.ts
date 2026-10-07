import { CONTROLLER, FEE_DECL, FEE_IMPL, SERVICE, TEST, sampleReport } from "../test/fixtures";
import { IMPACT_EDGE_KINDS, buildView, edgeKey, explainingPath, highlightFor, shortLabel } from "./model";

const allKinds = new Set(IMPACT_EDGE_KINDS);
const base = { edgeKinds: allKinds, query: "", hideIsolated: false };

describe("shortLabel", () => {
  it("shortens qualified ids to owner.member", () => {
    expect(shortLabel("java:com.acme.bank.domain.Account#withdraw(Money)")).toBe("Account.withdraw(Money)");
    expect(shortLabel("java:com.acme.bank.domain.Money#<init>(BigDecimal,String)")).toBe("Money(BigDecimal,String)");
    expect(shortLabel("java:com.acme.bank.domain.Money")).toBe("Money");
    expect(shortLabel("file:src/main/java/A.java")).toBe("A.java");
  });
});

describe("buildView", () => {
  it("filters nodes by depth and drops edges to hidden nodes", () => {
    const view = buildView(sampleReport(), { ...base, maxDepth: 2 });
    expect(view.nodes.map((n) => n.id)).toEqual([FEE_IMPL, FEE_DECL, SERVICE]);
    expect(view.edges.every((e) => e.from !== CONTROLLER && e.to !== CONTROLLER)).toBe(true);
  });

  it("filters edges by kind without removing nodes", () => {
    const view = buildView(sampleReport(), { ...base, maxDepth: 10, edgeKinds: new Set(["CALLS"]) });
    expect(view.nodes).toHaveLength(5);
    expect(view.edges.map((e) => e.kind)).toEqual(["CALLS", "CALLS", "CALLS"]);
  });

  it("hides isolated nodes on request and reports how many", () => {
    const view = buildView(sampleReport(), { ...base, maxDepth: 10, edgeKinds: new Set(["CALLS"]), hideIsolated: true });
    // Only OVERRIDES connects the changed implementation, so with CALLS alone it is isolated.
    expect(view.nodes.map((n) => n.id)).not.toContain(FEE_IMPL);
    expect(view.hiddenIsolated).toBe(1);
  });

  it("marks search matches case-insensitively", () => {
    const view = buildView(sampleReport(), { ...base, maxDepth: 10, query: "transfercontroller" });
    expect(view.nodes.filter((n) => n.matchesQuery).map((n) => n.id)).toEqual([CONTROLLER, TEST]);
  });
});

describe("explaining paths", () => {
  it("uses the report's path for impacted symbols and tests", () => {
    const report = sampleReport();
    expect(explainingPath(report, CONTROLLER)?.hops).toHaveLength(3);
    expect(explainingPath(report, TEST)?.root).toBe(FEE_IMPL);
    expect(explainingPath(report, FEE_IMPL)).toBeNull();
  });

  it("highlights exactly the nodes and edges on the path", () => {
    const report = sampleReport();
    const highlight = highlightFor(report, SERVICE);
    expect([...highlight.nodes].sort()).toEqual([FEE_DECL, FEE_IMPL, SERVICE].sort());
    expect(highlight.edges.size).toBe(2);
    const testEdge = report.graph.edges.find((e) => e.from === TEST);
    expect(testEdge && highlight.edges.has(edgeKey(testEdge))).toBe(false);
  });
});
