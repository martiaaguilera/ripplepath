import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AnalysisReport, GraphNode, TestRecommendation } from "../api/types";
import { parseUrl, toSearch, type UrlState } from "../app/url";
import { ChangeHeader } from "../components/ChangeHeader";
import { NodeList } from "../components/GraphLists";
import {
  ACCOUNT,
  API_ERRORS,
  COV_TEST,
  FEE_BASE,
  FEE_FILE,
  FEE_HEAD,
  FEE_IMPL,
  MIGRATION,
  TEST,
  blob,
  sampleReport,
} from "../test/fixtures";
import { ArchitectureView } from "./ArchitectureView";
import { DiffView } from "./DiffView";
import { OwnersView } from "./OwnersView";
import { RiskView } from "./RiskView";
import { TestsView } from "./TestsView";

const fetchFile = vi.hoisted(() => vi.fn());
vi.mock("../api/client", () => ({ fetchFile }));

function url(patch: Partial<UrlState> = {}): UrlState {
  return { base: "main~1", head: "main", view: "overview", sel: null, file: null, test: null, line: null, ...patch };
}

describe("URL state", () => {
  it("round-trips every field and omits defaults", () => {
    const state = url({ view: "diff", file: "src/a b.java", line: 12, sel: "java:a.B#c(int)" });
    expect(parseUrl(toSearch(state))).toEqual(state);
    expect(toSearch(url())).toBe("?base=main%7E1&head=main");
  });

  it("ignores unknown views and non-numeric lines", () => {
    const parsed = parseUrl("?view=admin&line=12abc");
    expect(parsed.view).toBe("overview");
    expect(parsed.line).toBeNull();
  });
});

describe("ChangeHeader", () => {
  it("states the risk score is not a probability and shows each verdict from the report", () => {
    render(<ChangeHeader report={sampleReport()} url={url()} navigate={vi.fn()} />);
    const strip = screen.getByRole("region", { name: "Change summary" });
    expect(strip).toHaveTextContent("25");
    expect(strip).toHaveTextContent("not a probability");
    expect(strip).toHaveTextContent("It is not a probability of failure");
    expect(strip).toHaveTextContent("Full suite");
    expect(strip).toHaveTextContent("1 new violations");
    expect(within(strip).getByRole("link", { name: /Policy/ })).toHaveTextContent("fail");
  });

  it("navigates to the explaining view", () => {
    const navigate = vi.fn();
    render(<ChangeHeader report={sampleReport()} url={url()} navigate={navigate} />);
    fireEvent.click(screen.getByRole("link", { name: /^Tests/ }));
    expect(navigate).toHaveBeenCalledWith({ view: "tests" });
  });
});

describe("RiskView", () => {
  it("renders the decomposition exactly as reported", () => {
    render(<RiskView report={sampleReport()} navigate={vi.fn()} />);
    const table = screen.getByRole("table", { name: /Risk signals/ });
    const migration = within(table).getByRole("row", { name: /Migration changed/ });
    expect(migration).toHaveTextContent("×15");
    expect(within(migration).getByRole("img", { name: "15 of a maximum 15 points" })).toBeInTheDocument();
    const flaky = within(table).getByRole("row", { name: /Flaky impacted tests/ });
    expect(flaky).toHaveTextContent("Not evaluated");
    expect(flaky).toHaveTextContent("no CI history ingested");
    expect(screen.getByRole("note")).toHaveTextContent("not a probability of failure");
  });

  it("links symbol ids in evidence, including ids that end in a parenthesis", () => {
    const report = sampleReport();
    // ACCOUNT only appears in evidence; make it a navigable symbol.
    report.graph.nodes.push({ ...(report.graph.nodes[0] as GraphNode), id: ACCOUNT, role: "impacted", change: null });
    const navigate = vi.fn();
    render(<RiskView report={report} navigate={navigate} />);
    const row = screen.getByRole("row", { name: /New architecture violation/ });
    fireEvent.click(within(row).getByRole("button", { name: "Account.withdraw(Money)" }));
    expect(navigate).toHaveBeenCalledWith({ view: "overview", sel: ACCOUNT });
    // The file token keeps its line and opens the diff only for changed files.
    expect(within(row).queryByRole("button", { name: /Account\.java:25/ })).toBeNull();
    fireEvent.click(within(screen.getByRole("row", { name: /Migration changed/ })).getByRole("button", { name: MIGRATION }));
    expect(navigate).toHaveBeenCalledWith({ view: "diff", file: MIGRATION });
  });

  it("lists every policy gate with its result", () => {
    render(<RiskView report={sampleReport()} navigate={vi.fn()} />);
    const gates = screen.getByRole("table", { name: /Merge-policy gates/ });
    expect(within(gates).getByRole("row", { name: /new_architecture_violation/ })).toHaveTextContent("fail");
    expect(within(gates).getByRole("row", { name: /config_changed/ })).toHaveTextContent("pass");
  });
});

