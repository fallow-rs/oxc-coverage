//! Position resolution: the istanbul `getMapping` range remap, plus the direct
//! per-position lookup used for the `line: 0` sentinel and the no-drop
//! fallback.
//!
//! Surviving positions are resolved exactly as `istanbul-lib-source-maps`'s
//! `lib/get-mapping.js` does. Source maps carry segments only at token
//! *starts*, so a direct lookup of an exclusive end snaps backward to the
//! previous segment (truncating the span) and a span smaller than its
//! enclosing segment never balloons. `getMapping` fixes both: the start
//! resolves with greatest-lower-bound (so a sub-segment span snaps to its
//! enclosing mapped span) and the end resolves to the *next* original segment
//! after the end (or the end of the original line), matching
//! `createSourceMapStore().transformCoverage`.
//!
//! Coordinate boundary: Istanbul `Position` is 1-based line + 0-based UTF-16
//! column; `srcmap-sourcemap` is 0-based for both. Conversion happens here.

use oxc_coverage_types::{Location, Position};
use srcmap_sourcemap::{Bias, GeneratedLocation, OriginalLocation, SourceMap};

use crate::context::{LocationKey, MappedLocation, OriginalLineKey, RemapContext};

/// The original end resolved by [`original_end_position_for`].
///
/// Mirrors `originalEndPositionFor`: either the start of the *next* original
/// segment on the same original line, or the end of that line (istanbul's
/// `column: Infinity`, which a `u32` cannot hold and which clamps at use).
enum EndResult {
    /// The next original segment after the end position; its column is the
    /// exclusive end of the span.
    Mapped { source: u32, line: u32, column: u32 },
    /// No segment follows on the same original line: the span extends to the
    /// end of the line (istanbul's `Infinity`). Clamped to the line's UTF-16
    /// length at use via [`RemapCaches::original_line_end_column`].
    ///
    /// [`RemapCaches::original_line_end_column`]: crate::context::RemapCaches::original_line_end_column
    EndOfLine { source: u32, line: u32 },
}

/// `originalPositionTryBoth`: greatest-lower-bound first, falling back to
/// least-upper-bound. `line` and `column` are 0-based (srcmap space).
fn original_position_try_both(sm: &SourceMap, line: u32, column: u32) -> Option<OriginalLocation> {
    sm.original_position_for_with_bias(line, column, Bias::GreatestLowerBound)
        .or_else(|| sm.original_position_for_with_bias(line, column, Bias::LeastUpperBound))
}

/// `allGeneratedPositionsFor({ ..., bias: LEAST_UPPER_BOUND })`.
///
/// Returns every generated position that maps to the original `(source, line)`
/// at the least-upper-bound of `column`: the exact column when a segment exists
/// there, otherwise the next greater original column on that line.
///
/// `srcmap-sourcemap`'s `all_generated_positions_for` is exact-match on
/// `(source, line, column)`, not least-upper-bound like
/// `@jridgewell/trace-mapping`, so the matched column is computed on the
/// original side first, mirroring trace-mapping's `sliceGeneratedPositions`.
/// Resolving it that way rather than through a generated-position round-trip
/// keeps the result faithful when several mappings share a generated column.
/// The first lookup for a source indexes the sorted original columns of every
/// line of that source in one pass over the mappings, in [`RemapContext`];
/// every later lookup is logarithmic.
fn all_generated_positions_for_lub(
    ctx: &mut RemapContext<'_>,
    source: &str,
    line: u32,
    column: u32,
) -> Vec<GeneratedLocation> {
    // Key the scan by name -> index (not the caller's raw mapping index): istanbul
    // and trace-mapping look up the source by name too, so for a map where the
    // same name appears at multiple indices this matches the library's first-match
    // behaviour. Well-formed maps have unique source names, so the two coincide.
    let Some(source_idx) = ctx.sm.source_index(source) else {
        return Vec::new();
    };
    let matched_column = matched_original_column(ctx, source_idx, line, column);
    let Some(matched_column) = matched_column else {
        return Vec::new();
    };
    ctx.sm.all_generated_positions_for(source, line, matched_column)
}

/// Least-upper-bound of `column` among the original columns mapped on
/// `(source_idx, line)`. The first call for a source indexes all its lines,
/// because one scan per line makes a large map quadratic.
fn matched_original_column(
    ctx: &mut RemapContext<'_>,
    source_idx: u32,
    line: u32,
    column: u32,
) -> Option<u32> {
    if ctx.caches.indexed_sources.insert(source_idx) {
        let index = &mut ctx.caches.original_line_columns;
        for mapping in ctx.sm.all_mappings().iter().filter(|m| m.source == source_idx) {
            let key = OriginalLineKey { source: source_idx, line: mapping.original_line };
            index.entry(key).or_default().push(mapping.original_column);
        }
        for (_, columns) in index.range_mut(
            OriginalLineKey { source: source_idx, line: 0 }..=OriginalLineKey {
                source: source_idx,
                line: u32::MAX,
            },
        ) {
            columns.sort_unstable();
            columns.dedup();
        }
    }
    let columns =
        ctx.caches.original_line_columns.get(&OriginalLineKey { source: source_idx, line })?;
    let idx = columns.partition_point(|mapped| *mapped < column);
    columns.get(idx).copied()
}

