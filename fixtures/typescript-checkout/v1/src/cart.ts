import type { Money } from "./money";
import type { PriceSource } from "./pricing";

export interface LineItem {
  sku: string;
  unitPrice: number;
  quantity: number;
}

export class Cart {
  private readonly items: LineItem[] = [];

  constructor(private readonly prices: PriceSource) {}

  add(item: LineItem): void {
    this.items.push(item);
  }

  total(): Money {
    return this.prices.quote(this.items);
  }
}
