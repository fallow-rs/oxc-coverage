# oxc_coverage_types

Istanbul-compatible coverage data types for the oxc-coverage suite.

## Overview

First-party serde types derived from Istanbul's JSON schema
(`@istanbuljs/schema`). They produce and consume `coverage-final.json` compatible
output, the format Jest, Vitest, c8, nyc, and Codecov all read.

This crate is the data-model layer of the suite. The instrumenter, the source-map
remapper, the V8-to-Istanbul converter, and the reporters all share these types
rather than defining their own, so a `FileCoverage` can move between layers with
no conversion step.

## Key Features

- `FileCoverage` with the `statementMap`, `fnMap`, `branchMap`, `s`, `f`, `b`,
  and `bT` fields, serializing to Istanbul's exact field set and ordering.
- `FnEntry`, `BranchEntry`, `Location`, and `Position` as the shared span
  vocabulary: 1-based line, 0-based UTF-16 column.
- `UnhandledPragma` for coverage pragma comments that were recognized but not
  applied.
- `parse_coverage_map` to read a whole `coverage-final.json` into a map of path
  to `FileCoverage`.

```rust
use oxc_coverage_types::parse_coverage_map;

let json = std::fs::read_to_string("coverage-final.json").unwrap();
let map = parse_coverage_map(&json).unwrap();

for (path, coverage) in &map {
    println!("{}: {} statements, {} functions, {} branches",
        path, coverage.s.len(), coverage.f.len(), coverage.b.len());
}
```

## Architecture

The types are plain serde structs with no dependency on Oxc, on a source-map
library, or on any encoding assumption. Coordinate conversion happens in the
crates that own an encoding boundary (UTF-8 spans in the instrumenter, UTF-16
offsets in the V8 converter, 0-based positions in the source-map remapper), which
keeps this crate a stable interchange format rather than a leaky one. Entries are
stored in `BTreeMap`s so serialized key order is deterministic across runs.

Depend on this crate directly when you need to read or write Istanbul coverage
data without instrumenting anything.
