//! Integration tests for the V8-to-Istanbul converter.
//!
//! Each test feeds a small JS snippet plus a V8 coverage report and checks
//! that the resulting Istanbul `FileCoverage` carries the right statement,
//! function and branch hit counts. Tests named `real_v8_*` use ranges from a
//! real `node:inspector` capture; the other reports are written by hand.

use cow_utils::CowUtils;

use oxc_coverage_instrument::{
    V8CoverageRange, V8FunctionCoverage, V8ToIstanbulError, v8_to_istanbul,
    v8_to_istanbul_with_loader,
};

fn range(start: u32, end: u32, count: u32) -> V8CoverageRange {
    V8CoverageRange { start_offset: start, end_offset: end, count }
}

/// Byte length of `source` as a V8 offset.
fn byte_len(source: &str) -> u32 {
    u32::try_from(source.len()).unwrap()
}

/// UTF-16 code-unit length of `source`, the unit V8 reports offsets in.
fn utf16_len(source: &str) -> u32 {
    u32::try_from(source.encode_utf16().count()).unwrap()
}

/// Byte offset of the first occurrence of `needle` in `source`.
fn byte_offset_of(source: &str, needle: &str) -> u32 {
    u32::try_from(source.find(needle).expect("needle occurs in source")).unwrap()
}

/// Byte offset of the last occurrence of `needle` in `source`.
fn last_byte_offset_of(source: &str, needle: &str) -> u32 {
    u32::try_from(source.rfind(needle).expect("needle occurs in source")).unwrap()
}

fn function(name: &str, ranges: Vec<V8CoverageRange>, is_block: bool) -> V8FunctionCoverage {
    V8FunctionCoverage { function_name: name.to_string(), ranges, is_block_coverage: is_block }
}

const REAL_V8_IF_FUNCTION_SOURCE: &str =
    "function f(x) { if (x) { return 1; } else { return -1; } }";
const REAL_V8_FUNCTION_END: u32 = 58;
const REAL_V8_THEN_START: u32 = 23;
const REAL_V8_THEN_END: u32 = 36;
const REAL_V8_ELSE_START: u32 = 36;
const REAL_V8_ELSE_END: u32 = 56;

fn assert_real_v8_if_counts(calls: &str, ranges: Vec<V8CoverageRange>, expected: &[u32]) {
    assert_real_v8_if_functions(calls, &[function("f", ranges, true)], expected);
}

fn assert_real_v8_if_functions(calls: &str, functions: &[V8FunctionCoverage], expected: &[u32]) {
    let source = format!("{REAL_V8_IF_FUNCTION_SOURCE} {calls}\n");
    assert_if_counts(&source, functions, expected);
}

fn assert_if_counts(source: &str, functions: &[V8FunctionCoverage], expected: &[u32]) {
    let fc = v8_to_istanbul(source, "real-inspector-if.js", functions, 0).unwrap();
    assert_eq!(arm_counts(&fc, "if"), expected);
}

/// The hit vector of the first `branch_type` branch in `fc`.
fn arm_counts(fc: &oxc_coverage_instrument::FileCoverage, branch_type: &str) -> Vec<u32> {
    fc.branch_map
        .iter()
        .find(|(_, branch)| branch.branch_type == branch_type)
        .and_then(|(id, _)| fc.b.get(id).cloned())
        .unwrap_or_else(|| panic!("{branch_type} branch must appear in branchMap"))
}

#[test]
fn assigns_function_count_from_outer_range() {
    // Three lines, one statement each. The V8 module-level range covers the
    // whole file with count = 1 (the module ran once).
    let source = "const x = 1;\nconst y = 2;\nconst z = 3;\n";
    let end = byte_len(source);
    let functions = vec![function("", vec![range(0, end, 1)], false)];

    let fc = v8_to_istanbul(source, "app.js", &functions, 0).unwrap();

    // Every statement falls inside the module range, so each gets count = 1.
    let s_values: Vec<u32> = fc.s.values().copied().collect();
    assert!(!s_values.is_empty(), "expected statementMap entries");
    assert!(s_values.iter().all(|&c| c == 1), "all statements should be covered: {s_values:?}");
}

#[test]
fn unexecuted_block_overrides_outer_count() {
    // Outer function range count = 1. Inner block range (the `if` body) has
    // count = 0. The "y = 2" statement sits inside that inner range and must
    // adopt count 0, not 1.
    let source = "function f() {\n  if (false) {\n    const y = 2;\n  }\n}\nf();\n";
    let module_end = byte_len(source);
    // Locate the `if` body block; the test only needs a range tightly
    // around the `const y = 2;` statement.
    let inner_start = byte_offset_of(source, "    const y");
    let inner_end = byte_offset_of(source, "  }");
    let functions =
        vec![function("f", vec![range(0, module_end, 1), range(inner_start, inner_end, 0)], true)];

    let fc = v8_to_istanbul(source, "branchy.js", &functions, 0).unwrap();

    // Find the statement entry for `const y = 2;` and verify its count is 0.
    let y_id = fc
        .statement_map
        .iter()
        .find(|(_, loc)| loc.start.line == 3)
        .map(|(id, _)| id.clone())
        .expect("statementMap should hold the inner declaration");
    assert_eq!(fc.s.get(&y_id).copied().unwrap_or(99), 0, "inner block count must override outer");
}

#[test]
fn returns_empty_when_no_coverage_ranges_apply() {
    // No ranges at all: every statement count should be 0.
    let source = "const x = 1;";
    let fc = v8_to_istanbul(source, "empty.js", &[], 0).unwrap();
    assert!(fc.s.values().all(|&c| c == 0), "no ranges, no hits");
}

#[test]
fn applies_utf16_wrapper_base_for_shifted_producers() {
    let source = "const marker = '😀é';\r\nconst y = 2;\n";
    let module_end = utf16_len(source);
    // Inspector coverage is normally source-relative. Some other producers
    // report a wrapper-shifted range and pass that UTF-16 base explicitly.
    let wrapper_length = 62;
    let functions =
        vec![function("", vec![range(wrapper_length, wrapper_length + module_end, 1)], false)];

    let fc = v8_to_istanbul(source, "cjs.js", &functions, wrapper_length).unwrap();
    let s_values: Vec<u32> = fc.s.values().copied().collect();
    assert!(s_values.iter().all(|&c| c == 1), "wrapper base must be applied: {s_values:?}");
}

