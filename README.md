# oxc-coverage

[![CI](https://github.com/fallow-rs/oxc-coverage/actions/workflows/ci.yml/badge.svg)](https://github.com/fallow-rs/oxc-coverage/actions/workflows/ci.yml)
[![Coverage](https://img.shields.io/endpoint?url=https://raw.githubusercontent.com/fallow-rs/oxc-coverage/badges/coverage.json)](https://github.com/fallow-rs/oxc-coverage/actions/workflows/coverage.yml)
[![Crates.io](https://img.shields.io/crates/v/oxc_coverage_instrument.svg)](https://crates.io/crates/oxc_coverage_instrument)
[![npm](https://img.shields.io/npm/v/oxc-coverage-instrument.svg)](https://www.npmjs.com/package/oxc-coverage-instrument)
[![docs.rs](https://docs.rs/oxc_coverage_instrument/badge.svg)](https://docs.rs/oxc_coverage_instrument)
[![MIT License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

Faster Istanbul coverage for JavaScript and TypeScript. It replaces the slow
step of Istanbul coverage, inserting the coverage counters into your code, with
Rust code built on the [Oxc](https://oxc.rs) parser. The coverage numbers stay
the same.

> **Unofficial.** This is an independent community project that builds on Oxc
> crates. It is not part of, affiliated with, or endorsed by the Oxc project.

## Use it with Vitest

You need Vitest 4.1.5 or newer.

**1. Install** the Istanbul coverage provider and this package:

```bash
npm install -D @vitest/coverage-istanbul oxc-coverage-instrument
```

**2. Add the instrumenter** to the coverage config in `vitest.config.ts`. This
is the only change. `compat: 'istanbul'` keeps the coverage numbers equal to
those of Vitest's own instrumenter:

```ts
import { defineConfig } from 'vitest/config';
import { createOxcInstrumenter } from 'oxc-coverage-instrument/vitest';

export default defineConfig({
  test: {
    coverage: {
      provider: 'istanbul',
      instrumenter: (options) => createOxcInstrumenter({ ...options, compat: 'istanbul' }),
    },
  },
});
```

**3. Run your tests** with coverage, as before:

```bash
npx vitest run --coverage
```

TypeScript, TSX, and JSX work without extra setup: Vite compiles them as usual,
and the coverage points at your `.ts` and `.tsx` lines. Your other coverage
settings (`include`, `exclude`, `reporter`, `thresholds`, Vitest projects,
browser mode) stay as they are.

**4. Optional: see more untested cases.** Remove `compat: 'istanbul'` to use
the default profile. It also counts the branches of optional chains, `??=`,
`||=`, and `&&=`, and gives anonymous functions the name that they get at
runtime. Your branch percentage can then change, so check your branch
thresholds (see [More detail with the default profile](#more-detail-with-the-default-profile)).

The [examples](examples/) show React, a monorepo with Vitest projects, and CI
reports (`lcov`, `cobertura`, coverage thresholds).

## Is it worth it?

Vitest has two coverage providers. `v8` reads the coverage that the V8 engine
collects. `istanbul` inserts counters into your code before the tests run; that
works in every browser that Vitest's browser mode supports, and gives Istanbul's
exact statement, branch, and function counts. This package replaces the step
that inserts the counters.

The table shows that step on the test suites of four open-source projects,
with the setup above (`compat: 'istanbul'`). It was measured on a GitHub-hosted
runner with a cold Vitest cache, as in CI
([method and full results](examples/vitest-real-projects/)). The instrumenter
columns give the time of all Vitest workers together, for all source files of
the run. The last column gives the time of the whole `vitest run`:

| Project | Files instrumented | Vitest's instrumenter | `oxc-coverage-instrument` | Speedup | Whole run |
|:--|--:|--:|--:|--:|--:|
| [h3](https://github.com/unjs/h3) (HTTP framework) | 81 | 1.5 s | 0.1 s | **13.4x** | 19.4 s → 17.3 s |
| [hono](https://github.com/honojs/hono) (web framework) | 636 | 11.8 s | 0.9 s | **13.3x** | 29.4 s → 26.3 s |
| [date-fns](https://github.com/date-fns/date-fns) (date library) | 1061 | 4.0 s | 0.5 s | **7.5x** | 37.4 s → 35.1 s |
| [es-toolkit](https://github.com/toss/es-toolkit) (utility library) | 802 | 5.7 s | 1.0 s | **5.7x** | 197.6 s → 194.3 s |

The times are rounded; the speedup comes from the unrounded times.

- **The coverage numbers stay the same.** The covered and total counts of
  lines, statements, functions, and branches are equal to those of Vitest's own
  instrumenter in all four projects.
- **The whole test run gains 2.1 s to 3.3 s.** That is 11% of the run for h3
  and hono, 6% for date-fns, and 2% for es-toolkit. The workers instrument
  files at the same time, so the time that you save is less than the sum in
  the table. The rest of the run (the tests themselves, the coverage
  collection, and the reports) does not change.
- **Local reruns gain less.** Vitest 5 caches the instrumented files on disk,
  so a rerun with a warm cache skips this step.

### More detail with the default profile

The numbers above use the strict profile (`compat: 'istanbul'`), which gives
the same coverage data as Vitest's instrumenter. The default profile, without
`compat`, records more. It adds a branch entry for each optional chain
(`a?.b`) and each `??=`, `||=`, and `&&=`. It also gives anonymous functions the
name that they get at runtime: the report shows `const onRequest = () => {}`
as `onRequest`, not as `(anonymous_12)`. A branch entry has two or more
paths, for example the two sides of `a ?? b`. On the same four projects:

| Project | Branch entries | Branch paths that no test ran | Functions without a name | Branch coverage |
|:--|--:|--:|--:|--:|
| hono | 2143 → 2375 | 296 → 385 | 1055 → 414 | 93.3% → 92.1% |
| h3 | 1302 → 1491 | 195 → 276 | 268 → 157 | 92.7% → 91.0% |
| date-fns | 776 → 1064 | 211 → 254 | 252 → 45 | 88.4% → 89.4% |
| es-toolkit | 1609 → 1668 | 54 → 68 | 332 → 274 | 98.4% → 98.0% |

Each extra untested path is a case that your tests do not check, for example
`user?.name` when `user` is `null`. The default profile shows you where these
cases are. Your branch percentage changes by up to 1.7 points, up or
down, so check your branch thresholds when you switch.

Choose:

| You use now | Then |
|:--|:--|
| `istanbul` | Add the `instrumenter` line with `compat: 'istanbul'`. Same numbers, less time. |
| `v8` and you are happy | Stay on `v8`. Switch when you need coverage in Firefox or WebKit, or Istanbul's exact counts. |
| `babel-plugin-istanbul` or `vite-plugin-istanbul` | Use `instrument()` from the npm package, or the `instrumenter` option of `vite-plugin-istanbul` 9 and newer. See the [npm package README](crates/oxc_coverage_instrument_napi/README.md). |
| Karma with `karma-coverage` | Register `oxc-coverage-instrument/karma` in `coverageReporter.instrumenters`. See [Karma](#karma). |
| `@storybook/addon-coverage` (Vite builder) | Give `istanbul.instrumenter` to the addon and make it use `vite-plugin-istanbul` 9. See [`examples/storybook-vite`](examples/storybook-vite/). The webpack builder does not accept an instrumenter yet. |
| Jest | This package has no Jest transformer. Jest keeps `babel-plugin-istanbul`. |

## The parts

Istanbul coverage has four steps and one data format. This project has one
part for each, and a sixth part for coverage from the V8 engine:

```
your code ─► 1 Instrument ─► tests run ─► 2 Coverage data ─► 3 Remap ─► 4 Summary ─► 5 Reports
                        V8 engine coverage ─► 6 V8 conversion ─┘
```

With the Vitest setup above, Vitest uses part 1 only. Vitest does steps 2 to 5
with its own Istanbul packages. Those steps are fast: on the four projects
above they take 0.9 s to 3.7 s per run, and the remap and the reports take
0.1 s to 1.1 s of that
([steps per project](examples/vitest-real-projects/#steps-after-the-test-run)).
With `oxc-coverage-instrument` they take about the same time, a little more
in three of the four projects. Vitest can also load a custom coverage provider
or a custom reporter, but this package does not ship one. Parts 2 to 6 are for
your own scripts, for example a CI step that writes the reports, and for tools
that you build.

| Part | What it does | npm function | Rust crate | Replaces |
|:--|:--|:--|:--|:--|
| 1. Instrument | Adds a counter to each statement, function, and branch. | `instrument()`, `createOxcInstrumenter()` | [`oxc_coverage_instrument`](crates/oxc_coverage_instrument/README.md) | `istanbul-lib-instrument`, `babel-plugin-istanbul`, `swc-plugin-coverage-instrument` |
| 2. Coverage data | Sets the format of the counts: `coverage-final.json`. | plain JSON | [`oxc_coverage_types`](crates/oxc_coverage_types/README.md) | the data model of `istanbul-lib-coverage`, without its merge |
| 3. Remap | Moves the positions from the built JavaScript back to your source files. | `remapCoverageMap()` | [`oxc_coverage_source_maps`](crates/oxc_coverage_source_maps/README.md) | `istanbul-lib-source-maps` |
| 4. Summary | Calculates the totals and the percentages, per file and for the project. | `summarizeCoverage()` | [`oxc_coverage_report`](crates/oxc_coverage_report/README.md) | the summaries of `istanbul-lib-coverage` and `istanbul-lib-report` |
| 5. Reports | Writes `text`, `lcov`, `cobertura`, `html`, and other formats. | `createReport()`, `writeHtmlReport()` | [`oxc_coverage_reports`](crates/oxc_coverage_reports/README.md) | a part of `istanbul-reports` |
| 6. V8 conversion | Turns the coverage that V8 records into the format of part 2. | `v8ToIstanbul()` | [`oxc_coverage_v8`](crates/oxc_coverage_v8/README.md) | `ast-v8-to-istanbul`; more detail than `v8-to-istanbul` |

All npm functions are in the `oxc-coverage-instrument` package. They take and
return the coverage data as a JSON string.

In the snippets below, `coverageJson` is a coverage map: one entry per file,
keyed by the file path. Vitest writes it to `coverage/coverage-final.json` when
the `json` reporter is on (it is on by default):

```javascript
import { readFileSync } from 'node:fs';

const coverageJson = readFileSync('coverage/coverage-final.json', 'utf8');
```

`instrument()` and `v8ToIstanbul()` return the data of one file, not a map.
Put it in a map before you pass it on:
`JSON.stringify({ [file.path]: file })`, where `file` is the parsed result.

### 1. Instrument

**What it does.** It reads a source file and writes a copy with a counter on
each statement, function, and branch. When the tests run the copy, each counter
adds one each time its code runs.

**Why use it.** This is the slowest step of Istanbul coverage. In Vitest it is
5.7x to 13.4x faster than Vitest's own instrumenter (see
[Is it worth it?](#is-it-worth-it)).

**How.** In Vitest, use the setup at the top of this README. In
`vite-plugin-istanbul` 9 and newer, pass `createOxcInstrumenter()` as its
`instrumenter` option. In Storybook with the Vite builder, give the same
instrumenter to `@storybook/addon-coverage` (see
[`examples/storybook-vite`](examples/storybook-vite/)). In your own build
step or plugin, call `instrument()`:

```javascript
import { instrument } from 'oxc-coverage-instrument';

const result = instrument(source, 'src/app.ts', { sourceMap: true, stripTypescript: true });
// result.code: the JavaScript code with the counters
// result.coverageMap: the coverage data of this file, as JSON, with all counts at 0
// result.sourceMap: the source map of result.code
```

When the code runs, the counters go into `globalThis.__coverage__`, keyed by the
file path. That object is the coverage map that the other parts read.

#### Karma

`karma-coverage` 2 accepts a custom instrumenter. Register
`oxc-coverage-instrument/karma` and select it for your files:

```javascript
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

`karma-coverage` matches the `instrumenter` patterns against the absolute file
path, so start each pattern with `**/`. With `compat: 'istanbul'`, the
statement, function, and branch maps and the hit counts are the same as with
the default instrumenter of `karma-coverage`. The [Karma example](examples/karma/) runs both on lodash in
headless Chrome and compares the `coverage-final.json` files.

`stripTypescript: true` removes the TypeScript types. Without it, the output
keeps the types, and plain Node.js cannot run it. JSX stays JSX either way, so
compile it after this step. The Vitest adapter strips the types for you. The
[npm package README](crates/oxc_coverage_instrument_napi/README.md) lists all
options.

### 2. Coverage data

**What it does.** It is the format of the result. For each file it records each
statement, function, and branch, its position, and how many times it ran. This
is the `coverage-final.json` that Vitest's `json` reporter writes.

**Why use it.** Every part reads and writes this format, and it is the same as
Istanbul's. nyc and the other Istanbul tools read the output of this project,
and this project reads their output.

**How.** In JavaScript it is plain JSON: `JSON.parse()` it. In Rust, use the
types of `oxc_coverage_types` (`FileCoverage`, `CoverageMap`). The npm package
has no function to merge two coverage files.

### 3. Remap

**What it does.** Your tests often run built JavaScript, not your TypeScript
source. The counts then point at lines of the built file. A source map tells
which built line comes from which source line. This part uses the source map
to move each position back to your `.ts` or `.tsx` file.

**Why use it.** You need it when you instrument built output, for example a
bundle, and want the coverage on your source files. Vitest remaps on its own.

**How.** Each file entry of the coverage data can hold its source map in
`inputSourceMap`. `remapCoverageMap()` uses it:

```javascript
import { remapCoverageMap } from 'oxc-coverage-instrument';

const remapped = remapCoverageMap(coverageJson);
```

The file path in the result comes from `sources` in the source map, so set
`sources` (or `sourceRoot`) to the path that you want in the report. When the
maps are separate files, read them first and pass them to
`remapCoverageMapWithLoader(coverageJson, { [filePath]: mapJson })`. To skip
this step, pass the source map to `instrument()` as `inputSourceMap` and set
`composeInputSourceMap: true`. The coverage data then points at the source from
the start.

### 4. Summary

**What it does.** It counts the covered and total lines, statements, functions,
and branches, and calculates the percentages. It does this per file and for the
whole project.

**Why use it.** Use it for a coverage threshold in CI, a badge, or a comment on
a pull request. It is about as fast as `istanbul-lib-coverage`; the gain is
that the same package also writes the reports. The command line does the same
check with `report --fail-under 80`, which exits with code 2 under the
threshold.

**How.**

```javascript
import { summarizeCoverage } from 'oxc-coverage-instrument';

const { total, files } = summarizeCoverage(coverageJson);
if (total.lines.pct < 80) process.exitCode = 1;
```

### 5. Reports

**What it does.** It writes the coverage in the formats that people and tools
read:

| Format | For |
|:--|:--|
| `text`, `text-summary` | The table in your terminal. |
| `json-summary` | Scripts and badges. |
| `lcov` | Codecov, Coveralls, SonarQube, and editor plugins. Writes `lcov.info`. |
| `cobertura` | GitLab, Azure DevOps, and Jenkins. |
| `html` | People: a page per file that marks the lines that did not run. |

It does not write `clover`, `json`, `teamcity`, `text-lcov`, or `html-spa`.

**Why use it.** One script writes all the formats from one
`coverage-final.json`. The `lcov` and `cobertura` reports are 2.6x to 2.9x
faster than `istanbul-reports`. The `text` reports take about the same time.
The `html` report is about 32 times slower, because it highlights the source
code when it writes the pages (see the [benchmarks](#benchmarks-per-component)).

**How.**

```javascript
import { writeFileSync } from 'node:fs';
import { createReport, writeHtmlReport } from 'oxc-coverage-instrument';

writeFileSync('coverage/lcov.info', createReport(coverageJson, 'lcov', { rootDir: process.cwd() }));
writeHtmlReport(coverageJson, 'coverage/html', { rootDir: process.cwd() });
```

`rootDir` makes the paths in the reports relative to your project. The
[`vitest-ci-reports`](examples/vitest-ci-reports/) example writes `lcov`,
`cobertura`, and `html` after a Vitest run, and fails the run under a
threshold. The WebAssembly build has no `html` report.

### 6. V8 conversion

**What it does.** This is a different way to get coverage. The V8 engine in
Node.js and Chrome can record which byte ranges of a script ran, with no
counters in the code. This part turns that record into the coverage data of
part 2, with statements, functions, and branches from the source code.

**Why use it.** Use it when you collect V8 coverage yourself, for example with
`node:inspector` or with Playwright's `page.coverage`, and want Istanbul data and
reports. Vitest's `v8` provider does this step on its own.

**How.** Pass the script source and the `functions` array that V8 gives for
that script:

```javascript
import { v8ToIstanbul } from 'oxc-coverage-instrument';

const file = JSON.parse(v8ToIstanbul(source, 'src/app.js', JSON.stringify(script.functions)));
const coverageJson = JSON.stringify({ [file.path]: file });
```

V8 does not record each branch path. When a count is the same as the count of
the code around it, V8 removes that range. A branch path with no range of its
own therefore gets a count from the code around it:

- A ternary path, or the implicit `else` of an `if`, gets the count of the
  code where the branch starts, minus the counts of the other paths.
- An `if` body, a `switch` case, an operand of `&&`, `||` or `??`, and a
  default value get the count of the code around them.

V8 has no range for a default value, so a default parameter gets the call
count of its function, not the number of calls that used the default. An
optional chain (`?.`) path with no range gets 0, and so does the right side
of `||=`, `&&=` and `??=`. When an `istanbul ignore` comment removes one path
of a ternary or an `if`, the other path does not take the remainder.

On the lodash 4.17.21 build, this rule gives a branch total within 1% of the
Istanbul instrumentation of the same workload.

### Rust and the command line

Each part is a separate Rust crate, so a Rust tool takes only the parts it
needs:

```toml
[dependencies]
oxc_coverage_instrument = "0.18"
```

```rust
use oxc_coverage_instrument::{instrument, InstrumentOptions};

let source = "function add(a, b) { return a + b; }";
let result = instrument(source, "add.js", &InstrumentOptions::default()).unwrap();

assert_eq!(result.coverage_map.fn_map["0"].name, "add");
println!("{}", result.code);
```

The `oxc-coverage-instrument` command instruments files and writes reports. It
is not on crates.io, so install it from Git. It keeps TypeScript types in its
output:

```bash
cargo install --git https://github.com/fallow-rs/oxc-coverage oxc_coverage_instrument_cli
oxc-coverage-instrument src/app.js -o dist/app.js --source-map
oxc-coverage-instrument report --format text coverage-final.json
```

### How this compares with Vitest's Istanbul packages

Vitest maintains forks of the Istanbul packages in
[`vitest-dev/istanbuljs`](https://github.com/vitest-dev/istanbuljs).
`@vitest/istanbul-lib-instrument` keeps the Istanbul API and replaces Babel
with the Oxc parser of `rolldown` and with `magic-string`. The forks of the
other packages are JavaScript ports of the Istanbul packages. This project
replaces all of them with Rust code behind a native or WebAssembly binding.

The instrumenter is checked against `istanbul-lib-instrument` on a shared
fixture corpus, by count and byte for byte. This repository was previously
named `oxc-coverage-instrument`; that name now belongs to the npm package, the
instrumenter crate, and the CLI binary.

## Benchmarks per component

Each part measured on its own, against the package it replaces. Times on the
five pinned library builds of `scripts/real-world-corpus.json`, in one Node.js
process, as the median of 7 runs after 2 warm-up runs, from
`scripts/replacement-benchmarks.mjs`. Measured on 2026-10-24 on an Apple M5
(macOS, Darwin 25.5), Node.js 24.20.0, with the release build of the native
binding. The time is the sum over the five files, in milliseconds. A ratio
above 1 means that `oxc-coverage-instrument` is faster.

| Stage | Package | Package (ms) | `oxc-coverage-instrument` (ms) | Ratio |
|:--|:--|--:|--:|--:|
| Instrument | `istanbul-lib-instrument` 6.0.3 | 1898 | 64 | 29.7x |
| Instrument | `@vitest/istanbul-lib-instrument` 2.0.0 | 411 | 64 | 6.4x |
| V8 to Istanbul | `ast-v8-to-istanbul` 1.0.7, with `oxc-parser` | 341 | 88 | 3.9x |
| V8 to Istanbul | `v8-to-istanbul` 9.3.0 | 33 | 88 | 0.4x |
| Source-map remap | `istanbul-lib-source-maps` 5.0.6 | 180 | 139 | 1.3x |
| Source-map remap | `@vitest/istanbul-lib-source-maps` 2.0.0 | 168 | 139 | 1.2x |
| Summary | `istanbul-lib-coverage` 3.2.2 | 39 | 44 | 0.9x |
| `text` report | `istanbul-lib-report` 3.0.1 + `istanbul-reports` 3.2.0 | 44 | 41 | 1.1x |
| `text-summary` report | same | 33 | 35 | 0.9x |
| `json-summary` report | same | 32 | 36 | 0.9x |
| `lcov` report | same, as `lcovonly` | 131 | 45 | 2.9x |
| `cobertura` report | same | 157 | 61 | 2.6x |
| `html` report | same | 128 | 4112 | 0.03x |

How to read the slower rows:

- `v8-to-istanbul` does less work. It gives one statement per source line and
  no branch locations from the AST: 51876 statements and 123 branches for
  three.js, where `oxc-coverage-instrument` and `ast-v8-to-istanbul` give the 20247 statements and
  4018 branches that the instrumenter records.
- The summary and the `text`, `text-summary`, and `json-summary` reports spend
  most of their time in parsing the coverage JSON, on both sides.
- The `html` report highlights each source file in Rust when it writes the
  report. The Istanbul `html` report highlights in the browser.

The Vitest fork took the same time as the Istanbul packages for the summary and
the reports, within about 3%, so the table leaves it out for these rows.

`scripts/benchmark-comparison.sh` compares the instrumenter with
`babel-plugin-istanbul` and `swc-plugin-coverage-instrument`. It runs each tool
and file in its own Node.js process, as the median of 5 runs, and installs the
current release of each tool. Measured on the same machine and day:

| Package | Package (ms) | `oxc-coverage-instrument` (ms) | Ratio |
|:--|--:|--:|--:|
| `babel-plugin-istanbul` 8.0.0 | 571 | 67 | 8.6x |
| `swc-plugin-coverage-instrument` 0.0.33 | 880 | 67 | 13.2x |

`swc-plugin-coverage-instrument` runs as WebAssembly inside SWC's plugin
sandbox, so its time includes that overhead.

<details>
<summary>Per-file times</summary>

Instrument (ms). The last column is the statement, function, and branch count of
Istanbul, of the Vitest fork, and of `oxc-coverage-instrument` in the strict profile. They are equal
for every file.

| File | KB | `istanbul-lib-instrument` | `@vitest/istanbul-lib-instrument` | `oxc-coverage-instrument` | s/f/b |
|:--|--:|--:|--:|--:|:--|
| React 18.2.0 | 107 | 45 | 7.8 | 1.6 | 1194/176/332 |
| lodash 4.17.21 | 531 | 160 | 35 | 6.5 | 3714/691/1303 |
| Vue 3.3.4 | 463 | 331 | 67 | 11 | 7799/1157/3117 |
| D3 7.8.5 | 573 | 523 | 116 | 19 | 11935/2518/2971 |
| three.js 0.155.0 | 1237 | 839 | 185 | 26 | 20247/2210/4018 |

V8 to Istanbul (ms), on the precise coverage of one run of each build.

| File | V8 functions | `v8-to-istanbul` | `ast-v8-to-istanbul` | `oxc-coverage-instrument` |
|:--|--:|--:|--:|--:|
| React 18.2.0 | 133 | 0.8 | 9.3 | 1.3 |
| lodash 4.17.21 | 611 | 5.1 | 39 | 6.7 |
| Vue 3.3.4 | 755 | 4.8 | 58 | 13 |
| D3 7.8.5 | 1620 | 6.0 | 92 | 26 |
| three.js 0.155.0 | 1775 | 16 | 143 | 41 |

Source-map remap (ms), on the coverage of a `@babel/core` build of each file,
mapped back to the file. All three give the same entry counts.

| File | `istanbul-lib-source-maps` | `@vitest/istanbul-lib-source-maps` | `oxc-coverage-instrument` |
|:--|--:|--:|--:|
| React 18.2.0 | 4.0 | 4.2 | 3.1 |
| lodash 4.17.21 | 17 | 16 | 13 |
| Vue 3.3.4 | 33 | 34 | 23 |
| D3 7.8.5 | 51 | 46 | 42 |
| three.js 0.155.0 | 75 | 68 | 58 |

</details>

Regenerate these numbers with:

```bash
node scripts/prepare-real-world-corpus.mjs
npm --prefix crates/oxc_coverage_instrument_napi run build
node scripts/replacement-benchmarks.mjs
./scripts/benchmark-comparison.sh
```

## Documentation

- [ARCHITECTURE.md](ARCHITECTURE.md) for the pipeline, the crate map, and the
  cross-cutting concerns, including how to run the benchmark comparison.
- [`oxc_coverage_instrument`](crates/oxc_coverage_instrument/README.md) replaces
  `istanbul-lib-instrument`, and documents the deliberate divergences from it.
- [`oxc_coverage_instrument_napi`](crates/oxc_coverage_instrument_napi/README.md)
  is the `oxc-coverage-instrument` npm package, including the runtime matrix.
- [`oxc_coverage_instrument_cli`](crates/oxc_coverage_instrument_cli/README.md)
  is the `oxc-coverage-instrument` binary.
- [`oxc_coverage_types`](crates/oxc_coverage_types/README.md) replaces
  `istanbul-lib-coverage`.
- [`oxc_coverage_source_maps`](crates/oxc_coverage_source_maps/README.md)
  replaces `istanbul-lib-source-maps`.
- [`oxc_coverage_v8`](crates/oxc_coverage_v8/README.md) replaces
  `v8-to-istanbul`.
- [`oxc_coverage_report`](crates/oxc_coverage_report/README.md) replaces
  `istanbul-lib-report`, and
  [`oxc_coverage_reports`](crates/oxc_coverage_reports/README.md) provides a
  selected set of `istanbul-reports`-compatible formats.
- `examples/` carries runnable projects for Vitest with TypeScript, Node with
  WebAssembly, and Cloudflare Workers.

## Compatibility

- Rust 1.96 or newer, 2024 edition
- Oxc 0.155.x
- Istanbul `coverage-final.json` v3 or newer
- Node.js 18 or newer, through napi-rs

## Related projects

| Project | AST |
|:--------|:----|
| [`istanbul-lib-instrument`](https://github.com/istanbuljs/istanbuljs) | Babel |
| [`babel-plugin-istanbul`](https://github.com/istanbuljs/babel-plugin-istanbul) | Babel |
| [`swc-plugin-coverage-instrument`](https://github.com/kwonoj/swc-plugin-coverage-instrument) | SWC |

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md), and [AGENTS.md](AGENTS.md) for the
workspace layout and the check profiles.

## License

MIT