/// `originalEndPositionFor`: resolve the exclusive end of a generated range to
/// the start of the next original segment, or the end of the original line.
///
/// `gen_end_line` and `gen_end_col` are 0-based (srcmap space).
fn original_end_position_for(
    ctx: &mut RemapContext<'_>,
    gen_end_line: u32,
    gen_end_col: u32,
) -> Option<EndResult> {
    // beforeEnd = originalPositionTryBoth(line, column - 1). A column-0 exclusive
    // end would make istanbul evaluate `column - 1 === -1`, which
    // `@jridgewell/trace-mapping` rejects by throwing. Reporting "no mapping" is
    // the conservative equivalent: the caller drops the entry in drop mode, or
    // keeps the generated position in no-drop mode.
    let before_col = gen_end_col.checked_sub(1)?;
    let before = original_position_try_both(ctx.sm, gen_end_line, before_col)?;
    let source = ctx.sm.source(before.source);

    // afterEndMappings = allGeneratedPositionsFor(LUB) one column to the right;
    // map each back (GLB) and take the first that lands on the same original
    // line. That segment's start is the exclusive end of the span.
    let next_column = before.column.checked_add(1)?;
    let after = all_generated_positions_for_lub(ctx, source, before.line, next_column);
    for gen_pos in &after {
        if let Some(orig) = ctx.sm.original_position_for_with_bias(
            gen_pos.line,
            gen_pos.column,
            Bias::GreatestLowerBound,
        ) && orig.line == before.line
        {
            return Some(EndResult::Mapped {
                source: orig.source,
                line: orig.line,
                column: orig.column,
            });
        }
    }
    Some(EndResult::EndOfLine { source: before.source, line: before.line })
}

/// Resolve a `Location` through istanbul `getMapping` semantics.
///
/// Returns the remapped location in Istanbul space (1-based line, 0-based
/// column), or `None` where `getMapping` yields `null`: the start or the end
/// fails to map, or they map to different sources.
///
/// The `line: 0` "unknown" sentinel is handled by the callers, not here:
/// `getMapping` has no notion of it and `line - 1` would underflow.
fn get_mapping_location(loc: &Location, ctx: &mut RemapContext<'_>) -> Option<MappedLocation> {
    let start = original_position_try_both(ctx.sm, loc.start.line - 1, loc.start.column)?;
    let end = original_end_position_for(ctx, loc.end.line - 1, loc.end.column)?;

    let (end_source, mut end_line, mut end_col, end_is_eol) = match end {
        EndResult::Mapped { source, line, column } => (source, line, column, false),
        EndResult::EndOfLine { source, line } => {
            (source, line, ctx.caches.original_line_end_column(ctx.sm, source, line), true)
        }
    };

    // getMapping: both endpoints must carry a source and they must agree.
    if start.source != end_source {
        return None;
    }

    // Degenerate-span guard (get-mapping.js): a zero-area span at the same
    // line+column corrupts `keyFromLoc` merge dedup. Recompute the end via LUB
    // of the generated end, then step one column left. Skipped for the
    // end-of-line case: istanbul's `Infinity` can never equal `start.column`,
    // so the guard never fires there.
    if !end_is_eol && start.line == end_line && start.column == end_col {
        let lub = ctx.sm.original_position_for_with_bias(
            loc.end.line - 1,
            loc.end.column,
            Bias::LeastUpperBound,
        )?;
        end_line = lub.line;
        // get-mapping.js does `end.column -= 1` unconditionally; when LUB lands on
        // column 0 it yields a JS `-1`, which cannot round-trip through a `u32`
        // and is an invalid Istanbul position anyway, so it saturates to 0. The
        // sub-case needs the degenerate guard to fire and the recomputed LUB to
        // sit at column 0 on the same generated line; istanbul itself marks the
        // branch "edge case too hard to test for".
        end_col = lub.column.saturating_sub(1);
    }

    Some(MappedLocation {
        source: start.source,
        location: Location {
            start: Position { line: start.line + 1, column: start.column },
            end: Position { line: end_line + 1, column: end_col },
        },
    })
}

/// Direct per-position remap. Used for the `line: 0` "unknown" sentinel, which
/// `getMapping` has no notion of, and in no-drop mode for entries `getMapping`
/// cannot resolve, where the generated position is kept.
fn direct_remap_position(pos: &mut Position, sm: &SourceMap) {
    if pos.line == 0 {
        return;
    }
    let gen_line = pos.line - 1;
    if let Some(orig) = sm.original_position_for(gen_line, pos.column) {
        pos.line = orig.line + 1;
        pos.column = orig.column;
    }
}

/// Direct per-position remap of both endpoints. See [`direct_remap_position`].
pub fn direct_remap_location(loc: &mut Location, sm: &SourceMap) {
    direct_remap_position(&mut loc.start, sm);
    direct_remap_position(&mut loc.end, sm);
}

/// Resolve a `Location` through [`get_mapping_location`], memoised per
/// `(start, end)` pair for the lifetime of the caches.
pub fn get_mapped_location_cached(
    ctx: &mut RemapContext<'_>,
    loc: &Location,
) -> Option<MappedLocation> {
    let key = LocationKey::from(loc);
    if let Some(cached) = ctx.caches.mapping_cache.get(&key) {
        return cached.clone();
    }
    let remapped = get_mapping_location(loc, ctx);
    ctx.caches.mapping_cache.insert(key, remapped.clone());
    remapped
}

/// [`get_mapped_location_cached`] without the resolved source index.
pub fn get_mapping_location_cached(ctx: &mut RemapContext<'_>, loc: &Location) -> Option<Location> {
    get_mapped_location_cached(ctx, loc).map(|mapped| mapped.location)
}
