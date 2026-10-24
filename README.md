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
oxc_coverage_instrument = "0.17"
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
| `istanbul-lib-coverage` | `@vitest/istanbul-lib-coverage` | `summarizeCoverage()` | `oxc_coverage_types` | The data model and the summaries. No public merge API. |
| `istanbul-lib-report`, `istanbul-reports` | `@vitest/istanbul-lib-report` | `createReport()`, `writeHtmlReport()` | `oxc_coverage_report`, `oxc_coverage_reports` | The `text`, `text-summary`, `json-summary`, `lcov`, `cobertura`, and `html` reports. Not `clover`, `json`, `lcovonly`, `teamcity`, `text-lcov`, or `html-spa`. |

The Vitest fork keeps the Istanbul API and replaces Babel with the Oxc parser of
`rolldown` and `magic-string`. This project replaces the same packages with Rust
code behind a native or WebAssembly binding.

## Benchmarks

Times on the five pinned library builds of `scripts/real-world-corpus.json`, in
one Node.js process, as the median of 7 runs after 2 warm-up runs. Measured on
2026-10-24 on an Apple M5 (macOS, Darwin 25.5), Node.js 24.20.0, with the
release build of the native binding. The time is the sum over the five files, in
milliseconds. A ratio above 1 means that oxc is faster.

| Stage | Package | Package (ms) | oxc (ms) | Ratio |
|:--|:--|--:|--:|--:|
| Instrument | `istanbul-lib-instrument` 6.0.3 | 1985 | 65 | 30.5x |
| Instrument | `@vitest/istanbul-lib-instrument` 2.0.0 | 409 | 65 | 6.3x |
| Instrument | `babel-plugin-istanbul` 8.0.0 | 571 | 67 | 8.6x |
| Instrument | `swc-plugin-coverage-instrument` 0.0.33 | 880 | 67 | 13.2x |
| V8 to Istanbul | `ast-v8-to-istanbul` 1.0.7, with `oxc-parser` | 339 | 91 | 3.7x |
| V8 to Istanbul | `v8-to-istanbul` 9.3.0 | 32 | 91 | 0.4x |
| Source-map remap | `istanbul-lib-source-maps` 5.0.6 | 185 | 160 | 1.2x |
| Source-map remap | `@vitest/istanbul-lib-source-maps` 2.0.0 | 177 | 160 | 1.1x |
| Summary | `istanbul-lib-coverage` 3.2.2 | 38 | 46 | 0.8x |
| `text` report | `istanbul-lib-report` 3.0.1 + `istanbul-reports` 3.2.0 | 45 | 42 | 1.1x |
| `text-summary` report | same | 32 | 37 | 0.9x |
| `json-summary` report | same | 33 | 37 | 0.9x |
| `lcov` report | same | 229 | 45 | 5.1x |
| `cobertura` report | same | 162 | 62 | 2.6x |
| `html` report | same | 132 | 4299 | 0.03x |

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

<details>
<summary>Per-file times</summary>

Instrument (ms). The last column is the statement, function, and branch count of
both Istanbul and oxc in the strict profile, which are equal for every file.

| File | KB | `istanbul-lib-instrument` | `@vitest/istanbul-lib-instrument` | `babel-plugin-istanbul` | `swc-plugin-coverage-instrument` | oxc | s/f/b |
|:--|--:|--:|--:|--:|--:|--:|:--|
| React 18.2.0 | 107 | 46 | 7.6 | 16.6 | 23.5 | 1.4 | 1194/176/332 |
| lodash 4.17.21 | 531 | 181 | 33 | 49.6 | 79.7 | 6.7 | 3714/691/1303 |
| Vue 3.3.4 | 463 | 356 | 69 | 103.4 | 174.0 | 11 | 7799/1157/3117 |
| D3 7.8.5 | 573 | 546 | 113 | 165.2 | 248.1 | 20 | 11935/2518/2971 |
| three.js 0.155.0 | 1237 | 856 | 186 | 236.4 | 354.4 | 26 | 20247/2210/4018 |

V8 to Istanbul (ms), on the precise coverage of one run of each build.

| File | V8 functions | `v8-to-istanbul` | `ast-v8-to-istanbul` | oxc |
|:--|--:|--:|--:|--:|
| React 18.2.0 | 133 | 0.9 | 9.0 | 1.4 |
| lodash 4.17.21 | 611 | 5.3 | 40 | 6.9 |
| Vue 3.3.4 | 755 | 4.7 | 57 | 13 |
| D3 7.8.5 | 1620 | 6.3 | 92 | 27 |
| three.js 0.155.0 | 1775 | 15 | 141 | 43 |

Source-map remap (ms), on the coverage of a `@babel/core` build of each file,
mapped back to the file. All three give the same entry counts.

| File | `istanbul-lib-source-maps` | `@vitest/istanbul-lib-source-maps` | oxc |
|:--|--:|--:|--:|
| React 18.2.0 | 4.1 | 4.6 | 3.6 |
| lodash 4.17.21 | 17 | 17 | 15 |
| Vue 3.3.4 | 37 | 33 | 27 |
| D3 7.8.5 | 50 | 48 | 46 |
| three.js 0.155.0 | 77 | 74 | 68 |

</details>

Regenerate these numbers with:

```bash
node scripts/prepare-real-world-corpus.mjs
npm --prefix crates/oxc_coverage_instrument_napi run build
node scripts/replacement-benchmarks.mjs
./scripts/benchmark-comparison.sh
```

The second script adds `babel-plugin-istanbul`, `swc-plugin-coverage-instrument`,
and the native CLI. `swc-plugin-coverage-instrument` runs as WebAssembly inside
SWC's plugin sandbox, so its time includes that overhead.

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
