import { expect, test } from "vitest";
import { formatMoney, money } from "./money";

test("formats with two decimals", () => {
  expect(formatMoney(money(3))).toBe("3.00 EUR");
});