#[test]
fn assigns_branch_arm_counts_from_block_coverage() {
    // V8 block coverage emits one inner range per `{ ... }` BlockStatement,
    // not per inner statement. The arm resolver matches both BlockStatement
    // ranges against the collected consequent / alternate body spans, even
    // though istanbul records `branchMap.locations[0]` as the whole
    // `IfStatement` span (see istanbul-lib-instrument's coverIfBranches).
    // arm[0] therefore reflects "number of times the predicate was truthy",
    // which equals the then-block's hit count.
    let source = "function f(x) {\n  if (x) {\n    a();\n  } else {\n    b();\n  }\n}\nf(true);\n";
    let module_end = byte_len(source);
    let then_start = byte_offset_of(source, "if (x) {") + 7;
    let then_end = byte_offset_of(source, "} else") + 1;
    let else_start = byte_offset_of(source, "else {") + 5;
    let else_end = last_byte_offset_of(source, "\n  }") + 4;

    let functions = vec![function(
        "f",
        vec![
            range(0, module_end, 1),
            range(then_start, then_end, 1),
            range(else_start, else_end, 0),
        ],
        true,
    )];

    let fc = v8_to_istanbul(source, "ifelse.js", &functions, 0).unwrap();
    let arm_counts = arm_counts(&fc, "if");
    assert_eq!(arm_counts.len(), 2, "if has two arms");
    assert_eq!(
        arm_counts[0], 1,
        "arm[0] resolves through the collected then-block body span; predicate truthy once"
    );
    assert_eq!(arm_counts[1], 0, "else arm should report zero hits");
}

#[test]
fn ternary_arm_without_a_range_gets_the_parent_count() {
    // V8 removes an arm range whose count equals the count of its parent
    // range. The arm with no range therefore gets the parent count minus the
    // other arms: 1 - 0 for the first arm, and 0 for the second arm.
    let source = "function f(x) { return x ? a() : b(); }\nf(true);\n";
    let end = byte_len(source);
    let functions = vec![function("f", vec![range(0, end, 1)], true)];

    let fc = v8_to_istanbul(source, "ternary.js", &functions, 0).unwrap();
    assert_eq!(arm_counts(&fc, "cond-expr"), vec![1, 0]);
}

#[test]
fn function_only_v8_coverage_does_not_infer_ternary_arm_counts() {
    let source = "function f(x) { return x ? a() : b(); }\nf(true);\n";
    let end = byte_len(source);
    let functions = vec![function("f", vec![range(0, end, 1)], false)];

    let fc = v8_to_istanbul(source, "ternary.js", &functions, 0).unwrap();
    assert_eq!(arm_counts(&fc, "cond-expr"), vec![0, 0]);
}

/// Source of the `node:inspector` capture in [`branch_arm_v8_functions`].
const BRANCH_ARMS_SOURCE: &str = include_str!("../fixtures/v8/branch-arms.js");

/// `Profiler.takePreciseCoverage` output (`callCount: true, detailed: true`)
/// for [`BRANCH_ARMS_SOURCE`] under Node 24.20, run as a `vm` script.
fn branch_arm_v8_functions() -> Vec<V8FunctionCoverage> {
    vec![
        function(
            "",
            vec![
                range(0, 796, 1),
                range(601, 795, 4),
                range(667, 673, 1),
                range(674, 677, 3),
                range(697, 708, 1),
                range(709, 714, 3),
                range(746, 749, 3),
                range(750, 753, 1),
            ],
            true,
        ),
        function("pick", vec![range(0, 50, 4), range(36, 41, 3), range(42, 47, 1)], true),
        function("always", vec![range(51, 103, 4), range(95, 100, 0)], true),
        function("either", vec![range(104, 146, 4)], true),
        function("fallback", vec![range(147, 191, 4), range(184, 188, 1)], true),
        function("greet", vec![range(192, 241, 4)], true),
        function("guard", vec![range(242, 305, 4), range(275, 291, 1)], true),
        function("kind", vec![range(306, 412, 4), range(344, 371, 3), range(376, 406, 1)], true),
        function("only", vec![range(413, 519, 4), range(483, 513, 0)], true),
        function("nested", vec![range(520, 572, 4), range(558, 569, 3), range(564, 568, 2)], true),
    ]
}

/// The arm counts of every branch in `fc`, in source order.
fn arm_counts_by_line(fc: &oxc_coverage_instrument::FileCoverage) -> Vec<(u32, &str, Vec<u32>)> {
    let mut found: Vec<_> = fc
        .branch_map
        .iter()
        .map(|(id, branch)| {
            let start = &branch.loc.start;
            ((start.line, start.column), branch.branch_type.as_str(), fc.b[id].clone())
        })
        .collect();
    found.sort_by_key(|(start, _, _)| *start);
    found.into_iter().map(|((line, _), kind, counts)| (line, kind, counts)).collect()
}

#[test]
fn real_v8_branch_arms_without_a_range_get_inferred_counts() {
    assert_eq!(utf16_len(BRANCH_ARMS_SOURCE), 796, "the fixture matches the capture");
    let fc = v8_to_istanbul(BRANCH_ARMS_SOURCE, "branch-arms.js", &branch_arm_v8_functions(), 0)
        .unwrap();

    // Each row is (line, type, arm counts). The counts equal the
    // istanbul-lib-instrument counts for the same workload, except the
    // `default-arg` arm: V8 has no range for a default value, so the arm gets
    // the call count of the function (4) where Istanbul counts the 1 call
    // that used the default.
    assert_eq!(
        arm_counts_by_line(&fc),
        [
            // Both arms have a V8 range.
            (2, "cond-expr", vec![3, 1]),
            // V8 removed the `?` arm range (count 4 equals the function count).
            (5, "cond-expr", vec![4, 0]),
            // The left operand has no range; the right range was removed.
            (8, "binary-expr", vec![4, 4]),
            // `??`: the left operand gets the function count.
            (11, "binary-expr", vec![4, 1]),
            (13, "default-arg", vec![4]),
            // The synthetic `else` gets the parent count minus the `then` count.
            (17, "if", vec![1, 3]),
            (23, "switch", vec![3, 1]),
            // V8 removed the `case 1` range (count 4 equals the function count).
            (31, "switch", vec![4, 0]),
            // `b` gets the count of the `|| (b && c)` range that contains it.
            (39, "binary-expr", vec![4, 3, 2]),
            (45, "cond-expr", vec![1, 3]),
            (46, "cond-expr", vec![1, 3]),
            (48, "cond-expr", vec![3, 1]),
        ]
    );
}

#[test]
fn real_v8_arm_next_to_an_ignored_arm_does_not_take_the_remainder() {
    // A pragma removes one arm. Its count is unknown, so the other arm cannot
    // take "parent minus the other arms". `node:inspector` output for the
    // calls at the end of the source (Node 24.20).
    let source = "function g(x) {\n  /* istanbul ignore if */\n  if (x) {\n    x = 0;\n  }\n  return x;\n}\nfunction t(x) {\n  return x ? /* istanbul ignore next */ 'a' : 'b';\n}\ng(0); g(0); g(1);\nt(0); t(0);\n";
    assert_eq!(utf16_len(source), 182, "the source matches the capture");
    let functions = [
        function("", vec![range(0, 182, 1)], true),
        function("g", vec![range(0, 82, 3), range(52, 68, 1)], true),
        function("t", vec![range(83, 151, 2), range(110, 142, 0)], true),
    ];

    let fc = v8_to_istanbul(source, "pragma.js", &functions, 0).unwrap();
    // The synthetic `else` has no source text to inherit from, so it stays 0.
    // Istanbul counts 2.
    assert_eq!(arm_counts(&fc, "if"), vec![0]);
    // V8 removed the `: 'b'` range, so the arm inherits the function count.
    assert_eq!(arm_counts(&fc, "cond-expr"), vec![2]);
}

