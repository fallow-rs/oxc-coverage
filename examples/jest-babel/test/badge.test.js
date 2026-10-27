const assert = require('node:assert/strict');

const { Badge } = require('../src/badge');

describe('badge', () => {
  it('shows an empty badge', () => {
    assert.deepEqual(Badge({ count: 0 }).props, { className: 'badge empty' });
  });

  it('caps the count', () => {
    assert.equal(Badge({ count: 120 }).children[0], '99+');
  });
});
