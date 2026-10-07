export interface Money {
  amount: number;
  currency: string;
}

export function money(amount: number, currency = "EUR"): Money {
  return { amount, currency };
}

export function formatMoney(value: Money): string {
  return `${value.amount.toFixed(2)} ${value.currency}`;
}
