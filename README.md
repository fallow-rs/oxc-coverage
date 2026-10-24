# oxc-coverage

[![CI](https://github.com/fallow-rs/oxc-coverage/actions/workflows/ci.yml/badge.svg)](https://github.com/fallow-rs/oxc-coverage/actions/workflows/ci.yml)
[![Coverage](https://img.shields.io/endpoint?url=https://raw.githubusercontent.com/fallow-rs/oxc-coverage/badges/coverage.json)](https://github.com/fallow-rs/oxc-coverage/actions/workflows/coverage.yml)
[![Crates.io](https://img.shields.io/crates/v/oxc_coverage_instrument.svg)](https://crates.io/crates/oxc_coverage_instrument)
[![npm](https://img.shields.io/npm/v/oxc-coverage-instrument.svg)](https://www.npmjs.com/package/oxc-coverage-instrument)
[![docs.rs](https://docs.rs/oxc_coverage_instrument/badge.svg)](https://docs.rs/oxc_coverage_instrument)
[![MIT License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

Istanbul-compatible coverage toolchain for JavaScript and TypeScript, built on
the [Oxc](https://oxc.rs) parser: instrumentation, source-map remapping,
V8-to-Istanbul conversion, and reporters.

> **Unofficial.** This is an independent community project that builds on Oxc
> crates. It is not part of, affiliated with, or endorsed by the Oxc project.

The core is an AST-level instrumenter built on `oxc_traverse` and
`oxc_codegen`. Its output is checked against `istanbul-lib-instrument` on a
shared fixture corpus, both by count and byte for byte. Around it sits the rest
of the Istanbul-compatible pipeline: the data model, source-map remapping,
V8-to-Istanbul conversion, report summarization, and the `text`,
`text-summary`, `json-summary`, `lcov`, `cobertura`, and `html` reporters. It
does not claim to port every Istanbul package or reporter.

This repository was previously named `oxc-coverage-instrument`. That name now
belongs to the instrumenter crate, the npm package, and the CLI binary only; the
other crates share the `oxc_coverage_*` prefix.

## Install

```toml
[dependencies]
oxc_coverage_instrument = "0.18"
```

```bash
npm install oxc-coverage-instrument
```

```bash
cargo install --git https://github.com/fallow-rs/oxc-coverage oxc_coverage_instrument_cli
```

## Usage

```rust
use oxc_coverage_instrument::{instrument, InstrumentOptions};

let source = "function add(a, b) { return a + b; }";
let result = instrument(source, "add.js", &InstrumentOptions::default()).unwrap();

assert_eq!(result.coverage_map.fn_map["0"].name, "add");
println!("{}", result.code);
```

```javascript
import { instrument } from 'oxc-coverage-instrument';

const result = instrument(source, 'file.js', { sourceMap: true });
const coverageMap = JSON.parse(result.coverageMap);
```

```bash
oxc-coverage-instrument src/app.js -o dist/app.js --source-map
oxc-coverage-instrument report --format text coverage-final.json
```

Vitest, `vite-plugin-istanbul`, and custom Vite or Rollup plugins are covered in
the [Node.js package README](crates/oxc_coverage_instrument_napi/README.md).

## What it replaces

Each row names an Istanbul package, its Vitest fork from
[`vitest-dev/istanbuljs`](https://github.com/vitest-dev/istanbuljs) where one
exists, and the replacement in the `oxc-coverage-instrument` npm package and in
the Rust crates.

| Package | Vitest fork | npm replacement | Rust crate | Scope |
|:--|:--|:--|:--|:--|
| `istanbul-lib-instrument` | `@vitest/istanbul-lib-instrument` | `instrument()`, `createOxcInstrumenter()` from `oxc-coverage-instrument/vitest` | `oxc_coverage_instrument` | Full. The strict profile matches Istanbul byte for byte. |
| `babel-plugin-istanbul`, `swc-plugin-coverage-instrument` | | `instrument()` in a Vite or Rollup plugin | `oxc_coverage_instrument` | Same output shape as `istanbul-lib-instrument`. |
| `v8-to-istanbul`, `ast-v8-to-istanbul` | | `v8ToIstanbul()` | `oxc_coverage_v8` | Statement, function, and branch entries from the AST, as `ast-v8-to-istanbul` gives. `v8-to-istanbul` gives one statement per line. |
| `istanbul-lib-source-maps` | `@vitest/istanbul-lib-source-maps` | `remapCoverageMap()` | `oxc_coverage_source_maps` | Full `transformCoverage`, plus the optional `dropUnmapped`. `instrument()` can also compose the input source map at instrument time (`composeInputSourceMap`). |
| `istanbul-lib-coverage` | `@vitest/istanbul-lib-coverage` | `summarizeCoverage()` | `oxc_coverage_types`, `oxc_coverage_report` | The data model (`oxc_coverage_types`) and the summaries (`oxc_coverage_report`). No public merge API. |
| `istanbul-lib-report`, `istanbul-reports` | `@vitest/istanbul-lib-report` | `createReport()`, `writeHtmlReport()` | `oxc_coverage_report`, `oxc_coverage_reports` | The `text`, `text-summary`, `json-summary`, `cobertura`, and `html` reports, and `lcov`, which writes the `lcov.info` of Istanbul's `lcovonly`. Istanbul's `lcov` also writes an HTML tree. Not `clover`, `json`, `teamcity`, `text-lcov`, or `html-spa`. |

The Vitest instrumenter fork keeps the Istanbul API and replaces Babel with the
Oxc parser of `rolldown` and with `magic-string`. The other Vitest forks are
JavaScript ports of the Istanbul packages. This project replaces all of them
with Rust code behind a native or WebAssembly binding.

## Benchmarks

Times on the five pinned library builds of `scripts/real-world-corpus.json`, in
one Node.js process, as the median of 7 runs after 2 warm-up runs, from
`scripts/replacement-benchmarks.mjs`. Measured on 2026-10-24 on an Apple M5
(macOS, Darwin 25.5), Node.js 24.20.0, with the release build of the native
binding. The time is the sum over the five files, in milliseconds. A ratio
above 1 means that oxc is faster.

| Stage | Package | Package (ms) | oxc (ms) | Ratio |
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
  three.js, where oxc and `ast-v8-to-istanbul` give the 20247 statements and
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

| Package | Package (ms) | oxc (ms) | Ratio |
|:--|--:|--:|--:|
| `babel-plugin-istanbul` 8.0.0 | 571 | 67 | 8.6x |
| `swc-plugin-coverage-instrument` 0.0.33 | 880 | 67 | 13.2x |

`swc-plugin-coverage-instrument` runs as WebAssembly inside SWC's plugin
sandbox, so its time includes that overhead.

<details>
<summary>Per-file times</summary>

Instrument (ms). The last column is the statement, function, and branch count of
Istanbul, of the Vitest fork, and of oxc in the strict profile. They are equal
for every file.

| File | KB | `istanbul-lib-instrument` | `@vitest/istanbul-lib-instrument` | oxc | s/f/b |
|:--|--:|--:|--:|--:|:--|
| React 18.2.0 | 107 | 45 | 7.8 | 1.6 | 1194/176/332 |
| lodash 4.17.21 | 531 | 160 | 35 | 6.5 | 3714/691/1303 |
| Vue 3.3.4 | 463 | 331 | 67 | 11 | 7799/1157/3117 |
| D3 7.8.5 | 573 | 523 | 116 | 19 | 11935/2518/2971 |
| three.js 0.155.0 | 1237 | 839 | 185 | 26 | 20247/2210/4018 |

V8 to Istanbul (ms), on the precise coverage of one run of each build.

| File | V8 functions | `v8-to-istanbul` | `ast-v8-to-istanbul` | oxc |
|:--|--:|--:|--:|--:|
| React 18.2.0 | 133 | 0.8 | 9.3 | 1.3 |
| lodash 4.17.21 | 611 | 5.1 | 39 | 6.7 |
| Vue 3.3.4 | 755 | 4.8 | 58 | 13 |
| D3 7.8.5 | 1620 | 6.0 | 92 | 26 |
| three.js 0.155.0 | 1775 | 16 | 143 | 41 |

Source-map remap (ms), on the coverage of a `@babel/core` build of each file,
mapped back to the file. All three give the same entry counts.

| File | `istanbul-lib-source-maps` | `@vitest/istanbul-lib-source-maps` | oxc |
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
