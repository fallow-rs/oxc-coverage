//! Standalone position-remap predicate for the eager instrument-time drop gate.

use std::cell::RefCell;

use oxc_coverage_types::{FileCoverage, Location, Position};
use srcmap_sourcemap::SourceMap;

use crate::{
    apply::apply_source_map_single_result_with_caches,
    context::{RemapCaches, RemapContext},
    get_mapping::{get_mapped_location_cached, get_mapping_location_cached},
    options::RemapOptions,
    sources::has_resolved_source,
};

/// Generated-line adjustment applied before output-map composition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GeneratedLineShift {
    /// First generated line whose mappings move down.
    pub first_following_line: u32,
    /// Number of generated lines inserted before `first_following_line`.
    pub line_delta: u32,
}

/// A prepared, parsed `inputSourceMap` with reusable lookup caches.
///
/// Lets the instrument crate decide, at AST-transform time, whether a coverage
/// point's positions remap through the input source map, then reuse the same
/// parsed map for full coverage remapping and generated-map composition without
/// depending on `srcmap-sourcemap` internals.
pub struct PositionRemapper {
    sm: SourceMap,
    /// Lookup caches shared across every `location_maps` call for this map. The
    /// eager gate queries one node at a time on a hot traversal path, so
    /// without a persistent context the per-`(source, line)` column index would
    /// be rebuilt and discarded on every node. `RefCell` because
    /// `location_maps` takes `&self`: the transform visits with `&self`.
    caches: RefCell<RemapCaches>,
}

impl PositionRemapper {
    /// Parse a source map from JSON.
    ///
    /// Returns `None` when the JSON fails to parse, or when none of its
    /// `sources` entries resolves to a path.
    #[must_use]
    pub fn from_json(input_sm_json: &str) -> Option<Self> {
        let sm = SourceMap::from_json(input_sm_json).ok()?;
        if !has_resolved_source(&sm) {
            return None;
        }
        Some(Self { sm, caches: RefCell::new(RemapCaches::default()) })
    }

    /// Whether an istanbul `Location` survives `getMapping` resolution, that is
    /// whether `remap_coverage_with_options(.., RemapOptions { drop_unmapped:
    /// true })` would keep the entry.
    ///
    /// The eager AST-level drop gate consults this so it agrees with the
    /// deferred prune by construction: both ask `get_mapping_location`, so a
    /// span that resolves for one resolves for the other. The location is not
    /// mutated; the gate needs only the keep/drop decision, and the position
    /// rewrite happens later in the no-drop `remap_coverage` pass.
    ///
    /// The `line == 0` "unknown" sentinel is a keep no-op, matching the direct
    /// lookup the remap path routes it to.
    #[must_use]
    pub fn location_maps(&self, loc: &Location) -> bool {
        if loc.start.line == 0 || loc.end.line == 0 {
            return self.direct_position_maps(&loc.start) && self.direct_position_maps(&loc.end);
        }
        let mut caches = self.caches.borrow_mut();
        let mut ctx = RemapContext::new(&self.sm, &mut caches);
        get_mapping_location_cached(&mut ctx, loc).is_some()
    }

    /// The `getMapping`-resolved source index and original location for `loc`,
    /// or `None` when resolution fails or either endpoint carries the
    /// `line == 0` sentinel, which `getMapping` has no notion of.
    ///
    /// The eager gate keys coverage points on this so that generated spans
    /// collapsing onto one original location share a counter slot, the same
    /// fold the canonicalizing remap applies through `merge_file_coverage`.
    #[must_use]
    pub fn remap_location(&self, loc: &Location) -> Option<(u32, Location)> {
        if loc.start.line == 0 || loc.end.line == 0 {
            return None;
        }
        let mut caches = self.caches.borrow_mut();
        let mut ctx = RemapContext::new(&self.sm, &mut caches);
        get_mapped_location_cached(&mut ctx, loc).map(|mapped| (mapped.source, mapped.location))
    }

    /// Remap one coverage object through this already parsed source map.
    ///
    /// Lookup caches populated by transform-time keep/drop checks are reused by
    /// the full remap. Multi-source results remain `None`, matching
    /// [`crate::remap_coverage`].
    #[must_use]
    pub fn remap_coverage(&self, coverage: &FileCoverage) -> Option<FileCoverage> {
        let mut caches = self.caches.borrow_mut();
        apply_source_map_single_result_with_caches(
            coverage,
            &self.sm,
            RemapOptions::default(),
            &mut caches,
        )
    }

    /// `true` when the istanbul position (1-based `line`, 0-based UTF-16
    /// `column`) has a direct greatest-lower-bound mapping, or is the
    /// `line == 0` sentinel.
    fn direct_position_maps(&self, pos: &Position) -> bool {
        pos.line == 0 || self.sm.original_position_for(pos.line - 1, pos.column).is_some()
    }
}

/// Finalize a generated source map and compose it with an optional prepared
/// input map.
///
/// `prepared_input` takes precedence over `input_source_map_json`. The raw JSON
/// fallback preserves behavior for parseable maps without a resolved source,
/// which [`PositionRemapper::from_json`] intentionally rejects for coverage
/// remapping.
#[must_use]
pub fn finalize_generated_source_map(
    output_json: &str,
    shift: Option<GeneratedLineShift>,
    prepared_input: Option<&PositionRemapper>,
    input_source_map_json: Option<&str>,
) -> String {
    let Ok(mut output_sm) = SourceMap::from_json(output_json) else {
        return output_json.to_string();
    };
    if let Some(shift) = shift {
        let mappings = output_sm
            .all_mappings()
            .iter()
            .copied()
            .map(|mut mapping| {
                if mapping.generated_line >= shift.first_following_line {
                    mapping.generated_line =
                        mapping.generated_line.saturating_add(shift.line_delta);
                }
                mapping
            })
            .collect();
        output_sm = SourceMap::from_parts_with_extensions(
            output_sm.file.clone(),
            output_sm.source_root.clone(),
            output_sm.sources.clone(),
            output_sm.sources_content.clone(),
            output_sm.names.clone(),
            mappings,
            output_sm.ignore_list.clone(),
            output_sm.debug_id.clone(),
            output_sm.scopes.clone(),
            output_sm.extensions.clone(),
        );
    }

    let parsed_input = if prepared_input.is_none() {
        input_source_map_json.and_then(|json| SourceMap::from_json(json).ok())
    } else {
        None
    };
    let input_sm = prepared_input.map(|prepared| &prepared.sm).or(parsed_input.as_ref());
    if let Some(input_sm) = input_sm {
        let composed = srcmap_remapping::remap_ref(&output_sm, |_name: &str| Some(input_sm));
        return composed.to_json();
    }

    output_sm.to_json()
}
