const assert = require('node:assert/strict');

const { parseLine, parseList, chunk, loadList } = require('../src/parse');

describe('parse', () => {
  it('parses one line', () => {
    assert.deepEqual(parseLine('sku-1 x2 @12.50 #sale'), {
      sku: 'sku-1',
      quantity: 2,
      price: 12.5,
      tags: ['sale'],
    });
    assert.equal(parseLine('   '), null);
  });

  it('collects errors when not strict', () => {
    const { items, errors } = parseList('a x1 @1\nbroken\n// note');
    assert.equal(items.length, 1);
    assert.equal(errors.length, 1);
  });

  it('chunks', () => {
    assert.deepEqual([...chunk([1, 2, 3], 2)], [[1, 2], [3]]);
  });

  it('loads through a reader', async () => {
    const result = await loadList(async () => 'b x3 @2', 'list.txt');
    assert.equal(result.items[0].quantity, 3);
  });
});
