# oxc_coverage_reports

Istanbul-compatible coverage reporters for the oxc-coverage suite, a Rust port of
`istanbul-reports`.

## Overview

Each module defines a reporter that consumes the `ReportNode` tree built by
`oxc_coverage_report::summarize` and writes one of the Istanbul-compatible output
formats. `Format` selects a reporter at runtime, typically from a CLI flag.

## Key Features

- `text`: ANSI-friendly console table with per-folder and per-file rows.
- `text_summary`: four-line metric rollup, useful for piping to a pull-request
  comment or a CI summary.
- `json_summary`: the `coverage-summary.json` shape consumed by Codecov, Vitest,
  and dashboard tools.
- `lcov`: an LCOV tracefile consumed by Codecov, Coveralls, the GitLab
  merge-request widget, and the `lcov` and `genhtml` toolchain.
- `cobertura`: Cobertura XML consumed by the GitLab merge-request widget,
  Jenkins, Azure DevOps, and Codecov.
- `html`: a self-contained directory of HTML pages with a per-file source gutter,
  behind the `html` Cargo feature.

```rust
use oxc_coverage_types::parse_coverage_map;
use oxc_coverage_report::summarize;
use oxc_coverage_reports::json_summary;

let map = parse_coverage_map(json).unwrap();
let root = summarize(&map);
let mut buf = Vec::new();
json_summary::write(&root, &mut buf).unwrap();
```

The HTML report ships sortable index tables, a filter box, an auto/light/dark
theme toggle, server-side syntax highlighting via
[syntect](https://docs.rs/syntect), copyable line anchors, a jump-to-next-uncovered
button, and a strict Content-Security-Policy so the rendered report performs zero
outbound network requests. Detail pages colour each line by hit, miss, or partial
branch.

## Architecture

Single-file reporters implement `oxc_coverage_report::Visitor` and write into any
`io::Write`, so the caller decides between a file, stdout, and an in-memory
buffer. The `html` reporter is the exception: it emits a directory tree rather
than a stream, so it is driven through `Format::write_to_dir`, and calling
`Format::write` on that variant returns an error instead of producing a
half-formed report.

The `html` feature is opt-in because it pulls in the syntect grammar and theme
data, which dominates the binary size of a build that never renders HTML.
