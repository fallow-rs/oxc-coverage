// Tests that run a part of lodash, so its coverage has counts above zero
// and paths that no test runs.
const _ = globalThis._;

describe('lodash', () => {
  it('chunks and flattens arrays', () => {
    expect(_.chunk([1, 2, 3, 4, 5], 2)).toEqual([[1, 2], [3, 4], [5]]);
    expect(_.flattenDeep([1, [2, [3, [4]]]])).toEqual([1, 2, 3, 4]);
  });

  it('groups and sorts collections', () => {
    const people = [
      { name: 'Ann', age: 34 },
      { name: 'Bob', age: 27 },
      { name: 'Cem', age: 34 },
    ];
    expect(Object.keys(_.groupBy(people, 'age'))).toEqual(['27', '34']);
    expect(_.sortBy(people, ['age', 'name']).map((p) => p.name)).toEqual(['Bob', 'Ann', 'Cem']);
  });

  it('reads and writes deep paths', () => {
    const object = {};
    _.set(object, 'a[0].b.c', 4);
    expect(_.get(object, 'a[0].b.c')).toBe(4);
    expect(_.get(object, 'a.x', 'default')).toBe('default');
  });

  it('merges and compares objects', () => {
    const merged = _.merge({ a: [{ b: 2 }] }, { a: [{ c: 3 }] });
    expect(_.isEqual(merged, { a: [{ b: 2, c: 3 }] })).toBe(true);
  });

  it('formats strings', () => {
    expect(_.camelCase('Foo Bar')).toBe('fooBar');
    expect(_.template('hello <%= user %>!')({ user: 'karma' })).toBe('hello karma!');
  });
});
