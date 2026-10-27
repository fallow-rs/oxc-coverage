# monocart-coverage-reports

[monocart-coverage-reports](https://github.com/cenfun/monocart-coverage-reports)
(MCR) is a coverage reporter. It accepts two input formats: V8 coverage (an
array) and Istanbul coverage (an object). The Istanbul data from
`oxc-coverage-instrument` goes into MCR without a change on either side.

This example shows two inputs:

| Script | Input | Result |
|:--|:--|:--|
| `npm run coverage` then `npm run report` | `coverage/coverage-final.json` from a Vitest run with the `oxc-coverage-instrument` instrumenter | MCR reports in `coverage/monocart` |
| `npm run lodash` | The real lodash 4.17.21 build, instrumented with `instrument()` and run under a fixed workload | MCR reports in `coverage/lodash-istanbul` and `coverage/lodash-default` |

Each script compares the MCR totals with `summarizeCoverage()` of
`oxc-coverage-instrument`. The script exits with code 1 when a covered count or
a total count differs. The Vitest run uses the default profile, so the data
also has `optional-chain` and `??` branch entries. The lodash script runs both
the strict `istanbul` profile and the default profile.

## Run it

Build the native binding first (see the repository README), then:

```bash
npm install
npm run coverage
npm run verify
npm run v8-compare
```

CI runs `coverage` and `verify`. It does not run `v8-compare`, because its
times depend on the machine.

## Use it in your own project

Give the coverage map to `add()`. MCR merges it with `istanbul-lib-coverage`
and writes the reports with `istanbul-reports`:

```js
import { readFileSync } from 'node:fs';
import { MCR } from 'monocart-coverage-reports';

const mcr = MCR({
  outputDir: 'coverage/monocart',
  reports: ['html', 'console-summary', 'lcovonly'],
});
await mcr.add(JSON.parse(readFileSync('coverage/coverage-final.json', 'utf8')));
await mcr.generate();
```

Use `add()`, not the `inputDir` option or the `--inputDir` CLI flag. Those read
the MCR raw files (`coverage-*.json` with an `id`, a `type`, and a `data`
field). A plain `coverage-final.json` does not have that shape.

## What MCR does not use from this data

- The MCR `v8` report (its own HTML UI) needs V8 data. With Istanbul input,
  MCR writes the Istanbul reports (`html`, `lcov`, `json-summary`, and the
  others) and its `console-summary`, `markdown-summary`, and `raw` reports.
- MCR ignores the `x_`-prefixed fields, for example `x_fallow_functionMap`.
  The totals do not change.

## V8 input

`npm run v8-compare` collects the V8 precise coverage of the same lodash
workload. It then converts that V8 data twice: once with the converter inside
MCR and once with `v8ToIstanbul()` of `oxc-coverage-instrument`. It prints both
totals next to the totals of the Istanbul instrumentation, and the median time
of each converter. The script does not fail on a difference. The two
converters count V8 data differently:

- MCR finds the branches with an acorn walk. When a branch arm has no V8 block
  range, MCR gives the arm the parent count minus the counts of the other
  arms.
- `v8ToIstanbul()` gives such an arm `0`. This is true for ternary arms,
  the right side of `&&`, `||`, and `??`, and default arguments. The result
  is a lower branch percentage than the real one.
- MCR counts lines from the V8 byte ranges over all code lines. Istanbul and
  `v8ToIstanbul()` count the start lines of statements.

One result, on an Apple M5 with Node.js 24.20 and the release build of the
native binding (median of 9 runs; covered/total):

| Metric | Istanbul instrumentation | `v8ToIstanbul()` | MCR converter |
|:--|--:|--:|--:|
| Statements | 2084/3714 (56.11%) | 2097/3714 (56.46%) | 1970/3734 (52.76%) |
| Branches | 948/2835 (33.44%) | 260/2835 (9.17%) | 950/2832 (33.55%) |
| Functions | 320/691 (46.31%) | 320/691 (46.31%) | 320/691 (46.31%) |
| Lines | 2083/3706 (56.21%) | 2096/3706 (56.56%) | 3332/6663 (50.01%) |
| Time | | 9 ms to 10 ms | 39 ms to 45 ms |

The MCR time is `add()` plus `generate()` with the `json` report, so it
contains more work than the conversion. `v8ToIstanbul()` is faster, and its
statement and function totals are closer to the instrumentation. Its branch
total is far too low, so it is not a replacement for the MCR converter
today. MCR also uses its own V8 data (bytes and lines) for its `v8` report,
which `v8ToIstanbul()` does not give.

Use a release build for the times. With a debug build of the binding,
`v8ToIstanbul()` took more than 60 times longer on the same input.