/// Assert the arm counts of every branch, in source order, for a real
/// `node:inspector` capture (Node 24.20, `callCount: true, detailed: true`,
/// run as a `vm` script). Unless a test says otherwise, the expected counts
/// are the istanbul-lib-instrument counts for the same workload.
fn assert_real_v8_branches(
    source: &str,
    functions: &[V8FunctionCoverage],
    expected: &[(u32, &str, Vec<u32>)],
) {
    let fc = v8_to_istanbul(source, "case.js", functions, 0).unwrap();
    assert_eq!(arm_counts_by_line(&fc), expected);
}

#[test]
fn real_v8_ternary_line_break_after_operator() {
    // A line break after `?` and `:` moves the arm start away from the V8 range start.
    assert_real_v8_branches(
        "function f(x) {\n  return x ?\n      'yes' :\n      'no';\n}\nfor (let i=0;i<10;i++) f(i%3);",
        &[
            function("", vec![range(0, 87, 1), range(80, 87, 10)], true),
            function("f", vec![range(0, 56, 10), range(27, 40, 6), range(41, 53, 4)], true),
        ],
        &[(2, "cond-expr", vec![6, 4])],
    );
}

#[test]
fn real_v8_ternary_line_break_before_operator() {
    // A line break before `?` and `:` moves the previous operand end away from the V8 range start.
    assert_real_v8_branches(
        "function f(x) {\n  return x\n    ? 'yes'\n    : 'no';\n}\nfor (let i=0;i<10;i++) f(i%3);",
        &[
            function("", vec![range(0, 83, 1), range(76, 83, 10)], true),
            function("f", vec![range(0, 52, 10), range(31, 38, 6), range(43, 49, 4)], true),
        ],
        &[(2, "cond-expr", vec![6, 4])],
    );
}

#[test]
fn real_v8_ternary_comment_after_operator() {
    // V8 removed the `:` range. A comment sits between `?` and the consequent.
    assert_real_v8_branches(
        "function f(x) {\n  return x ? /* a */ 'yes' : /* b */ 'no';\n}\nfor (let i=0;i<10;i++) f(0);",
        &[
            function("", vec![range(0, 89, 1), range(84, 89, 10)], true),
            function("f", vec![range(0, 60, 10), range(27, 42, 0)], true),
        ],
        &[(2, "cond-expr", vec![0, 10])],
    );
}

#[test]
fn real_v8_short_ternary_consequent_without_range() {
    // V8 removed the `?` range. The `: 0` range is within the tolerance of the short consequent, but it is the alternate range.
    assert_real_v8_branches(
        "const f = (v) => v ? v : 0;\nfor (let i=0;i<10;i++) f(1);",
        &[
            function("", vec![range(0, 56, 1), range(51, 56, 10)], true),
            function("f", vec![range(10, 26, 10), range(23, 26, 0)], true),
        ],
        &[(1, "cond-expr", vec![10, 0])],
    );
}

#[test]
fn real_v8_short_ternary_alternate_without_range() {
    // V8 removed the `:` range. The `? 1` range is within the tolerance of the short alternate, but it is the consequent range.
    assert_real_v8_branches(
        "const f = (v) => v ? 1 : 0;\nfor (let i=0;i<10;i++) f(0);",
        &[
            function("", vec![range(0, 56, 1), range(51, 56, 10)], true),
            function("f", vec![range(10, 26, 10), range(19, 22, 0)], true),
        ],
        &[(1, "cond-expr", vec![0, 10])],
    );
}

#[test]
fn real_v8_ternary_arm_with_inner_logical_range() {
    // The `|| c` range inside the consequent is nearer to the arm than the arm range is.
    assert_real_v8_branches(
        "function f(a,b,c){ return a ? (b || c) : (c && b); }\nfor (let i=0;i<10;i++) f(i%2, i%3, i%5);",
        &[
            function("", vec![range(0, 93, 1), range(76, 93, 10)], true),
            function(
                "f",
                vec![
                    range(0, 52, 10),
                    range(28, 38, 5),
                    range(33, 37, 2),
                    range(39, 49, 5),
                    range(44, 48, 4),
                ],
                true,
            ),
        ],
        &[
            (1, "cond-expr", vec![5, 5]),
            (1, "binary-expr", vec![5, 2]),
            (1, "binary-expr", vec![5, 4]),
        ],
    );
}

#[test]
fn real_v8_if_after_early_return() {
    // After `return`, V8 splits the range around the second `if` at its body.
    assert_real_v8_branches(
        "function f(a,b){\n  if (a) return 0;\n  if (b) {\n    return 1;\n  }\n  return 2;\n}\nfor (let i=0;i<10;i++) f(i%2, i%3);",
        &[
            function("", vec![range(0, 114, 1), range(102, 114, 10)], true),
            function(
                "f",
                vec![range(0, 78, 10), range(26, 45, 5), range(45, 64, 3), range(64, 77, 2)],
                true,
            ),
        ],
        &[(2, "if", vec![5, 5]), (3, "if", vec![3, 2])],
    );
}

#[test]
fn real_v8_if_after_continue() {
    // After `continue`, V8 splits the loop body range at the second `if` body.
    assert_real_v8_branches(
        "function f(n){\n  let s=0;\n  for (let i=0;i<n;i++) {\n    if (i%3===0) continue;\n    if (i>100) { s++ }\n  }\n  return s;\n}\nf(10);",
        &[
            function("", vec![range(0, 126, 1)], true),
            function(
                "f",
                vec![
                    range(0, 119, 1),
                    range(50, 105, 10),
                    range(69, 78, 4),
                    range(78, 94, 6),
                    range(94, 101, 0),
                ],
                true,
            ),
        ],
        &[(4, "if", vec![4, 6]), (5, "if", vec![0, 6])],
    );
}

#[test]
fn real_v8_ternary_after_early_return() {
    // After `return`, the range that is active at the ternary is the continuation range, not the function range.
    assert_real_v8_branches(
        "function f(a,b){\n  if (!a) return 0;\n  const r = b ? 'x' : 'y';\n  return r;\n}\nfor (let i=0;i<10;i++) f(i%2, 1);",
        &[
            function("", vec![range(0, 111, 1), range(101, 111, 10)], true),
            function("f", vec![range(0, 77, 10), range(27, 56, 5), range(57, 62, 0)], true),
        ],
        &[(2, "if", vec![5, 5]), (3, "cond-expr", vec![5, 0])],
    );
}

#[test]
fn real_v8_logical_assignment() {
    // V8 has no range for the right side of a logical assignment, so that arm stays 0. Istanbul does not instrument a logical assignment.
    assert_real_v8_branches(
        "function f(a){ let x=a; x ||= 5; x &&= x+1; return x }\nfor (let i=0;i<10;i++) f(i%2);",
        &[
            function("", vec![range(0, 85, 1), range(78, 85, 10)], true),
            function("f", vec![range(0, 54, 10)], true),
        ],
        &[(1, "binary-expr", vec![10, 0]), (1, "binary-expr", vec![10, 0])],
    );
}