describe("TestsView", () => {
  it("shows the decision, every fallback reason and the run order", () => {
    render(<TestsView report={sampleReport()} url={url({ view: "tests" })} navigate={vi.fn()} />);
    expect(screen.getByRole("heading", { name: "Run the full suite" })).toBeInTheDocument();
    expect(screen.getByText("MIGRATION_CHANGED")).toBeInTheDocument();
    // No runtime is invented when history is incomplete.
    expect(screen.getAllByText("not estimated")).toHaveLength(2);
    const rows = within(screen.getByRole("table", { name: /Recommended tests/ })).getAllByRole("row").slice(1);
    expect(rows[0]).toHaveTextContent("TransferServiceTest");
    expect(rows[0]).toHaveTextContent("measured coverage");
    expect(rows[1]).toHaveTextContent("flaky");
    expect(rows[1]).toHaveTextContent("1.52 s");
    expect(rows[1]).toHaveTextContent("failed");
  });

  it("draws measured coverage hops differently from static hops", () => {
    const { rerender } = render(
      <TestsView report={sampleReport()} url={url({ view: "tests", test: COV_TEST })} navigate={vi.fn()} />,
    );
    const path = screen.getByRole("list", { name: /Evidence path/ });
    const steps = within(path).getAllByRole("listitem");
    expect(steps[1]).toHaveClass("path__step--measured");
    expect(steps[1]).toHaveTextContent("measured by coverage");
    expect(steps[1]).toHaveTextContent("coverage.jacoco@aaaaaaaaaa");

    rerender(<TestsView report={sampleReport()} url={url({ view: "tests", test: TEST })} navigate={vi.fn()} />);
    const staticSteps = within(screen.getByRole("list", { name: /Evidence path/ })).getAllByRole("listitem");
    expect(staticSteps.slice(1).every((s) => s.classList.contains("path__step--static"))).toBe(true);
    expect(staticSteps[3]).toHaveTextContent("statically inferred");
  });

  it("selects a test through the URL", () => {
    const navigate = vi.fn();
    render(<TestsView report={sampleReport()} url={url({ view: "tests" })} navigate={navigate} />);
    fireEvent.click(screen.getByRole("button", { name: "TransferControllerTest.returnsOk()" }));
    expect(navigate).toHaveBeenCalledWith({ test: TEST }, { replace: true });
  });

  it("windows long test tables without dropping the table structure", () => {
    const report = sampleReport();
    const template = report.tests[1] as TestRecommendation;
    const many: TestRecommendation[] = Array.from({ length: 400 }, (_, i) => ({
      ...template,
      id: `java:bank.T${String(i).padStart(3, "0")}`,
    }));
    report.tests = many;
    report.test_selection.ordered = many.map((t) => t.id);
    render(<TestsView report={report} url={url({ view: "tests" })} navigate={vi.fn()} />);
    const table = screen.getByRole("table", { name: /Recommended tests/ });
    const rendered = within(table).getAllByRole("button").length;
    expect(rendered).toBeGreaterThan(0);
    expect(rendered).toBeLessThan(400);
    expect(screen.getByRole("heading", { name: /Ranked tests/ })).toHaveTextContent("400");
  });
});

