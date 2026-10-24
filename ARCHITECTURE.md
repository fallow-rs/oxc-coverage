# Architecture

This document describes the high-level structure of the oxc-coverage suite: how
source flows through the crates, what each crate owns, and which concerns cut
across all of them. For usage, see the README of the crate you need.

## Bird's Eye View

Coverage counters are injected at the AST level, and no regular expression
rewrites the input source. The standalone source-to-source adapter also inserts
a synthetic AST marker after the directive prologue. After codegen it replaces
that marker statement with the generated coverage setup and shifts the following
source-map lines.

```
source code (JS/TS)
    |
    v
oxc_parser          parse to AST
    |
    v
SemanticBuilder     build scope tree
    |
    v
CoverageTransform   traverse AST, inject ++cov().s[N] counters
    |
    v
oxc_codegen         emit instrumented code and source map
    |
    v
setup insertion     replace generated marker, shift source map
    |
    v
instrumented code + coverage map
```

`CoverageTransform` is an `oxc_traverse` visitor. It assigns each statement,
function, and branch an id, records its `Location` in the coverage map, and
injects the counter expression that increments the matching slot at runtime.
The standalone adapter adds the setup marker, codegen emits the rewritten AST
and its source map, then the adapter replaces that marker with the coverage
setup. The experimental host-owned AST API instead returns the setup preamble
for the host to insert and connect to semantic state.

