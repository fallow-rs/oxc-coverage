# oxc_coverage_source_maps

Istanbul-compatible source-map remapping for `FileCoverage`, a Rust port of
`istanbul-lib-source-maps`.

## Overview

Istanbul coverage data carries an `inputSourceMap` on each `FileCoverage` when
the instrumented input was itself a transform output, such as TypeScript emitted
by `tsc` and then instrumented. Downstream reporters (nyc,
`@vitest/coverage-istanbul`, monocart) walk that map and rewrite every coverage
position back to the original source. This crate covers both upstream usage
modes.

Mode A, remap at report time: `remap_coverage` and `remap_coverage_map` walk a
single `FileCoverage` (or every entry of a `coverage-final.json` shaped map)
through the embedded `inputSourceMap`. When the map sits next to the instrumented
file on disk rather than embedded, `remap_coverage_with_loader` takes a loader
callback, matching the `sourceStore` semantics `istanbul-lib-source-maps` exposes
to nyc:

```rust
use oxc_coverage_source_maps::remap_coverage_with_loader;

let remapped = remap_coverage_with_loader(&fc, |path| {
    std::fs::read_to_string(format!("{path}.map")).ok()
});
```

Mode B, remap during collection: `SourceMapStore` accumulates per-file maps via
`add_map` and rewrites incoming `FileCoverage` objects via `transform_coverage`.
Runners that hand maps in incrementally (Jest with `transform`, plugins with
their own transform pipeline) consult the store as files are instrumented rather
than once at report time.

## Key Features

- One-to-many remapping. The single-file helpers return `Some` only when the
  mapped locations all belong to one original file. Use
  `remap_coverage_to_map` when one generated bundle carries mappings for several
  sources. The coverage-map helpers and `SourceMapStore::transform_coverage_to_map`
  take that path explicitly, so each original source receives its own
  `FileCoverage`.
- Merging rather than replacement. When several generated chunks map to the same
  original path, statements merge by location, functions by declaration location,
  and branches by ordered arm locations, matching `istanbul-lib-source-maps`. The
  first function or branch supplies metadata such as name, body, type, and
  umbrella location. Matching `s`, `f`, `b`, and `bT` counts use saturating `u32`
  addition.
- Istanbul `getMapping` range semantics. Starts resolve by greatest lower bound,
  ends resolve to the next original segment or to the end of the original line.
  This matches `createSourceMapStore().transformCoverage` byte for byte. An
  exclusive end that lands between source-map segments therefore widens to the
  full token instead of truncating backwards, and a sub-segment span such as a
  one-character arrow declaration widens to its enclosing span. Line numbers and
  coverage percentages are unaffected, but `istanbul-lib-coverage`'s `keyFromLoc`
  includes columns, so flush coverage caches written before 0.4.0 before merging
  them with newer runs.
- Optional pruning of unmappable entries, via `RemapOptions`.

### Dropping unmapped entries

A position whose lookup returns `None` keeps its generated coordinates for an
unambiguous single-source map. In a multi-source map it has no safe original
owner and is omitted. Pass `RemapOptions { drop_unmapped: true }` to drop
statement, function, and branch entries that fail to remap along with their `s`,
`f`, `b`, and `bT` slots:

```rust
use oxc_coverage_source_maps::{remap_coverage_map_with_options, RemapOptions};

let remapped =
    remap_coverage_map_with_options(&coverage_map, RemapOptions { drop_unmapped: true });
```

Drop semantics follow `istanbul-lib-source-maps`'s `transformer.js`: a statement
drops when its start or end fails, a function drops when any of `decl` or `loc`
start or end fails, and branch arms drop per arm. The whole branch drops when no
arm survives, or when the retained mapped arms disagree on their source. Branch
ownership comes from the arms, so an unmapped umbrella falls back to the first
retained arm, and a mapped umbrella stays branch metadata even when it resolves
elsewhere. This is what you want when instrumenting compiler-emitted boilerplate
that has no original-source mapping, such as the `?vue&type=script` chunk
`@vitejs/plugin-vue` produces, where the alternative is reporters rendering
chunk-line positions against `.vue` paths.

## Architecture

`PositionRemapper` wraps a parsed source map and answers the one question the
eager instrument-time path needs: whether a whole `Location` resolves to the
original source, so a keep-or-drop decision made before remapping matches the one
the deferred path would make. Istanbul's `Position` is a
1-based line plus a 0-based UTF-16 column, while `srcmap-sourcemap`'s
`original_position_for` is 0-based on both axes, so conversion happens at the
lookup boundary rather than in the shared types. The remap functions are thin
layers over that: they walk each `Location` in a `FileCoverage`, resolve start and
end independently, and rebuild the entry maps and hit-count vectors under the
resolved source path.

`oxc_coverage_instrument` re-exports this crate's surface, so depend on it
directly only when you remap coverage without instrumenting it.
