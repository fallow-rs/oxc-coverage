//! Edge cases for the V8 conversion entry point and the source-map trailer helpers.

use std::collections::BTreeMap;

use oxc_coverage_types::{BranchEntry, FileCoverage, Location, Position};
use oxc_coverage_v8::{
    ClassMemberSpans, V8CoverageRange, V8FunctionCoverage, apply_v8_coverage,
    apply_v8_coverage_with_class_members, extract_external_source_mapping_url,
    extract_inline_source_map,
};

// The `~~~~~~` payload packs into sextets that include slot 62, which the
// URL-safe alphabet spells `-`, so the decoder's `b'+' | b'-'` arm runs.
const SAMPLE_MAP: &str = r#"{"version":3,"sources":["foo.ts"],"mappings":"","note":"~~~~~~"}"#;

fn byte_len(source: &str) -> u32 {
    u32::try_from(source.len()).expect("fixture fits in u32")
}

fn utf16_len(source: &str) -> u32 {
    u32::try_from(source.encode_utf16().count()).expect("fixture fits in u32")
}

fn single_if_arm_coverage() -> FileCoverage {
    let mut coverage = FileCoverage {
        path: "arm.js".to_string(),
        statement_map: BTreeMap::new(),
        fn_map: BTreeMap::new(),
        branch_map: BTreeMap::new(),
        s: BTreeMap::new(),
        f: BTreeMap::new(),
        b: BTreeMap::new(),
        b_t: None,
        input_source_map: None,
        x_fallow_function_map: None,
    };
    coverage.branch_map.insert(
        "0".to_string(),
        BranchEntry {
            loc: Location {
                start: Position { line: 3, column: 0 },
                end: Position { line: 3, column: 11 },
            },
            line: 3,
            branch_type: "if".to_string(),
            locations: vec![Location {
                start: Position { line: 3, column: 4 },
                end: Position { line: 3, column: 6 },
            }],
        },
    );
    coverage.b.insert("0".to_string(), vec![0]);
    coverage
}

fn single_statement_coverage(location: Location) -> FileCoverage {
    let mut coverage = FileCoverage {
        path: "line.js".to_string(),
        statement_map: BTreeMap::new(),
        fn_map: BTreeMap::new(),
        branch_map: BTreeMap::new(),
        s: BTreeMap::new(),
        f: BTreeMap::new(),
        b: BTreeMap::new(),
        b_t: None,
        input_source_map: None,
        x_fallow_function_map: None,
    };
    coverage.statement_map.insert("0".to_string(), location);
    coverage.s.insert("0".to_string(), 0);
    coverage
}

/// Encode `bytes` with the URL-safe base64 alphabet (`-` / `_`) and no padding,
/// so the decoder's URL-safe arms are exercised.
fn base64_urlsafe(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);
        out.push(ALPHABET[(b0 >> 2) as usize] as char);
        out.push(ALPHABET[(((b0 & 0b11) << 4) | (b1 >> 4)) as usize] as char);
        if chunk.len() > 1 {
            out.push(ALPHABET[(((b1 & 0b1111) << 2) | (b2 >> 6)) as usize] as char);
        }
        if chunk.len() > 2 {
            out.push(ALPHABET[(b2 & 0b11_1111) as usize] as char);
        }
    }
    out
}

#[test]
fn extracts_inline_source_map_with_urlsafe_base64_alphabet() {
    let payload = base64_urlsafe(SAMPLE_MAP.as_bytes());
    assert!(
        payload.contains('-') || payload.contains('_'),
        "test fixture must exercise the URL-safe alphabet; got {payload}",
    );
    let source =
        format!("const x = 1;\n//# sourceMappingURL=data:application/json;base64,{payload}");
    let map = extract_inline_source_map(&source).expect("URL-safe base64 should decode");
    assert_eq!(map["version"], 3);
    assert_eq!(map["sources"][0], "foo.ts");
}

#[test]
fn extracts_inline_source_map_from_percent_encoded_payload() {
    // The non-base64 inline form: `encodeURIComponent` applied to the JSON, so
    // the decoder has to walk `%HH` escapes.
    let percent =
        "%7B%22version%22%3A3%2C%22sources%22%3A%5B%22foo.ts%22%5D%2C%22mappings%22%3A%22%22%7D";
    let source = format!("const x = 1;\n//# sourceMappingURL=data:application/json,{percent}");
    let map = extract_inline_source_map(&source).expect("percent payload should decode");
    assert_eq!(map["version"], 3);
    assert_eq!(map["sources"][0], "foo.ts");
}

