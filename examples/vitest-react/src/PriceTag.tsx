export interface PriceTagProps {
  amount: number;
  currency?: string;
  discount?: number;
}

export function formatAmount(amount: number, currency = 'EUR'): string {
  return new Intl.NumberFormat('en-US', { style: 'currency', currency }).format(amount);
}

export function PriceTag({ amount, currency = 'EUR', discount }: PriceTagProps) {
  const final = discount ? amount * (1 - discount) : amount;
  return (
    <span className={discount ? 'price price--sale' : 'price'}>
      {discount ? <s>{formatAmount(amount, currency)}</s> : null}
      {formatAmount(final, currency)}
    </span>
  );
}
