import { describe, expect, it } from "vitest";
import { money } from "../money";
import { applyDiscount } from "./discount";

describe("applyDiscount", () => {
  it("takes 10% off with WELCOME10", () => {
    expect(applyDiscount(money(100), "WELCOME10").amount).toBe(90);
  });

  it("leaves totals alone without a code", () => {
    expect(applyDiscount(money(100), "NONE").amount).toBe(100);
  });
});

describe("SPRING20", () => {
  it("takes 20% off", () => {
    expect(applyDiscount(money(100), "SPRING20").amount).toBe(80);
  });
});
