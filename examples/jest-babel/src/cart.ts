export type Item = {
  sku: string;
  price: number;
  quantity?: number;
  tags?: string[];
};

export type Discount =
  | { kind: 'percent'; value: number }
  | { kind: 'fixed'; value: number }
  | { kind: 'none' };

export class Cart {
  private readonly items = new Map<string, Item>();

  constructor(private readonly currency: string = 'EUR') {}

  add(item: Item): this {
    const existing = this.items.get(item.sku);
    if (existing) {
      existing.quantity = (existing.quantity ?? 1) + (item.quantity ?? 1);
    } else {
      this.items.set(item.sku, { ...item, quantity: item.quantity ?? 1 });
    }
    return this;
  }

  remove(sku: string): boolean {
    return this.items.delete(sku);
  }

  get size(): number {
    let count = 0;
    for (const item of this.items.values()) {
      count += item.quantity ?? 0;
    }
    return count;
  }

  subtotal(): number {
    return [...this.items.values()].reduce(
      (sum, item) => sum + item.price * (item.quantity ?? 1),
      0,
    );
  }

  total(discount: Discount = { kind: 'none' }): number {
    const subtotal = this.subtotal();
    switch (discount.kind) {
      case 'percent':
        return round(subtotal * (1 - clamp(discount.value, 0, 100) / 100));
      case 'fixed':
        return round(Math.max(0, subtotal - discount.value));
      default:
        return round(subtotal);
    }
  }

  tagged(tag: string): Item[] {
    return [...this.items.values()].filter((item) => item.tags?.includes(tag) === true);
  }

  format(): string {
    return `${this.total().toFixed(2)} ${this.currency}`;
  }
}

export const clamp = (value: number, min: number, max: number): number =>
  value < min ? min : value > max ? max : value;

export function round(value: number): number {
  return Math.round(value * 100) / 100;
}
