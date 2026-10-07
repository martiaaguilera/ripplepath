import type { LineItem } from "../cart";
import { type Money, money } from "../money";
import { applyDiscount, type DiscountCode } from "./discount";
import type { PriceSource } from "./price-source";

export class PricingService implements PriceSource {
  constructor(private readonly code: DiscountCode = "NONE") {}

  quote(items: LineItem[]): Money {
    const subtotal = items.reduce((sum, item) => sum + item.unitPrice * item.quantity, 0);
    return applyDiscount(money(subtotal), this.code);
  }
}