describe("ArchitectureView", () => {
  it("shows new violations with the exact edge, file:line and rule", () => {
    const navigate = vi.fn();
    render(<ArchitectureView report={sampleReport()} navigate={navigate} />);
    const violations = screen.getByRole("table", { name: /Layer-rule violations/ });
    expect(within(violations).getByRole("row", { name: /domain → api/ })).toHaveTextContent("new");
    const detail = screen.getByRole("complementary", { name: "Violation detail" });
    expect(detail).toHaveTextContent("#0 domain must not depend on api");
    expect(detail).toHaveTextContent("src/domain/Account.java:25");
    expect(detail).toHaveTextContent("java.call.static");
    expect(detail).toHaveTextContent("does not exist in base");
    // Not a symbol of this report's graph, so it is not offered as a link to the blast radius.
    expect(within(detail).queryByRole("button", { name: "Account.withdraw(Money)" })).toBeNull();
    // Account.java did not change, so there is no diff to open for it.
    expect(within(detail).queryByRole("button", { name: /Account\.java:25/ })).toBeNull();
  });

  it("switches the diagram between head, base and the delta", () => {
    render(<ArchitectureView report={sampleReport()} navigate={vi.fn()} />);
    const diagram = screen.getByRole("group", { name: /Layer diagram/ });
    expect(within(diagram).getByRole("button", { name: /domain → api: 1 edges, 1 new violations/ })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("radio", { name: "Base" }));
    // domain → api has no edges in base, so the arc is not drawn.
    expect(within(diagram).queryByRole("button", { name: /domain → api/ })).toBeNull();
    fireEvent.click(screen.getByRole("radio", { name: "Base → head" }));
    expect(within(diagram).getByRole("button", { name: /domain → api: 0→1 edges/ })).toBeInTheDocument();
  });

  it("filters violations by the selected layer arc and by status", () => {
    render(<ArchitectureView report={sampleReport()} navigate={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: /api → domain: 3 edges/ }));
    expect(screen.getByText("No layer-rule violation matches the current filters.")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /api → domain ✕/ }));
    fireEvent.click(screen.getByRole("checkbox", { name: "new" }));
    expect(screen.getByText("No layer-rule violation matches the current filters.")).toBeInTheDocument();
  });

  it("explains how to configure layers when none are", () => {
    const report = sampleReport();
    report.architecture.configured = false;
    render(<ArchitectureView report={report} navigate={vi.fn()} />);
    expect(screen.getByText(/No layers are configured/)).toBeInTheDocument();
  });
});

