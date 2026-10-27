# oxc_coverage_v8

V8 inspector coverage to Istanbul `FileCoverage` conversion, a Rust port of
`v8-to-istanbul`.

## Overview

V8's inspector protocol reports coverage as `[startOffset, endOffset, count]`
ranges grouped by function. Istanbul reporters consume per-statement,
per-function, and per-branch hit counts keyed by line and column. This crate
takes a pre-built `FileCoverage`, typically produced by an AST-traversal pass in
an instrumenter, and fills in its hit-count vectors from V8 ranges.

The input shape is the one Node's inspector and `@vitest/coverage-v8` emit, so
V8-collected data joins the reporting pipeline at the same point runtime-collected
`__coverage__` does.

## Key Features

- Encoding normalization. Inspector offsets and Istanbul columns are UTF-16 code
  units, while Oxc branch spans are UTF-8 bytes, so source-side coordinates are
  converted before matching. Astral characters and mixed line endings are handled.
- Explicit wrapper base. Node wraps every CommonJS module in
  `(function(exports,require,module,...){`. Node inspector output is
  source-relative and reports a `wrapper_length` of zero; a nonzero value is an
  explicit UTF-16 base, for producers that already report wrapper-shifted ranges.
- Block coverage. With block coverage enabled, statement, function, and branch
  counts are populated by intersecting V8 ranges with locations recovered from a
  visit-only AST pass.
- Source-map recovery. Inline `//# sourceMappingURL=data:...` trailers are
  decoded automatically by `extract_inline_source_map`; external map references
  are reported by `extract_external_source_mapping_url` and resolve through the
  optional loader on `v8_to_istanbul_with_loader`.

```rust
use oxc_coverage_instrument::v8_to_istanbul_with_loader;

let fc = v8_to_istanbul_with_loader(source, "app.js", &functions, 0, |url| {
    std::fs::read_to_string(url).ok()
})?;
```

## Architecture

`apply_v8_coverage` is the core: it walks the V8 ranges once, intersects each
against the locations already recorded in the `FileCoverage`, and writes the
resulting hit counts into the `s`, `f`, and `b` vectors. Because the entry maps
are supplied rather than derived, the crate never parses source on the counting
path, and the same `FileCoverage` shape is produced whether counts came from V8
or from injected counters.

V8 reports the instance fields and the static members of a class as two
initializer functions. Their ranges do not show which code each one runs: some
V8 versions give both the class body, and a range can include a computed key.
`apply_v8_coverage_with_class_members` takes the byte spans of the class member
code from the AST pass, so each member gets the count of its own initializer.

The `v8_to_istanbul` entry points that build the `FileCoverage` from source live
in `oxc_coverage_instrument`, which owns the AST pass; this crate stays free of a
parser dependency on the conversion path.
