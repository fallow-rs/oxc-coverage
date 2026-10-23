# oxc-coverage

[![CI](https://github.com/fallow-rs/oxc-coverage-instrument/actions/workflows/ci.yml/badge.svg)](https://github.com/fallow-rs/oxc-coverage-instrument/actions/workflows/ci.yml)
[![Coverage](https://img.shields.io/endpoint?url=https://raw.githubusercontent.com/fallow-rs/oxc-coverage-instrument/badges/coverage.json)](https://github.com/fallow-rs/oxc-coverage-instrument/actions/workflows/coverage.yml)
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

The repository is named `oxc-coverage-instrument` for historical reasons. That
name now belongs to the instrumenter crate, the npm package, and the CLI binary
only; the other crates share the `oxc_coverage_*` prefix.

## Install

```toml
[dependencies]
oxc_coverage_instrument = "0.17"
```

```bash
npm install oxc-coverage-instrument
```

```bash
cargo install --git https://github.com/fallow-rs/oxc-coverage-instrument oxc_coverage_instrument_cli
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
- Oxc 0.151.x
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
