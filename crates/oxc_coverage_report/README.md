# oxc_coverage_report

Coverage summarization and the tree-visitor base for the oxc-coverage suite, a
Rust port of `istanbul-lib-report`.

## Overview

This crate consumes Istanbul-compatible coverage data, a `CoverageMap` parsed by
`oxc_coverage_types`, summarizes hit counts into per-metric totals, and organizes
per-file coverage into a folder and file `ReportNode` tree that a reporter walks
via the `Visitor` trait.

The reporters themselves (text, text-summary, json-summary, lcov, cobertura, and
html behind a feature flag) live in the sibling `oxc_coverage_reports` crate.
Custom reporters implement `Visitor` directly against this crate.

## Key Features

- `summarize` builds the report tree from a coverage map, grouping files into
  folder nodes.
- `walk` drives a `Visitor` over that tree, calling `on_summary` for folders and
  `on_detail` for files.
- `CoverageSummary` and `Metric` carry the covered, total, skipped, and
  percentage values for statements, branches, functions, and lines.

```rust
use oxc_coverage_types::parse_coverage_map;
use oxc_coverage_report::{summarize, walk, Visitor, ReportNode};

struct Counter { files: usize, folders: usize }
impl Visitor for Counter {
    fn on_detail(&mut self, _node: &ReportNode) -> std::io::Result<()> {
        self.files += 1;
        Ok(())
    }
    fn on_summary(&mut self, _node: &ReportNode) -> std::io::Result<()> {
        self.folders += 1;
        Ok(())
    }
}

let map = parse_coverage_map(json).unwrap();
let root = summarize(&map);
let mut visitor = Counter { files: 0, folders: 0 };
walk(&root, &mut visitor).unwrap();
```

## Architecture

Summarization is a two-stage pass. The summarizer folds each `FileCoverage` into
a `CoverageSummary`, then the tree builder groups files by common path prefix into
`ReportNode`s of kind folder or file, rolling child summaries up into their
parent. `walk` is a depth-first traversal over the finished tree, so a reporter
never touches raw coverage entries and never has to reimplement percentage
arithmetic.

Depend on this crate directly when writing a reporter that the suite does not
already ship.
