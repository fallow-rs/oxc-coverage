# Karma example

This example runs Karma with `karma-coverage` in headless Chrome. It
instruments two sources:

- `node_modules/lodash/lodash.js`: lodash 4.17.21, a real library of more than 17,000
  lines, loaded as a browser script.
- `src/cart.js`: a small ES module that uses optional chains, `||=`, `??=`, and
  class methods.

## Setup

```js
// karma.conf.js
module.exports = (config) => {
  config.set({
    preprocessors: { 'src/**/*.js': ['coverage'] },
    reporters: ['progress', 'coverage'],
    coverageReporter: {
      instrumenters: { oxc: require('oxc-coverage-instrument/karma') },
      instrumenter: { '**/*.js': 'oxc' },
      instrumenterOptions: { oxc: { compat: 'istanbul' } },
    },
  });
};
```

`karma-coverage` matches the `instrumenter` patterns against the absolute path
of each file. Start each pattern with `**/`.

`compat: 'istanbul'` gives the same statement, function, and branch maps and
the same hit counts as the default instrumenter of `karma-coverage`. Only the
`hash` value and the `_coverageSchema` key differ. Without `compat`, the default profile records more. The
[crate README](../../crates/oxc_coverage_instrument/README.md#differences-from-istanbul-lib-instrument)
lists each difference.

## Run it

```bash
npm --prefix ../../crates/oxc_coverage_instrument_napi run build:debug
npm install
npm run verify
```

Set `CHROME_BIN` when Chrome is not in its usual location.

`npm run verify` does these steps:

1. It runs the tests three times. Each run writes
   `coverage/<name>/coverage-final.json`:
   - `istanbul`: the default instrumenter of `karma-coverage`
     (`istanbul-lib-instrument`).
   - `oxc`: `oxc-coverage-instrument`, default profile.
   - `oxc-strict`: `oxc-coverage-instrument` with `compat: 'istanbul'`.
2. `scripts/compare-coverage.mjs` compares the three files. `oxc-strict` must
   have the same statement, function, and branch maps as `istanbul`, and the
   same hit counts. All other keys must be equal too, except `hash` and
   `_coverageSchema`. `oxc` must have the same statements, functions, and hit
   counts, and it can differ only by the documented extensions.
3. `scripts/preprocessor-path.mjs` runs the preprocessor of `karma-coverage`
   in Node with an input source map and `includeAllSources: true`. The browser
   runs do not use these two paths. It also checks that a relative
   `instrumenter` pattern does not select the adapter.

## Result

| File | Statements | Functions | Branch paths, `istanbul` and `oxc-strict` | Branch paths, `oxc` |
|:--|--:|--:|--:|--:|
| `lodash.js` | 1447/3714 | 190/691 | 551/2835 | 551/2835 |
| `src/cart.js` | 26/27 | 6/7 | 10/11 | 16/17 |

The tests ran the same code in all three runs. In `lodash.js`, the default
profile changes only function names and the positions of an implicit `else`.
lodash has no optional chains and no logical assignment. In `src/cart.js`, the
default profile adds three branch entries: one for `discount?.percent`, one
for `||=`, and one for `??=`.