When an `inputSourceMap` is supplied, the instrumenter composes the codegen
output map with the input map, so downstream remappers (Vitest, nyc, monocart)
resolve coverage positions back to the original source. Composition is delegated
to [`srcmap-remapping`](https://crates.io/crates/srcmap-remapping), which mirrors
`@ampproject/remapping` semantics, the primitive `istanbul-lib-source-maps` and
the major bundlers rely on.

## Code Map

The workspace provides a broad Istanbul-compatible coverage pipeline, with one
crate per responsibility. It does not claim to port every Istanbul package or
reporter; the reporting crate implements the formats named below.

| Crate | Replaces |
|:------|:---------|
| [`oxc_coverage_instrument`](crates/oxc_coverage_instrument/README.md) | `istanbul-lib-instrument` |
| [`oxc_coverage_types`](crates/oxc_coverage_types/README.md) | `istanbul-lib-coverage` (data model) |
| [`oxc_coverage_source_maps`](crates/oxc_coverage_source_maps/README.md) | `istanbul-lib-source-maps` |
| [`oxc_coverage_v8`](crates/oxc_coverage_v8/README.md) | `v8-to-istanbul` |
| [`oxc_coverage_report`](crates/oxc_coverage_report/README.md) | `istanbul-lib-report` |
| [`oxc_coverage_reports`](crates/oxc_coverage_reports/README.md) | `istanbul-reports` (partial) |

Two more crates are workspace-local rather than published to crates.io:
[`oxc_coverage_instrument_cli`](crates/oxc_coverage_instrument_cli/README.md)
ships the `oxc-coverage-instrument` binary, and
[`oxc_coverage_instrument_napi`](crates/oxc_coverage_instrument_napi/README.md)
ships the `oxc-coverage-instrument` npm package. The npm package exposes the
instrumenter, remapping, V8 conversion, summaries, and reporters; the html
reporter is in the native bindings only, to keep the WebAssembly binding under
its size ceiling.

At runtime the crates compose into one pipeline, from source on disk to a
rendered report.

```
   source code (JS/TS)
        |
        v
   +-----------------------------+
   | oxc_coverage_instrument     |  parse, transform, codegen
   +-----------------------------+
        |
        |  instrumented code + composed source map
        v
   +-----------------------------+
   | runtime collection          |  browser / Node / V8
   | (writes __coverage__)       |
   +-----------------------------+
        |
        |  raw FileCoverage objects
        v
   +-----------------------------+
   | oxc_coverage_source_maps    |  remap to original source paths
   +-----------------------------+
        |
        |  remapped FileCoverage
        v
   +-----------------------------+
   | oxc_coverage_report         |  tree, summary, visitor
   | oxc_coverage_reports        |  text, text-summary, json-summary,
   |                             |  lcov, cobertura, html
   +-----------------------------+
```

`oxc_coverage_types` sits underneath every box in that diagram. It defines
`FileCoverage`, `FnEntry`, `BranchEntry`, `Location`, and `Position`, and it owns
the serde representation that has to round-trip `coverage-final.json` unchanged.
Every other crate consumes those types rather than defining its own.

`oxc_coverage_v8` is a second entry point into the same model: instead of
instrumenting source, it fills the hit-count vectors of a `FileCoverage` from V8
inspector ranges, so V8-collected data joins the pipeline at the same place
runtime-collected `__coverage__` does.

With the experimental `ast-api` feature, an Oxc host can enter the first box at
the transform boundary. `instrument_program` accepts a host-owned `Program` and
`Scoping`, mutates the AST with counters, and returns coverage metadata plus the
setup preamble. Parsing, TypeScript or JSX lowering, preamble insertion,
semantic rebuilding, and code generation remain owned by the host.

`oxc_coverage_instrument` re-exports the remap, V8, and data-model surfaces, so a
consumer that only needs the default path can depend on that one crate.

## Cross-Cutting Concerns

### Istanbul conformance

Output is checked against `istanbul-lib-instrument` on a shared fixture corpus
covering every branch type, function form, Unicode columns, pragma boundaries,
hashbangs, directive prologues, binding collisions, class fields, stripped
TypeScript, and edge cases. The corpus lives in
`crates/oxc_coverage_instrument/tests/conformance/`. The suite asserts that
statement, function, and branch counts match exactly, that branch types and
per-branch location counts match, that the JSON field set matches, and that the
instrumented output re-parses as valid JavaScript.

CI also runs a blocking byte-for-byte diff over the same corpus under the strict
Istanbul compatibility profile, without divergence filters. A separate gate
compares the default profile with that strict shape and permits only documented
name, method-span, synthetic-else, logical-assignment, and optional-chain
extensions. The default Oxc extensions are enumerated in
[the instrumenter's README](crates/oxc_coverage_instrument/README.md#differences-from-istanbul-lib-instrument),
and `scripts/istanbul-diff.mjs` is the tool that enforces profile parity.

### Position semantics

Istanbul's `Position` is a 1-based line plus a 0-based UTF-16 column. Every
public `Location` in this workspace uses that convention, so `statementMap`,
`fnMap`, `branchMap`, and `unhandledPragmas` columns match Babel and
`istanbul-lib-instrument` on non-ASCII sources.

The internal representations disagree, and each boundary converts explicitly:

- Oxc spans are UTF-8 byte offsets.
- V8 inspector ranges are absolute UTF-16 code-unit offsets.
- `srcmap-sourcemap`'s `original_position_for` is 0-based on both axes.

Conversion happens at the lookup boundary in each crate rather than being pushed
into the shared types, which keeps `oxc_coverage_types` free of any encoding
assumption.

### Benchmarks

`./scripts/benchmark-comparison.sh` times this instrumenter against
`istanbul-lib-instrument`, `babel-plugin-istanbul`, and
`swc-plugin-coverage-instrument` on five pinned library builds (React, lodash,
Vue, D3, three.js), reporting the median of several runs per file. The Node.js
table runs every tool in one process, so the numbers are comparable; a second
table times the native CLI, which pays process startup on top.

The script downloads the pinned library builds but installs the current release
of each competing instrumenter, so results are only meaningful alongside the
resolved versions and the machine they were produced on.

`node scripts/replacement-benchmarks.mjs` times every stage of a coverage run
against the package it replaces, and the Vitest fork of that package where one
exists: instrumentation, V8 conversion, source-map remapping, summaries, and
each reporter. It installs exact package versions into
`.bench-tmp/replacements`, uses real V8 inspector coverage and real
`@babel/core` source maps of the same five builds, and prints the output size of
each tool next to its time, so a fast result that produces less output is
visible. The README tables come from this script; regenerate them with the
release build of the N-API binding (`npm --prefix
crates/oxc_coverage_instrument_napi run build`) and update the date and machine
line when you change them.

`swc-plugin-coverage-instrument` is written in Rust but runs as a WASM module
inside SWC's plugin sandbox, so its numbers include WASM and serialization
overhead at every AST boundary rather than measuring native Rust.

CodSpeed runs the Rust benchmarks under `crates/*/benches/` on every push to
`main` and every pull request that touches `crates/` or a workspace manifest.
