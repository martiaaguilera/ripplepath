import type { Cart } from "../cart";
import { checkoutSummary } from "../checkout/checkout";

export function CartBadge({ cart }: { cart: Cart }) {
  return <span className="badge">{checkoutSummary(cart)}</span>;
}

export function Header({ cart }: { cart: Cart }) {
  return (
    <header>
      <CartBadge cart={cart} />
    </header>
  );
}
