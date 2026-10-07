import { type Money, money } from "../money";

export type DiscountCode = "NONE" | "WELCOME10";

export function applyDiscount(total: Money, code: DiscountCode): Money {
  if (code === "WELCOME10") {
    return money(total.amount * 0.9, total.currency);
  }
  return total;
}