#[test]
fn corrupted_inline_source_map_silently_skipped() {
    // A malformed base64 payload should be silently ignored rather than
    // erroring or panicking. The function returns FileCoverage with no
    // inputSourceMap attached, and the rest of the conversion proceeds as
    // if no inline map were present. Pinning this prevents future drift
    // toward fail-loud behavior that would break otherwise-valid coverage
    // on a single bad map line.
    let source =
        "const x = 1;\n//# sourceMappingURL=data:application/json;base64,!!!not-base64!!!\n";
    let end = byte_len(source);
    let functions = vec![function("", vec![range(0, end, 1)], false)];

    let fc = v8_to_istanbul(source, "bad-map.js", &functions, 0).unwrap();
    assert!(fc.input_source_map.is_none(), "corrupt inline map must not produce inputSourceMap");
    assert!(fc.s.values().any(|&c| c == 1), "rest of coverage still resolves");
}

#[test]
fn extracts_inline_urlsafe_base64_source_map() {
    // The decoder must accept the URL-safe base64 alphabet (RFC 4648 §5, `-`
    // and `_` in place of `+` and `/`) as well as the standard one, or an
    // inline map written that way is silently dropped.
    let original_map_json = r#"{"version":3,"sources":["src/app.ts"],"sourcesContent":["const x: number = 1;"],"mappings":"AAAA","names":[]}"#;
    let base64 = encode_base64(original_map_json.as_bytes())
        .cow_replace('+', "-")
        .cow_replace('/', "_")
        .into_owned();
    let source =
        format!("const x = 1;\n//# sourceMappingURL=data:application/json;base64,{base64}\n");
    let end = byte_len(&source);
    let functions = vec![function("", vec![range(0, end, 1)], false)];

    let fc = v8_to_istanbul(&source, "app.js", &functions, 0).unwrap();
    let attached = fc.input_source_map.expect("URL-safe inline map should attach");
    assert_eq!(attached["sources"][0], "src/app.ts");
}

#[test]
fn extracts_inline_base64_source_map() {
    // Vite, esbuild, swc, tsc all emit `//# sourceMappingURL=data:...;base64,...`
    // trailers. v8_to_istanbul must decode and attach that map as
    // inputSourceMap so a downstream remap_coverage chains cleanly.
    let original_map_json = r#"{"version":3,"sources":["src/app.ts"],"sourcesContent":["const x: number = 1;"],"mappings":"AAAA","names":[]}"#;
    let base64 = encode_base64(original_map_json.as_bytes());
    let source =
        format!("const x = 1;\n//# sourceMappingURL=data:application/json;base64,{base64}\n");
    let end = byte_len(&source);
    let functions = vec![function("", vec![range(0, end, 1)], false)];

    let fc = v8_to_istanbul(&source, "app.js", &functions, 0).unwrap();
    let attached = fc.input_source_map.expect("inline map should attach");
    assert_eq!(attached["sources"][0], "src/app.ts");
    assert_eq!(attached["version"], 3);
}

