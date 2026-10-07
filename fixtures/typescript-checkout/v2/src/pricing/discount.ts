import { type Money, money } from "../money";

export type DiscountCode = "NONE" | "WELCOME10" | "SPRING20";

const RATES: Record<DiscountCode, number> = { NONE: 0, WELCOME10: 0.1, SPRING20: 0.2 };

export function applyDiscount(total: Money, code: DiscountCode): Money {
  return money(total.amount * (1 - RATES[code]), total.currency);
}
