import { beforeEach, describe, expect, it } from "vitest";
import { Cart } from "./cart";
import { PricingService } from "./pricing";

describe("Cart", () => {
  let cart: Cart;

  beforeEach(() => {
    cart = new Cart(new PricingService());
  });

  it("totals line items", () => {
    cart.add({ sku: "a", unitPrice: 2, quantity: 3 });
    expect(cart.total().amount).toBe(6);
  });
});
