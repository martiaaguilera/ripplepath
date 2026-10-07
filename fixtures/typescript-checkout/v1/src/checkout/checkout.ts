import { Cart } from "../cart";
import { formatMoney } from "../money";
import { PricingService } from "../pricing";

export function checkoutSummary(cart: Cart): string {
  return formatMoney(cart.total());
}

export function newCart(code?: "WELCOME10"): Cart {
  return new Cart(new PricingService(code));
}
