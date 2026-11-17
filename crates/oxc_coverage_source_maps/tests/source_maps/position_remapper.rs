//! `PositionRemapper`, the eager instrument-time keep/drop predicate.

use oxc_coverage_source_maps::{
    GeneratedLineShift, PositionRemapper, finalize_generated_source_map, remap_coverage,
};

use crate::fixtures::{full_shape_file_coverage, identity_three_line_map, loc};

#[test]
fn location_maps_matches_deferred_get_mapping_keep_decision() {
    // `AAAA;EACA`: gen(1,0)->orig(1,0) ; gen(2,2)->orig(2,0). Line 2's only
    // mapping sits at column 2, AFTER a statement that starts at column 0.
    let input_sm = r#"{"version":3,"sources":["src/app.ts"],"sourcesContent":["const a = 1;\nconst b = 2;\n"],"mappings":"AAAA;EACA","names":[]}"#;
    let remapper = PositionRemapper::from_json(input_sm).expect("usable map");

    // Line 1 maps cleanly at column 0.
    assert!(remapper.location_maps(&loc(1, 0, 1, 4)), "line-1 statement maps");

    // The line-2 statement at column 0 has no mapping at-or-before it on its
    // line (the only line-2 mapping is at column 2), but `getMapping`'s
    // least-upper-bound fallback resolves it forward and keeps it. The deferred
    // `drop_unmapped` path keeps it, so the eager gate must too.
    assert!(
        remapper.location_maps(&loc(2, 0, 2, 4)),
        "col-0 statement whose line maps at col 2 is kept (LUB fallback)",
    );

    // Line 3 has no mapping at all (the map only covers generated lines 1-2):
    // both GLB and LUB miss, so the span is genuinely unmapped and dropped.
    assert!(
        !remapper.location_maps(&loc(3, 0, 3, 4)),
        "a span on a generated line with no mappings is dropped",
    );

    // The `line == 0` "unknown" sentinel is kept as a no-op, matching the
    // direct-lookup route the remap path sends it down.
    assert!(remapper.location_maps(&loc(0, 0, 0, 0)), "line-0 sentinel is a keep no-op");
}

#[test]
fn admits_map_when_a_later_source_resolves() {
    let input_sm = r#"{"version":3,"sources":["","src/app.ts"],"sourcesContent":[null,"const a = 1;\n"],"mappings":"ACAA","names":[]}"#;
    let remapper = PositionRemapper::from_json(input_sm)
        .expect("a map with a usable later source is admitted like deferred remapping");

    assert!(remapper.location_maps(&loc(1, 0, 1, 4)), "source index 1 maps");
}

#[test]
fn prepared_map_reuses_the_same_remap_semantics() {
    let input_map = identity_three_line_map(None);
    let input_json = serde_json::to_string(&input_map).expect("fixture serializes");
    let remapper = PositionRemapper::from_json(&input_json).expect("usable map");
    let embedded = full_shape_file_coverage(Some(input_map));
    let mut prepared_input = embedded.clone();
    prepared_input.input_source_map = None;

    let prepared = remapper.remap_coverage(&prepared_input).expect("prepared remap succeeds");
    let embedded = remap_coverage(&embedded).expect("embedded remap succeeds");
    assert_eq!(
        serde_json::to_value(prepared).expect("prepared coverage serializes"),
        serde_json::to_value(embedded).expect("embedded coverage serializes"),
        "prepared remapping must match the embedded-map API",
    );
}

#[test]
fn prepared_map_composes_output_after_generated_line_shift() {
    let input_json = serde_json::to_string(&identity_three_line_map(None))
        .expect("input map fixture serializes");
    let remapper = PositionRemapper::from_json(&input_json).expect("usable map");
    let output_json = r#"{"version":3,"sources":["intermediate.js"],"sourcesContent":["a\nb\nc\n"],"mappings":"AAAA;AACA;AACA","names":[]}"#;
    let shift = GeneratedLineShift { first_following_line: 1, line_delta: 2 };

    let prepared =
        finalize_generated_source_map(output_json, Some(shift), Some(&remapper), Some("invalid"));
    let raw = finalize_generated_source_map(output_json, Some(shift), None, Some(&input_json));

    assert_eq!(prepared, raw, "prepared and raw composition paths must remain byte-equal");
}

#[test]
fn composed_map_traces_each_segment_to_the_original_source() {
    // Input map, intermediate line 1: col 0 -> src/app.ts(1,0), col 4 has no
    // source, col 8 -> src/app.ts(1,8).
    let input_json = r#"{"version":3,"sources":["src/app.ts"],"sourcesContent":["const a = 1;\n"],"mappings":"AAAA,I,IAAQ","names":[]}"#;
    // Output map, generated line 1: col 0 -> intermediate(1,0),
    // col 2 -> intermediate(1,5), col 6 -> intermediate(1,9).
    let output_json = r#"{"version":3,"sources":["intermediate.js"],"sourcesContent":["const a = 1;\n"],"mappings":"AAAA,EAAK,IAAI","names":[]}"#;

    let composed = finalize_generated_source_map(output_json, None, None, Some(input_json));
    let composed: serde_json::Value = serde_json::from_str(&composed).expect("composed map parses");

    assert_eq!(composed["sources"], serde_json::json!(["src/app.ts"]));
    assert_eq!(composed["sourcesContent"], serde_json::json!(["const a = 1;\n"]));
    // The segment at col 2 lands on the input segment without a source, so it
    // stays in the result without a source. The mapping at col 0 must not
    // extend over it.
    assert_eq!(composed["mappings"], "AAAA,E,IAAQ");
}
