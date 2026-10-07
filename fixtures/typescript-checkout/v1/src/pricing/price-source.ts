import type { LineItem } from "../cart";
import type { Money } from "../money";

export interface PriceSource {
  quote(items: LineItem[]): Money;
}