#[test]
fn inline_source_map_returns_none_on_malformed_payloads() {
    // Trailer stops before the payload separator.
    assert!(extract_inline_source_map("//# sourceMappingURL=data:application/json").is_none());

    // Bytes outside both alphabets, and not valid JSON either.
    assert!(extract_inline_source_map("//# sourceMappingURL=data:application/json,@@@@").is_none());

    // A single base64 sextet cannot encode a whole byte.
    assert!(
        extract_inline_source_map("//# sourceMappingURL=data:application/json;base64,A").is_none()
    );

    // A `%HH` escape truncated by the end of the payload.
    assert!(
        extract_inline_source_map("//# sourceMappingURL=data:application/json,payload%").is_none()
    );
    assert!(
        extract_inline_source_map("//# sourceMappingURL=data:application/json,payload%4").is_none()
    );
}

#[test]
fn external_source_mapping_url_filters_data_urls_and_blanks() {
    assert!(
        extract_external_source_mapping_url(
            "x;\n//# sourceMappingURL=data:application/json;base64,e30=",
        )
        .is_none(),
        "inline data URLs belong to extract_inline_source_map",
    );

    assert!(extract_external_source_mapping_url("x;\n//# sourceMappingURL=").is_none());

    assert!(extract_external_source_mapping_url("x;\n//# sourceMappingURL=   ").is_none());

    assert_eq!(
        extract_external_source_mapping_url("x;\n//# sourceMappingURL=foo.js.map"),
        Some("foo.js.map"),
    );
}

#[test]
fn source_mapping_url_requires_a_final_directive_line() {
    let rejected = [
        "const value = '//# sourceMappingURL=string.map';",
        "const value = `\n//# sourceMappingURL=template.map\n`;",
        "const value = `\n//# sourceMappingURL=template.map`;",
        "//# sourceMappingURL=early.map\nrun();",
    ];
    for source in rejected {
        assert_eq!(
            extract_external_source_mapping_url(source),
            None,
            "marker is not a final directive line: {source:?}",
        );
    }

    assert_eq!(
        extract_external_source_mapping_url(
            "const value = 1;\r\n  //# sourceMappingURL=final.map\r\n\t  ",
        ),
        Some("final.map"),
    );
}

#[test]
fn inline_source_mapping_url_requires_a_final_directive_line() {
    let payload = base64_urlsafe(SAMPLE_MAP.as_bytes());
    let template =
        format!("const value = `\n//# sourceMappingURL=data:application/json;base64,{payload}\n`;");
    assert!(extract_inline_source_map(&template).is_none());

    let trailer = format!(
        "const value = 1;\n//# sourceMappingURL=data:application/json;base64,{payload}\n \t"
    );
    assert_eq!(extract_inline_source_map(&trailer).unwrap()["version"], 3);
}

#[test]
fn statement_counts_resolve_across_every_ecmascript_line_terminator() {
    for (name, terminator) in
        [("LF", "\n"), ("CRLF", "\r\n"), ("CR", "\r"), ("LS", "\u{2028}"), ("PS", "\u{2029}")]
    {
        let source = format!("a();{terminator}b(); tail();");
        let byte_start = source.find("b();").unwrap();
        let utf16_start = utf16_len(&source[..byte_start]);
        let mut coverage = single_statement_coverage(Location {
            start: Position { line: 2, column: 0 },
            end: Position { line: 2, column: 4 },
        });
        let functions = vec![V8FunctionCoverage {
            function_name: String::new(),
            ranges: vec![
                V8CoverageRange { start_offset: 0, end_offset: utf16_len(&source), count: 1 },
                V8CoverageRange {
                    start_offset: utf16_start,
                    end_offset: utf16_start + 4,
                    count: 9,
                },
            ],
            is_block_coverage: true,
        }];
        apply_v8_coverage(&mut coverage, &source, &functions, 0, &BTreeMap::new());
        assert_eq!(coverage.s["0"], 9, "{name}");
    }
}

