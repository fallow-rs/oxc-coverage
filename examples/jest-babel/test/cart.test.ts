import assert from 'node:assert/strict';

import { Cart, clamp } from '../src/cart';

describe('Cart', () => {
  it('adds and merges items', () => {
    const cart = new Cart();
    cart.add({ sku: 'a', price: 2 }).add({ sku: 'a', price: 2, quantity: 2 });
    cart.add({ sku: 'b', price: 1.5, tags: ['sale'] });
    assert.equal(cart.size, 4);
    assert.equal(cart.subtotal(), 7.5);
  });

  it('applies a percent discount', () => {
    const cart = new Cart('USD').add({ sku: 'a', price: 10, quantity: 3 });
    assert.equal(cart.total({ kind: 'percent', value: 10 }), 27);
    assert.equal(cart.format(), '30.00 USD');
  });

  it('filters by tag', () => {
    const cart = new Cart().add({ sku: 'a', price: 1, tags: ['new'] }).add({ sku: 'b', price: 1 });
    assert.deepEqual(
      cart.tagged('new').map((item) => item.sku),
      ['a'],
    );
    assert.equal(cart.remove('b'), true);
  });

  it('clamps', () => {
    assert.equal(clamp(-1, 0, 10), 0);
    assert.equal(clamp(5, 0, 10), 5);
  });
});