describe("DiffView", () => {
  beforeEach(() => {
    fetchFile.mockReset();
  });

  it("renders the report's hunks with semantic annotation", async () => {
    fetchFile.mockImplementation((rev: string, path: string) =>
      Promise.resolve(blob(rev, path, rev === "aaaaaaaaaaaa" ? FEE_BASE : FEE_HEAD)),
    );
    const navigate = vi.fn();
    render(<DiffView report={sampleReport()} url={url({ view: "diff", file: FEE_FILE })} navigate={navigate} />);
    const table = await screen.findByRole("table", { name: /Unified diff/ });
    expect(fetchFile).toHaveBeenCalledWith("aaaaaaaaaaaa", FEE_FILE);
    expect(fetchFile).toHaveBeenCalledWith("bbbbbbbbbbbb", FEE_FILE);
    const removed = [...table.querySelectorAll("tr.diff__del")].map((row) => row.textContent);
    const added = [...table.querySelectorAll("tr.diff__add")].map((row) => row.textContent);
    // Whitespace is significant in source: the text must arrive exactly as written.
    expect(removed).toEqual(["5−    return m.times(1);"]);
    expect(added).toEqual(["5+    var rate = 2;", "6+    return m.times(rate);"]);
    const hunk = within(table).getByLabelText("Hunk 1");
    expect(hunk).toHaveTextContent("@@ −5,1 +5,2 @@");
    expect(hunk).toHaveTextContent("2 tests");
    expect(hunk).toHaveTextContent("covered");
    fireEvent.click(within(hunk).getByRole("button", { name: "StandardFeePolicy.feeFor(Money)" }));
    expect(navigate).toHaveBeenCalledWith({ view: "overview", sel: FEE_IMPL });
  });

  it("renders source as text, never as markup", async () => {
    const hostile = ["<img src=x onerror=alert(1)>", "", "", "", "<script>alert(2)</script>", "", "", ""];
    fetchFile.mockImplementation((rev: string, path: string) => Promise.resolve(blob(rev, path, hostile)));
    const { container } = render(
      <DiffView report={sampleReport()} url={url({ view: "diff", file: FEE_FILE })} navigate={vi.fn()} />,
    );
    await screen.findByRole("table", { name: /Unified diff/ });
    expect(container.querySelector("img, script")).toBeNull();
    expect(screen.getAllByText("<script>alert(2)</script>").length).toBeGreaterThan(0);
  });

  it("falls back to the hunk list when the file cannot be shown", async () => {
    fetchFile.mockImplementation((rev: string, path: string) => Promise.resolve(blob(rev, path, null)));
    render(<DiffView report={sampleReport()} url={url({ view: "diff", file: FEE_FILE })} navigate={vi.fn()} />);
    await waitFor(() => {
      expect(screen.getByText(/Contents not shown \(binary file\)/)).toBeInTheDocument();
    });
    expect(screen.getByLabelText("Hunk 1")).toHaveTextContent("StandardFeePolicy.feeFor(Money)");
  });

  it("reports a failed load instead of an empty diff", async () => {
    fetchFile.mockRejectedValue(new Error("'x' does not exist in revision 'main'"));
    render(<DiffView report={sampleReport()} url={url({ view: "diff", file: FEE_FILE })} navigate={vi.fn()} />);
    expect(await screen.findByRole("alert")).toHaveTextContent("does not exist in revision");
  });
});

describe("OwnersView", () => {
  it("lists likely reviewers as metadata", () => {
    render(<OwnersView report={sampleReport()} navigate={vi.fn()} />);
    expect(screen.getByText(/not authorization/)).toBeInTheDocument();
    expect(screen.getByRole("row", { name: /@acme\/payments 1 0/ })).toBeInTheDocument();
    expect(screen.getByRole("row", { name: new RegExp(MIGRATION.replace(/[.]/g, "\\.")) })).toHaveTextContent("no owner");
  });

  it("says plainly when there is no CODEOWNERS", () => {
    const report: AnalysisReport = sampleReport();
    report.owners = { ...report.owners, source: null, owners: [], files: [] };
    render(<OwnersView report={report} navigate={vi.fn()} />);
    expect(screen.getByText(/No CODEOWNERS file was found/)).toBeInTheDocument();
  });
});

describe("NodeList", () => {
  it("is operable with the keyboard", () => {
    const onSelect = vi.fn();
    const nodes = sampleReport().graph.nodes.map((n) => ({ ...n, label: n.id, matchesQuery: false }));
    render(<NodeList nodes={nodes} selectedId={null} onSelect={onSelect} />);
    const list = screen.getByRole("listbox", { name: "Graph symbols" });
    fireEvent.keyDown(list, { key: "ArrowDown" });
    fireEvent.keyDown(list, { key: "Enter" });
    // Ordered by depth: the changed symbol (depth 0) first, its depth-1 dependent second.
    expect(onSelect).toHaveBeenCalledWith(nodes[1]?.id);
    fireEvent.keyDown(list, { key: "End" });
    fireEvent.keyDown(list, { key: " " });
    expect(onSelect).toHaveBeenLastCalledWith(TEST);
    expect(list).toHaveAttribute("aria-activedescendant");
  });
});

// Keeps the fixture honest: the evidence string names the violation edge's endpoints.
it("fixture evidence names real ids", () => {
  expect(sampleReport().risk.signals[1]?.evidence[0]).toContain(API_ERRORS);
});