#[test]
fn wrapper_length_uses_utf16_code_units_for_non_ascii_wrappers() {
    let wrapper = "/* 🦀 */";
    let wrapper_length_utf16 = utf16_len(wrapper);
    assert_ne!(wrapper_length_utf16, byte_len(wrapper), "fixture distinguishes UTF-16 from bytes");

    let source = "x();";
    let mut coverage = single_statement_coverage(Location {
        start: Position { line: 1, column: 0 },
        end: Position { line: 1, column: 4 },
    });
    let functions = [V8FunctionCoverage {
        function_name: String::new(),
        ranges: vec![V8CoverageRange {
            start_offset: wrapper_length_utf16,
            end_offset: wrapper_length_utf16 + utf16_len(source),
            count: 7,
        }],
        is_block_coverage: true,
    }];

    apply_v8_coverage(&mut coverage, source, &functions, wrapper_length_utf16, &BTreeMap::new());

    assert_eq!(coverage.s["0"], 7, "the wrapper offset is measured in UTF-16 code units");
}

#[test]
fn arm_tiebreaker_prefers_tighter_v8_range_at_equal_distance() {
    // Branch arm on line 3, columns 4..6, mapping to byte offsets [26, 28).
    // Two V8 block ranges flank it within the 4-byte tolerance:
    //   A: [25, 28) -> distance 1+0 = 1, width 3
    //   B: [26, 27) -> distance 0+1 = 1, width 1
    // Same total distance, but B is the narrower (more block-specific) range,
    // so its count must win. Child ranges are appended in `A, B` order so the
    // tie-breaker actually has to replace `best`.
    let source = "let x = 1;\nlet y = 2;\nif (xx) y;\n";

    let mut fc = single_if_arm_coverage();

    let wider = V8CoverageRange { start_offset: 25, end_offset: 28, count: 7 };
    let tighter = V8CoverageRange { start_offset: 26, end_offset: 27, count: 99 };
    let functions = vec![V8FunctionCoverage {
        function_name: String::new(),
        ranges: vec![
            V8CoverageRange { start_offset: 0, end_offset: byte_len(source), count: 1 },
            wider,
            tighter,
        ],
        is_block_coverage: true,
    }];

    apply_v8_coverage(&mut fc, source, &functions, 0, &BTreeMap::new());
    assert_eq!(fc.b["0"][0], 99, "tighter range must win the tie-breaker for arm count");
}

#[test]
fn if_arm_without_body_span_does_not_inherit_enclosing_count() {
    let source = "let x = 1;\nlet y = 2;\nif (xx) y;\n";
    let mut fc = single_if_arm_coverage();
    let functions = vec![V8FunctionCoverage {
        function_name: String::new(),
        ranges: vec![V8CoverageRange { start_offset: 0, end_offset: byte_len(source), count: 5 }],
        is_block_coverage: true,
    }];

    apply_v8_coverage(&mut fc, source, &functions, 0, &BTreeMap::new());
    assert_eq!(fc.b["0"][0], 0, "missing body span must keep inheritance conservative");
}

#[test]
fn tight_zero_arm_range_wins_before_enclosing_inheritance() {
    let source = "let x = 1;\nlet y = 2;\nif (xx) y;\n";
    let mut fc = single_if_arm_coverage();
    let functions = vec![V8FunctionCoverage {
        function_name: String::new(),
        ranges: vec![
            V8CoverageRange { start_offset: 0, end_offset: byte_len(source), count: 5 },
            V8CoverageRange { start_offset: 25, end_offset: 27, count: 0 },
        ],
        is_block_coverage: true,
    }];
    let body_spans = BTreeMap::from([("0".to_string(), vec![(26, 28)])]);

    apply_v8_coverage(&mut fc, source, &functions, 0, &body_spans);
    assert_eq!(fc.b["0"][0], 0, "a tight zero range must not fall through to the outer count");
}

#[test]
fn zero_width_synthetic_else_does_not_match_adjacent_then_range() {
    const SYNTHETIC_ANCHOR: u32 = 21;

    let source = "function f(x){if(x){}}\n";
    let mut fc = single_if_arm_coverage();
    fc.branch_map.get_mut("0").unwrap().locations[0] = Location {
        start: Position { line: 1, column: SYNTHETIC_ANCHOR },
        end: Position { line: 1, column: SYNTHETIC_ANCHOR },
    };
    let functions = vec![V8FunctionCoverage {
        function_name: "f".to_string(),
        ranges: vec![
            V8CoverageRange { start_offset: 0, end_offset: byte_len(source), count: 5 },
            V8CoverageRange { start_offset: 19, end_offset: SYNTHETIC_ANCHOR, count: 3 },
        ],
        is_block_coverage: true,
    }];
    let body_spans =
        BTreeMap::from([("0".to_string(), vec![(SYNTHETIC_ANCHOR, SYNTHETIC_ANCHOR)])]);

    apply_v8_coverage(&mut fc, source, &functions, 0, &body_spans);
    assert_eq!(fc.b["0"][0], 0, "synthetic else must remain uncovered");
}

