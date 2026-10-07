import { Cart } from "../cart";
import { formatMoney } from "../money";

export function checkoutSummary(cart: Cart): string {
  return formatMoney(cart.total());
}