/// Standard-alphabet base64, so the test needs no encoder dependency.
fn encode_base64(bytes: &[u8]) -> String {
    const ALPHA: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = chunk.get(1).copied().unwrap_or(0);
        let b2 = chunk.get(2).copied().unwrap_or(0);
        out.push(ALPHA[(b0 >> 2) as usize] as char);
        out.push(ALPHA[(((b0 & 0x03) << 4) | (b1 >> 4)) as usize] as char);
        if chunk.len() > 1 {
            out.push(ALPHA[(((b1 & 0x0f) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(ALPHA[(b2 & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

#[test]
fn handles_non_ascii_source_columns() {
    // Istanbul and V8 report UTF-16 positions; srcmap's generated columns use
    // UTF-16 while Oxc spans use bytes. A
    // statement that follows a non-ASCII character must still land inside
    // the V8 range that contains it. `π` is 2 bytes UTF-8 / 1 UTF-16 unit,
    // so the byte position of `const y` shifts by an extra byte per π.
    let source = "const π = 1;\nconst y = π + 1;\n";
    let end = utf16_len(source);
    let functions = vec![function("", vec![range(0, end, 7)], false)];

    let fc = v8_to_istanbul(source, "greek.js", &functions, 0).unwrap();
    let s_values: Vec<u32> = fc.s.values().copied().collect();
    assert!(
        s_values.iter().all(|&c| c == 7),
        "non-ASCII statements must still resolve into the V8 range, got: {s_values:?}"
    );
}

#[test]
fn normalizes_real_inspector_utf16_offsets_before_matching() {
    let source = r"const prefix = '😀😀😀😀😀😀é';
function unicodeBranch(value) {
  if (value) {
    return '✓';
  } else {
    return 'miss';
  }
}
unicodeBranch(true);
unicodeBranch(true);
//# sourceURL=oxc-coverage-instrument://unicode-offsets.js
";
    // Captured from Node's Profiler.takePreciseCoverage. These offsets are
    // UTF-16 code units, including the astral and BMP characters above.
    let functions = vec![
        function("", vec![range(0, 232, 1)], true),
        function("unicodeBranch", vec![range(32, 130, 2), range(98, 128, 0)], true),
    ];

    let fc = v8_to_istanbul(source, "inspector-unicode.js", &functions, 0).unwrap();
    let function_count = fc
        .fn_map
        .iter()
        .find(|(_, entry)| entry.name == "unicodeBranch")
        .and_then(|(id, _)| fc.f.get(id))
        .copied();
    let branch_counts = fc
        .branch_map
        .iter()
        .find(|(_, entry)| entry.branch_type == "if")
        .and_then(|(id, entry)| fc.b.get(id).map(|counts| (entry, counts)))
        .expect("if branch must appear in branchMap");

    assert_eq!(function_count, Some(2));
    assert_eq!(branch_counts.1, &[2, 0]);
    assert_eq!(branch_counts.1.len(), branch_counts.0.locations.len());
}

#[test]
fn preserves_utf16_offsets_across_crlf_and_mixed_newlines() {
    let source = "const astral = '😀';\r\nconst bmp = 'é';\nconst value = astral + bmp;\r\n";
    let source_end = utf16_len(source);
    let expression_start = utf16_len(&source[..source.rfind("astral + bmp").unwrap()]);
    let expression_end = expression_start + utf16_len("astral + bmp");
    let functions = vec![function(
        "",
        vec![range(0, source_end, 1), range(expression_start, expression_end, 9)],
        true,
    )];

    let fc = v8_to_istanbul(source, "mixed-newlines.js", &functions, 0).unwrap();
    let line_three_id = fc
        .statement_map
        .iter()
        .find(|(_, location)| location.start.line == 3)
        .map(|(id, _)| id)
        .expect("third-line expression must be tracked");

    assert_eq!(fc.s.get(line_three_id), Some(&9));
}

#[test]
fn unicode_prior_line_preserves_nested_function_arm_inheritance() {
    let source = "const marker = '😀é';\r\nif (true) { function g() {} }\r\ng(); g();\n";
    let source_end = utf16_len(source);
    let function_start = utf16_len(&source[..source.find("function g").unwrap()]);
    let function_end = function_start + utf16_len("function g() {}");
    let functions = [
        function("", vec![range(0, source_end, 1)], true),
        function("g", vec![range(function_start, function_end, 2)], true),
    ];

    assert_if_counts(source, &functions, &[1, 0]);
}

#[test]
fn unicode_offsets_do_not_change_attached_source_map_locations() {
    let map_json = r#"{"version":3,"sources":["src/app.ts"],"mappings":"AAAA","names":[]}"#;
    let base64 = encode_base64(map_json.as_bytes());
    let source = format!(
        "const marker = '😀é';\r\nconst value = 1;\n//# sourceMappingURL=data:application/json;base64,{base64}\n"
    );
    let end = utf16_len(&source);
    let functions = vec![function("", vec![range(0, end, 3)], false)];

    let fc = v8_to_istanbul(&source, "unicode-map.js", &functions, 0).unwrap();
    let line_two = fc
        .statement_map
        .iter()
        .find(|(_, location)| location.start.line == 2)
        .expect("second-line statement must remain in generated-source coordinates");

    assert_eq!(fc.s.get(line_two.0), Some(&3));
    assert_eq!(line_two.1.start.line, 2);
    assert_eq!(fc.input_source_map.as_ref().unwrap()["sources"][0], "src/app.ts");
}

#[test]
fn function_counts_track_call_counts() {
    let source = "function add(a, b) { return a + b; }\nadd(1, 2);\n";
    let end = byte_len(source);
    // V8 says the function ran 3 times. The module body ran 1 time. Two
    // distinct V8 entries.
    let functions = vec![
        function("", vec![range(0, end, 1)], false),
        function(
            "add",
            vec![range(byte_offset_of(source, "function"), byte_offset_of(source, "}") + 1, 3)],
            false,
        ),
    ];

    let fc = v8_to_istanbul(source, "calls.js", &functions, 0).unwrap();

    let add_id = fc
        .fn_map
        .iter()
        .find(|(_, fe)| fe.name == "add")
        .map(|(id, _)| id.clone())
        .expect("fnMap must include `add`");
    assert_eq!(
        fc.f.get(&add_id).copied().unwrap_or(0),
        3,
        "function count should match V8 call count"
    );
}

#[test]
fn if_arm_zero_reports_then_block_count_with_mixed_arms() {
    // arm[0] reports how many times the predicate was truthy, resolved from
    // the consequent-body byte span against V8's then-block range. Five calls,
    // three truthy and two falsy, therefore give [3, 2]. V8 emits ranges
    // outermost-first, so a first-tolerance-match walk would take the
    // enclosing function range (count 5) for both arms.
    let source = "function f(x) {\n  if (x) {\n    a();\n  } else {\n    b();\n  }\n}\n";
    let module_end = byte_len(source);
    let then_start = byte_offset_of(source, "if (x) {") + 7;
    let then_end = byte_offset_of(source, "} else") + 1;
    let else_start = byte_offset_of(source, "else {") + 5;
    let else_end = last_byte_offset_of(source, "\n  }") + 4;

    let functions = vec![function(
        "f",
        vec![
            range(0, module_end, 5),
            range(then_start, then_end, 3),
            range(else_start, else_end, 2),
        ],
        true,
    )];

    let fc = v8_to_istanbul(source, "ifelse-counts.js", &functions, 0).unwrap();
    assert_eq!(
        arm_counts(&fc, "if"),
        vec![3, 2],
        "tightest match wins: arm[0] = then-block count (3), arm[1] = else-block count (2), \
         not the enclosing function's 5"
    );
}

#[test]
fn if_arm_zero_resolves_when_alternate_is_missing() {
    // No `else` clause; istanbul still records two arms with locations[1]
    // pointing at the synthesized empty alternate. arm[0] must still resolve
    // through the collected then-block span. arm[1] gets the parent count minus
    // the then count, 4 - 4 = 0.
    let source = "function f(x) {\n  if (x) {\n    a();\n  }\n}\n";
    let module_end = byte_len(source);
    let then_start = byte_offset_of(source, "if (x) {") + 7;
    let then_end = last_byte_offset_of(source, "\n  }") + 4;

    let functions =
        vec![function("f", vec![range(0, module_end, 4), range(then_start, then_end, 4)], true)];

    let fc = v8_to_istanbul(source, "if-only.js", &functions, 0).unwrap();
    let arm_counts = arm_counts(&fc, "if");
    assert_eq!(arm_counts.len(), 2, "if without else still has two arms");
    assert_eq!(arm_counts[0], 4, "arm[0] reflects then-block count");
    assert_eq!(arm_counts[1], 0, "every call took the then arm");
}

#[test]
fn real_v8_if_inherits_then_count_when_only_else_has_a_child_range() {
    assert_real_v8_if_counts(
        "f(true); f(true);",
        vec![range(0, REAL_V8_FUNCTION_END, 2), range(REAL_V8_ELSE_START, REAL_V8_ELSE_END, 0)],
        &[2, 0],
    );
}

#[test]
fn real_v8_if_inherits_else_count_when_only_then_has_a_child_range() {
    assert_real_v8_if_counts(
        "f(false); f(false);",
        vec![range(0, REAL_V8_FUNCTION_END, 2), range(REAL_V8_THEN_START, REAL_V8_THEN_END, 0)],
        &[0, 2],
    );
}

#[test]
fn real_v8_if_matches_explicit_then_and_else_ranges() {
    assert_real_v8_if_counts(
        "f(true); f(false); f(true);",
        vec![
            range(0, REAL_V8_FUNCTION_END, 3),
            range(REAL_V8_THEN_START, REAL_V8_THEN_END, 2),
            range(REAL_V8_ELSE_START, REAL_V8_ELSE_END, 1),
        ],
        &[2, 1],
    );
}

#[test]
fn function_only_v8_coverage_does_not_infer_if_arm_counts() {
    let calls = "f(true); f(true);";
    let source = format!("{REAL_V8_IF_FUNCTION_SOURCE} {calls}\n");
    assert_real_v8_if_functions(
        calls,
        &[
            function("", vec![range(0, byte_len(&source), 1)], true),
            function("f", vec![range(0, REAL_V8_FUNCTION_END, 2)], false),
        ],
        &[0, 0],
    );
}

#[test]
fn real_v8_if_ignores_uncalled_nested_function_outer_range() {
    let source = "if (true) { function g() {} }\n";
    let functions = [
        function("", vec![range(0, byte_len(source), 1)], true),
        function("g", vec![range(12, 27, 0)], false),
    ];
    assert_if_counts(source, &functions, &[1, 0]);
}

#[test]
fn real_v8_if_ignores_called_nested_function_outer_range() {
    let source = "if (true) { function g() {} }\ng(); g();\n";
    let functions = [
        function("", vec![range(0, byte_len(source), 1)], true),
        function("g", vec![range(12, 27, 2)], true),
    ];
    assert_if_counts(source, &functions, &[1, 0]);
}

#[test]
fn real_v8_if_uses_strict_parent_for_annex_b_function_arm() {
    let source = "if (true) function g() {}\ng(); g();\n";
    let functions = [
        function("", vec![range(0, byte_len(source), 1)], true),
        function("g", vec![range(10, 25, 2)], true),
    ];
    assert_if_counts(source, &functions, &[1, 0]);
}

#[test]
fn external_source_mapping_url_invokes_loader() {
    // When the source carries a non-data sourceMappingURL trailer, the loader
    // is consulted with the URL string and the returned JSON is parsed and
    // attached as inputSourceMap on the result.
    let map_json =
        r#"{"version":3,"sources":["src/app.ts"],"mappings":"AAAA","names":[]}"#.to_string();
    let source = "const x = 1;\n//# sourceMappingURL=app.js.map\n";
    let end = byte_len(source);
    let functions = vec![function("", vec![range(0, end, 1)], false)];

    let seen_urls = std::cell::RefCell::new(Vec::<String>::new());
    let fc = v8_to_istanbul_with_loader(source, "app.js", &functions, 0, |url| {
        seen_urls.borrow_mut().push(url.to_string());
        if url == "app.js.map" { Some(map_json.clone()) } else { None }
    })
    .unwrap();

    assert_eq!(
        seen_urls.into_inner(),
        vec!["app.js.map".to_string()],
        "loader sees the trailer URL"
    );
    let attached = fc.input_source_map.expect("external map should be attached");
    assert_eq!(attached["sources"][0], "src/app.ts");
}

#[test]
fn source_mapping_url_inside_template_does_not_invoke_loader() {
    let source = "const marker = `\n//# sourceMappingURL=ghost.map\n`;\n";
    let functions = vec![function("", vec![range(0, byte_len(source), 1)], false)];
    let seen = std::cell::RefCell::new(Vec::<String>::new());

    let coverage = v8_to_istanbul_with_loader(source, "app.js", &functions, 0, |url| {
        seen.borrow_mut().push(url.to_string());
        Some(r#"{"version":3,"sources":["ghost.ts"],"mappings":"AAAA","names":[]}"#.to_string())
    })
    .unwrap();

    assert!(seen.into_inner().is_empty(), "false trailer must not invoke the loader");
    assert!(coverage.input_source_map.is_none(), "false trailer must not attach a map");
}

#[test]
fn external_source_mapping_url_loader_returning_none_leaves_map_unset() {
    // A loader returning `None` (disk read failed, no map next to the file)
    // leaves `inputSourceMap` unset and does not disturb the rest of the
    // conversion.
    let source = "const x = 1;\n//# sourceMappingURL=missing.map\n";
    let end = byte_len(source);
    let functions = vec![function("", vec![range(0, end, 1)], false)];

    let fc = v8_to_istanbul_with_loader(source, "missing.js", &functions, 0, |_| None).unwrap();
    assert!(fc.input_source_map.is_none(), "no map attached when loader returns None");
    assert!(fc.s.values().any(|&c| c == 1), "rest of coverage still resolves");
}

#[test]
fn inline_source_map_takes_precedence_over_external_loader() {
    // When both forms could match (inline data URL is rfind-found before any
    // other comment), the inline form wins and the loader is never called.
    // A simple way to surface that: a panicking loader that would fail the
    // test if invoked.
    let original_map_json =
        r#"{"version":3,"sources":["src/app.ts"],"mappings":"AAAA","names":[]}"#;
    let base64 = encode_base64(original_map_json.as_bytes());
    let source =
        format!("const x = 1;\n//# sourceMappingURL=data:application/json;base64,{base64}\n");
    let end = byte_len(&source);
    let functions = vec![function("", vec![range(0, end, 1)], false)];

    let fc = v8_to_istanbul_with_loader(&source, "app.js", &functions, 0, |_| {
        panic!("loader must not be called when an inline map is present")
    })
    .unwrap();
    let attached = fc.input_source_map.expect("inline map should be attached");
    assert_eq!(attached["sources"][0], "src/app.ts");
}

#[test]
fn parse_error_propagates_as_v8_to_istanbul_error() {
    // Source that the Oxc parser refuses (stray `}` at the top level). Without
    // a Parse-error path, the converter would silently produce an empty
    // FileCoverage and downstream reporters would show 0 lines instead of
    // surfacing the underlying syntax error.
    let invalid = "function () { }}}\nconst x = ;\n";
    let functions: Vec<V8FunctionCoverage> = vec![];

    let err = v8_to_istanbul(invalid, "broken.js", &functions, 0)
        .expect_err("invalid source must return a Parse error");
    let V8ToIstanbulError::Parse(message) = err;
    assert!(
        !message.is_empty(),
        "parse error must carry the underlying diagnostic, got empty string",
    );
}

/// The reproduction from fallow-rs/oxc-coverage#303: a private and a
/// public method with the same body, both run through every return.
const PRIVATE_TWIN_SOURCE: &str = "import assert from 'node:assert/strict'
class Twin {
  #privateMethod(value){
    if(value===0)return 10
    if(value===1)return 20
    if(value===2)return 30
    if(value===3)return 40
    if(value===4)return 50
    if(value===5)return 60
    return 70
  }
  publicMethod(value){
    if(value===0)return 10
    if(value===1)return 20
    if(value===2)return 30
    if(value===3)return 40
    if(value===4)return 50
    if(value===5)return 60
    return 70
  }
  callPrivate(value){return this.#privateMethod(value)}
}
const twin=new Twin()
for(let value=0;value<=6;value++){
  assert.equal(twin.callPrivate(value),(value+1)*10)
  assert.equal(twin.publicMethod(value),(value+1)*10)
}
";

/// Ranges of one method, as Node 24 reports them for [`PRIVATE_TWIN_SOURCE`].
/// The outer range starts at the method start `method`. The block ranges are
/// relative to the opening parenthesis `params`. Both methods have the same
/// block ranges.
fn private_twin_method_ranges(method: u32, params: u32) -> Vec<V8CoverageRange> {
    const BLOCKS: [(u32, u32, u32); 11] = [
        (26, 35, 1),
        (35, 53, 6),
        (53, 62, 1),
        (62, 80, 5),
        (80, 89, 1),
        (89, 107, 4),
        (107, 116, 1),
        (116, 134, 3),
        (134, 143, 1),
        (143, 161, 2),
        (161, 187, 1),
    ];
    let outer = range(method, params + 188, 7);
    std::iter::once(outer)
        .chain(BLOCKS.iter().map(|&(start, end, count)| range(params + start, params + end, count)))
        .collect()
}

/// The V8 functions that `NODE_V8_COVERAGE` reports for [`PRIVATE_TWIN_SOURCE`]
/// on Node 24.
fn private_twin_functions() -> Vec<V8FunctionCoverage> {
    let end = byte_len(PRIVATE_TWIN_SOURCE);
    vec![
        function("", vec![range(0, end, 1), range(574, end - 1, 7)], true),
        function("#privateMethod", private_twin_method_ranges(55, 69), true),
        function("publicMethod", private_twin_method_ranges(260, 272), true),
        function("callPrivate", vec![range(463, 516, 7)], true),
    ]
}

fn fn_entry_by_name<'a>(
    fc: &'a oxc_coverage_instrument::FileCoverage,
    name: &str,
) -> (&'a String, &'a oxc_coverage_instrument::FnEntry) {
    let mut matches = fc.fn_map.iter().filter(|(_, entry)| entry.name == name);
    let found = matches.next().unwrap_or_else(|| panic!("fnMap must include `{name}`"));
    assert!(matches.next().is_none(), "fnMap must include `{name}` only once");
    found
}

/// `(line, column)` pairs of a location, for compact span assertions.
fn span_of(loc: &oxc_coverage_instrument::Location) -> ((u32, u32), (u32, u32)) {
    ((loc.start.line, loc.start.column), (loc.end.line, loc.end.column))
}

#[test]
fn private_twin_v8_ranges_match_the_recorded_node_output() {
    let functions = private_twin_functions();
    let private = &functions[1].ranges;
    let public = &functions[2].ranges;
    assert_eq!((private[0].start_offset, private[0].end_offset), (55, 257));
    assert_eq!((public[0].start_offset, public[0].end_offset), (260, 460));
    assert_eq!(private.len(), 12);
    assert_eq!((private[11].start_offset, private[11].end_offset), (230, 256));
    assert_eq!((public[11].start_offset, public[11].end_offset), (433, 459));
    assert!(private.iter().chain(public).all(|range| range.count > 0));
    assert_eq!(byte_offset_of(PRIVATE_TWIN_SOURCE, "#privateMethod("), 55);
    assert_eq!(byte_offset_of(PRIVATE_TWIN_SOURCE, "publicMethod("), 260);
}

#[test]
fn v8_conversion_records_a_private_method_like_its_public_twin() {
    let fc =
        v8_to_istanbul(PRIVATE_TWIN_SOURCE, "fixture.mjs", &private_twin_functions(), 0).unwrap();

    let (private_id, private) = fn_entry_by_name(&fc, "#privateMethod");
    let (public_id, public) = fn_entry_by_name(&fc, "publicMethod");

    assert_eq!(private.line, 3);
    assert_eq!(span_of(&private.decl), ((3, 2), (3, 16)), "decl is the `#privateMethod` key");
    assert_eq!(span_of(&private.loc), ((3, 23), (11, 3)), "loc is the method body");
    assert_eq!(public.line, 12);
    assert_eq!(span_of(&public.decl), ((12, 2), (12, 14)), "decl is the `publicMethod` key");
    assert_eq!(span_of(&public.loc), ((12, 21), (20, 3)), "loc is the method body");

    assert_eq!(fc.f[private_id], 7, "the private method ran 7 times");
    assert_eq!(fc.f[public_id], 7, "the public method ran 7 times");
    assert_eq!(fn_entry_by_name(&fc, "callPrivate").1.line, 21);
}

#[test]
fn v8_conversion_keeps_private_method_statements_and_branches() {
    let fc =
        v8_to_istanbul(PRIVATE_TWIN_SOURCE, "fixture.mjs", &private_twin_functions(), 0).unwrap();

    // Sorted by position, because the map keys sort as strings ("10" < "2").
    let counts_on_lines = |first: u32, last: u32| -> Vec<u32> {
        let mut found: Vec<_> = fc
            .statement_map
            .iter()
            .filter(|(_, loc)| (first..=last).contains(&loc.start.line))
            .map(|(id, loc)| ((loc.start.line, loc.start.column), fc.s[id]))
            .collect();
        found.sort_unstable();
        found.into_iter().map(|(_, count)| count).collect()
    };
    let private_statements = counts_on_lines(4, 10);
    let public_statements = counts_on_lines(13, 19);
    assert_eq!(private_statements.len(), 13);
    assert_eq!(private_statements, public_statements, "both bodies have the same counts");
    assert!(private_statements.iter().all(|count| *count > 0), "every path ran");

    let if_counts_on_lines = |first: u32, last: u32| -> Vec<Vec<u32>> {
        let mut found: Vec<_> = fc
            .branch_map
            .iter()
            .filter(|(_, branch)| (first..=last).contains(&branch.loc.start.line))
            .map(|(id, branch)| (branch.loc.start.line, fc.b[id].clone()))
            .collect();
        found.sort_unstable();
        found.into_iter().map(|(_, counts)| counts).collect()
    };
    assert_eq!(if_counts_on_lines(4, 10), if_counts_on_lines(13, 19));
    assert_eq!(if_counts_on_lines(4, 10).len(), 6);
}

#[test]
fn v8_conversion_records_private_accessors_and_modifiers() {
    let source = "class C {\n  get #a() { return 1; }\n  set #a(v) {}\n  static async *#g() {}\n  \"#q\"() {}\n  #q() {}\n}\n";
    let end = byte_len(source);
    let fc =
        v8_to_istanbul(source, "modifiers.js", &[function("", vec![range(0, end, 1)], true)], 0)
            .unwrap();

    let mut entries: Vec<_> =
        fc.fn_map.values().map(|entry| (entry.name.as_str(), span_of(&entry.decl))).collect();
    entries.sort_by_key(|(_, decl)| *decl);
    assert_eq!(
        entries,
        [
            ("get #a", ((2, 6), (2, 8))),
            ("set #a", ((3, 6), (3, 8))),
            ("#g", ((4, 16), (4, 18))),
            ("#q", ((5, 2), (5, 6))),
            ("#q", ((6, 2), (6, 4))),
        ],
        "a quoted public `\"#q\"` and a private `#q` keep distinct decl spans"
    );
}

const STATIC_FIELDS_SOURCE: &str = "let flag = 0;
class Box {
  kind = flag ? 'a' : 'b';
  static base = flag ? 1 : 2;
  static {
    if (flag) flag = 0;
  }
}
for (const n of [0, 1, 2, 3]) {
  flag = n;
  new Box();
}
";

/// Statement counts keyed by `line:column-line:column`, sorted by position.
fn statement_counts(fc: &oxc_coverage_instrument::FileCoverage) -> Vec<(String, u32)> {
    let mut found: Vec<_> = fc
        .statement_map
        .iter()
        .map(|(id, loc)| {
            let ((sl, sc), (el, ec)) = span_of(loc);
            ((sl, sc), format!("{sl}:{sc}-{el}:{ec}"), fc.s[id])
        })
        .collect();
    found.sort_unstable();
    found.into_iter().map(|(_, key, count)| (key, count)).collect()
}

/// Branch counts keyed by `type line:column`, sorted by position.
fn branch_counts(fc: &oxc_coverage_instrument::FileCoverage) -> Vec<(String, Vec<u32>)> {
    let mut found: Vec<_> = fc
        .branch_map
        .iter()
        .map(|(id, branch)| {
            let ((line, column), _) = span_of(&branch.loc);
            ((line, column), format!("{} {line}:{column}", branch.branch_type), fc.b[id].clone())
        })
        .collect();
    found.sort_unstable();
    found.into_iter().map(|(_, key, counts)| (key, counts)).collect()
}

/// The statement and branch counts of `istanbul-lib-instrument` for
/// [`STATIC_FIELDS_SOURCE`].
fn assert_static_fields_match_istanbul(fc: &oxc_coverage_instrument::FileCoverage) {
    let statements = statement_counts(fc);
    let expected_statements: Vec<(String, u32)> = [
        ("1:11-1:12", 1),
        ("3:9-3:25", 4),
        ("4:16-4:28", 1),
        ("6:4-6:23", 1),
        ("6:14-6:23", 0),
        ("9:0-12:1", 1),
        ("10:2-10:11", 4),
        ("11:2-11:12", 4),
    ]
    .into_iter()
    .map(|(key, count)| (key.to_string(), count))
    .collect();
    assert_eq!(statements, expected_statements);
    assert_eq!(
        branch_counts(fc),
        [
            ("cond-expr 3:9".to_string(), vec![3, 1]),
            ("cond-expr 4:16".to_string(), vec![0, 1]),
            ("if 6:4".to_string(), vec![0, 1]),
        ]
    );
}

#[test]
fn real_v8_node22_static_members_take_the_static_initializer_count() {
    // Node 22 gives `<instance_members_initializer>` and
    // `<static_initializer>` the same outer range: the class body.
    let functions = [
        function("", vec![range(0, 205, 1), range(154, 182, 4)], true),
        function(
            "<instance_members_initializer>",
            vec![range(24, 123, 4), range(40, 45, 3), range(46, 51, 1)],
            true,
        ),
        function(
            "<static_initializer>",
            vec![range(24, 123, 1), range(74, 77, 0), range(108, 117, 0)],
            true,
        ),
    ];
    let fc = v8_to_istanbul(STATIC_FIELDS_SOURCE, "static.js", &functions, 0).unwrap();
    assert_static_fields_match_istanbul(&fc);
}

#[test]
fn real_v8_node22_static_initializer_listed_first_keeps_instance_counts() {
    // The record order must not decide the tie.
    let functions = [
        function("", vec![range(0, 205, 1), range(154, 182, 4)], true),
        function(
            "<static_initializer>",
            vec![range(24, 123, 1), range(74, 77, 0), range(108, 117, 0)],
            true,
        ),
        function(
            "<instance_members_initializer>",
            vec![range(24, 123, 4), range(40, 45, 3), range(46, 51, 1)],
            true,
        ),
    ];
    let fc = v8_to_istanbul(STATIC_FIELDS_SOURCE, "static.js", &functions, 0).unwrap();
    assert_static_fields_match_istanbul(&fc);
}

#[test]
fn real_v8_node24_static_members_take_the_static_initializer_count() {
    // Node 24 gives each initializer the span of its own members.
    let functions = [
        function("", vec![range(0, 205, 1), range(154, 182, 4)], true),
        function(
            "<instance_members_initializer>",
            vec![range(28, 51, 4), range(40, 45, 3), range(46, 51, 1)],
            true,
        ),
        function(
            "<static_initializer>",
            vec![range(55, 121, 1), range(74, 77, 0), range(108, 117, 0)],
            true,
        ),
    ];
    let fc = v8_to_istanbul(STATIC_FIELDS_SOURCE, "static.js", &functions, 0).unwrap();
    assert_static_fields_match_istanbul(&fc);
}

#[test]
fn real_v8_node22_class_in_a_static_field_keeps_its_own_instance_counts() {
    // The inner class sits in a static field of the outer class. Its
    // instance field must not take the inner static initializer count.
    let source = "let flag = 0;
class Outer {
  static Inner = class {
    kind = flag ? 'a' : 'b';
    static base = flag ? 1 : 2;
  };
}
for (const n of [0, 1, 2, 3]) {
  flag = n;
  new Outer.Inner();
}
";
    let functions = [
        function("", vec![range(0, 210, 1), range(151, 187, 4)], true),
        function("<static_initializer>", vec![range(26, 120, 1)], true),
        function(
            "<instance_members_initializer>",
            vec![range(51, 117, 4), range(69, 74, 3), range(75, 80, 1)],
            true,
        ),
        function("<static_initializer>", vec![range(51, 117, 1), range(105, 108, 0)], true),
    ];
    let fc = v8_to_istanbul(source, "nested.js", &functions, 0).unwrap();
    let statements = statement_counts(&fc);
    let expected: Vec<(String, u32)> = [
        ("1:11-1:12", 1),
        ("3:17-6:3", 1),
        ("4:11-4:27", 4),
        ("5:18-5:30", 1),
        ("8:0-11:1", 1),
        ("9:2-9:11", 4),
        ("10:2-10:20", 4),
    ]
    .into_iter()
    .map(|(key, count)| (key.to_string(), count))
    .collect();
    assert_eq!(statements, expected);
    assert_eq!(
        branch_counts(&fc),
        [("cond-expr 4:11".to_string(), vec![3, 1]), ("cond-expr 5:18".to_string(), vec![0, 1])]
    );
}

#[test]
fn real_v8_node22_static_members_with_a_wrapper_and_non_ascii_text() {
    // A non-ASCII line moves the byte offsets away from the UTF-16 offsets,
    // and the wrapper moves every V8 offset by the same base.
    let prefix = "// \u{e9}t\u{e9} \u{1f600}\n";
    let source = format!("{prefix}{STATIC_FIELDS_SOURCE}");
    let wrapper = 10;
    let shift = utf16_len(prefix) + wrapper;
    let at = |start: u32, end: u32, count: u32| range(start + shift, end + shift, count);
    let functions = [
        function("", vec![range(0, 205 + shift, 1), at(154, 182, 4)], true),
        function(
            "<instance_members_initializer>",
            vec![at(24, 123, 4), at(40, 45, 3), at(46, 51, 1)],
            true,
        ),
        function(
            "<static_initializer>",
            vec![at(24, 123, 1), at(74, 77, 0), at(108, 117, 0)],
            true,
        ),
    ];
    let fc = v8_to_istanbul(&source, "static.js", &functions, wrapper).unwrap();
    let static_value = statement_counts(&fc)
        .into_iter()
        .find(|(key, _)| key == "5:16-5:28")
        .map(|(_, count)| count);
    assert_eq!(static_value, Some(1));
    assert_eq!(
        branch_counts(&fc),
        [
            ("cond-expr 4:9".to_string(), vec![3, 1]),
            ("cond-expr 5:16".to_string(), vec![0, 1]),
            ("if 7:4".to_string(), vec![0, 1]),
        ]
    );
}