#[test]
fn oversized_column_cannot_cross_into_a_later_line() {
    let source = "a();\nlate();\n";
    let mut fc = FileCoverage {
        path: "manual-location.js".to_string(),
        statement_map: BTreeMap::from([(
            "0".to_string(),
            Location {
                start: Position { line: 1, column: 100 },
                end: Position { line: 1, column: 100 },
            },
        )]),
        fn_map: BTreeMap::new(),
        branch_map: BTreeMap::new(),
        s: BTreeMap::from([("0".to_string(), 0)]),
        f: BTreeMap::new(),
        b: BTreeMap::new(),
        b_t: None,
        input_source_map: None,
        x_fallow_function_map: None,
    };
    let functions = [V8FunctionCoverage {
        function_name: String::new(),
        ranges: vec![
            V8CoverageRange { start_offset: 0, end_offset: 4, count: 1 },
            V8CoverageRange { start_offset: 5, end_offset: 13, count: 9 },
        ],
        is_block_coverage: true,
    }];

    apply_v8_coverage(&mut fc, source, &functions, 0, &BTreeMap::new());

    assert_eq!(fc.s["0"], 1, "invalid line-one column must clamp before line two");
}

/// The V8 records of Node.js 22 for a class with one instance field and one
/// static field. Both initializers have the class body as outer range.
fn same_span_initializers(source: &str) -> Vec<V8FunctionCoverage> {
    let body = V8CoverageRange { start_offset: 8, end_offset: 37, count: 0 };
    vec![
        V8FunctionCoverage {
            function_name: String::new(),
            ranges: vec![V8CoverageRange {
                start_offset: 0,
                end_offset: byte_len(source),
                count: 7,
            }],
            is_block_coverage: true,
        },
        V8FunctionCoverage {
            function_name: "<static_initializer>".to_string(),
            ranges: vec![V8CoverageRange { count: 1, ..body }],
            is_block_coverage: true,
        },
        V8FunctionCoverage {
            function_name: "<instance_members_initializer>".to_string(),
            ranges: vec![V8CoverageRange { count: 4, ..body }],
            is_block_coverage: true,
        },
    ]
}

#[test]
fn class_member_spans_select_the_initializer_range() {
    let source = "class A {\n  a = 1;\n  static b = 2;\n}\n";
    let functions = same_span_initializers(source);
    let instance_value = (byte_len("class A {\n  a = "), byte_len("class A {\n  a = 1"));
    let static_value = (
        byte_len("class A {\n  a = 1;\n  static b = "),
        byte_len("class A {\n  a = 1;\n  static b = 2"),
    );
    let members = ClassMemberSpans::new(vec![static_value], vec![instance_value], Vec::new());
    let count_at = |line: u32, column: u32, members: Option<&ClassMemberSpans>| {
        let mut fc = single_statement_coverage(Location {
            start: Position { line, column },
            end: Position { line, column: column + 1 },
        });
        match members {
            Some(members) => apply_v8_coverage_with_class_members(
                &mut fc,
                source,
                &functions,
                0,
                &BTreeMap::new(),
                members,
            ),
            None => apply_v8_coverage(&mut fc, source, &functions, 0, &BTreeMap::new()),
        }
        fc.s["0"]
    };

    assert_eq!(count_at(2, 6, Some(&members)), 4, "an instance value takes the instance count");
    assert_eq!(count_at(3, 13, Some(&members)), 1, "a static value takes the static count");
    assert_eq!(count_at(3, 9, Some(&members)), 4, "code in no member takes the instance range");
    let key =
        (byte_len("class A {\n  a = 1;\n  static "), byte_len("class A {\n  a = 1;\n  static b"));
    let with_key = ClassMemberSpans::new(vec![static_value], vec![instance_value], vec![key]);
    assert_eq!(count_at(3, 9, Some(&with_key)), 7, "a computed key skips both initializer ranges");
    assert_eq!(count_at(2, 6, None), 1, "with no member spans, the first record wins");
}
