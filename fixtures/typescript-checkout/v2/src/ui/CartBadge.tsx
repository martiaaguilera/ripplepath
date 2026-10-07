import type { Cart } from "../cart";
import { checkoutSummary } from "../checkout/summary";

export function CartBadge({ cart }: { cart: Cart }) {
  return <span className="badge">{checkoutSummary(cart)}</span>;
}

export function Header({ cart }: { cart: Cart }) {
  // Untyped on purpose: Ripplepath must report this call as unresolved, not guess a target.
  const legacy: any = cart;
  return (
    <header title={legacy.total().currency}>
      <CartBadge cart={cart} />
    </header>
  );
}
