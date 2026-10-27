// A small shopping cart. It uses syntax where the default profile of
// oxc-coverage-instrument records more than istanbul-lib-instrument:
// optional chains, logical assignment, and named arrow functions.

export const formatPrice = (cents, currency = 'EUR') => {
  const amount = (cents / 100).toFixed(2);
  return currency === 'EUR' ? `€${amount}` : `${amount} ${currency}`;
};

export class Cart {
  #items = new Map();

  add(product, quantity = 1) {
    if (quantity <= 0) {
      throw new RangeError('quantity must be positive');
    }
    const line = this.#items.get(product.id);
    if (line) {
      line.quantity += quantity;
    } else {
      this.#items.set(product.id, { product, quantity });
    }
    return this;
  }

  remove(productId) {
    return this.#items.delete(productId);
  }

  get size() {
    let count = 0;
    for (const { quantity } of this.#items.values()) count += quantity;
    return count;
  }

  total({ discount } = {}) {
    let sum = 0;
    for (const { product, quantity } of this.#items.values()) {
      sum += product.price * quantity;
    }
    const percent = discount?.percent ?? 0;
    return Math.round(sum * (1 - percent / 100));
  }

  summary(options) {
    const settings = { ...options };
    settings.currency ||= 'EUR';
    settings.separator ??= ', ';
    const lines = [...this.#items.values()].map(
      ({ product, quantity }) => `${quantity} x ${product.name}`,
    );
    return `${lines.join(settings.separator)}: ${formatPrice(this.total(settings), settings.currency)}`;
  }
}
