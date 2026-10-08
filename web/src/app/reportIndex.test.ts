import { describe, expect, it } from "vitest";
import { ACCOUNT, FEE_FILE, MIGRATION, sampleReport } from "../test/fixtures";
import { matchToken, signalsNaming } from "./reportIndex";

describe("matchToken", () => {
  const known = new Set([ACCOUNT, "src/A.java"]);
  const accept = (core: string) => known.has(core);

  it("keeps the closing parenthesis that belongs to a method id", () => {
    expect(matchToken(ACCOUNT, accept)).toEqual({ lead: "", core: ACCOUNT, trail: "" });
  });

  it("peels prose punctuation around ids and paths", () => {
    expect(matchToken(`(${ACCOUNT}),`, accept)).toEqual({ lead: "(", core: ACCOUNT, trail: ")," });
    expect(matchToken("(src/A.java)", accept)).toEqual({ lead: "(", core: "src/A.java", trail: ")" });
  });

  it("never matches a partial token", () => {
    expect(matchToken("java:bank.domain.Account", accept)).toBeNull();
    expect(matchToken(`x${ACCOUNT}`, accept)).toBeNull();
  });
});

describe("signalsNaming", () => {
  it("finds scored signals whose evidence names a symbol or a file", () => {
    const report = sampleReport();
    expect(signalsNaming(report, [ACCOUNT])).toEqual(["new_architecture_violation"]);
    expect(signalsNaming(report, [MIGRATION])).toEqual(["migration_changed"]);
    expect(signalsNaming(report, [FEE_FILE])).toEqual([]);
  });
});
