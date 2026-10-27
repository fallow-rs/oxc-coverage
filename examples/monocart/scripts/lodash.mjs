// Loads the real lodash 4.17.21 build from node_modules and runs a fixed
// workload on it. The Istanbul script and the V8 script use the same
// workload, so their coverage describes the same execution.

import { createRequire } from 'node:module';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';

const require = createRequire(import.meta.url);

export const LODASH_PATH = require.resolve('lodash/lodash.js');
export const readLodash = () => readFileSync(LODASH_PATH, 'utf8');

/** Runs `code` in a new context and returns that context. */
export const runInContext = (code, filename) => {
  const context = vm.createContext({});
  vm.runInContext(code, context, { filename });
  return context;
};

/** Calls a fixed set of lodash functions on the `_` of `context`. */
export const exercise = (context) => {
  const _ = context._;
  const users = [
    { user: 'barney', age: 36, active: true, tags: ['a', 'b'] },
    { user: 'fred', age: 40, active: false, tags: ['b'] },
    { user: 'pebbles', age: 1, active: true, tags: [] },
  ];
  _.chunk([1, 2, 3, 4, 5], 2);
  _.compact([0, 1, false, 2, '', 3]);
  _.difference([2, 1], [2, 3]);
  _.flattenDeep([1, [2, [3, [4]], 5]]);
  _.uniqBy([2.1, 1.2, 2.3], Math.floor);
  _.groupBy(users, 'active');
  _.sortBy(users, ['user', 'age']);
  _.orderBy(users, ['age'], ['desc']);
  _.filter(users, { active: true });
  _.find(users, (user) => user.age < 40);
  _.map(users, 'user');
  _.reduce(users, (sum, user) => sum + user.age, 0);
  _.keyBy(users, 'user');
  _.partition(users, 'active');
  _.cloneDeep(users);
  _.isEqual(users[0], { ...users[0] });
  _.merge({ a: [{ b: 2 }] }, { a: [{ c: 3 }] });
  _.get({ a: [{ b: { c: 3 } }] }, 'a[0].b.c');
  _.set({}, 'x[0].y.z', 5);
  _.pick({ a: 1, b: '2', c: 3 }, ['a', 'c']);
  _.omit({ a: 1, b: '2', c: 3 }, ['a']);
  _.defaultsDeep({ a: { b: 2 } }, { a: { b: 1, c: 3 } });
  _.camelCase('Foo Bar');
  _.kebabCase('fooBar');
  _.snakeCase('Foo Bar');
  _.capitalize('FRED');
  _.escape('fred, barney, & pebbles');
  _.template('hello <%= user %>!')({ user: 'fred' });
  _.truncate('hi-diddly-ho there, neighborino', { length: 24, separator: ' ' });
  _.padStart('abc', 6, '_-');
  _.words('fred, barney, & pebbles');
  _.range(0, 20, 5);
  _.random(1, 5);
  _.clamp(-10, -5, 5);
  _.sum([4, 2, 8, 6]);
  _.meanBy(users, 'age');
  _.debounce(() => undefined, 10).cancel();
  _.memoize((value) => value * 2)(4);
  _.curry((a, b, c) => a + b + c)(1)(2)(3);
  _.flow([_.add, (value) => value * value])(1, 2);
  _.chain(users).filter('active').map('user').head().value();
  _.zipObject(['a', 'b'], [1, 2]);
  _.intersectionBy([2.1, 1.2], [2.3, 3.4], Math.floor);
  _.isPlainObject({ a: 1 });
  _.isEmpty([]);
  _.toPairs({ a: 1, b: 2 });
  _.invert({ a: 1, b: 2 });
  _.times(3, String);
};
