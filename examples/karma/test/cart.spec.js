import { Cart, formatPrice } from '../src/cart.js';

const apple = { id: 'apple', name: 'Apple', price: 50 };
const pear = { id: 'pear', name: 'Pear', price: 75 };

describe('Cart', () => {
  it('adds products and counts them', () => {
    const cart = new Cart().add(apple).add(apple, 2).add(pear);
    expect(cart.size).toBe(4);
  });

  it('rejects a quantity that is not positive', () => {
    expect(() => new Cart().add(apple, 0)).toThrowError(RangeError);
  });

  it('calculates the total with a discount', () => {
    const cart = new Cart().add(apple, 4);
    expect(cart.total()).toBe(200);
    expect(cart.total({ discount: { percent: 10 } })).toBe(180);
  });

  it('writes a summary', () => {
    const cart = new Cart().add(apple).add(pear);
    expect(cart.summary()).toBe('1 x Apple, 1 x Pear: €1.25');
  });

  it('formats a price in another currency', () => {
    expect(formatPrice(1999, 'USD')).toBe('19.99 USD');
  });
});
