//! Tests for concise arrow bodies (`(x) => x * 2`), which carry no block of
//! their own.
//!
//! Coverage needs one: `++cov.f[N]` lives in a block, and the
//! `ExpressionStatement` inside it is what earns the arrow's `statementMap`
//! entry. The block is built by the instrumenter, and the expression it wraps
//! is turned back into a `return` on the way out, so the arrow keeps yielding
//! its value.
//!
//! Two properties are easy to break and invisible in a coverage map read on
//! its own, so both are pinned here: the instrumented arrow still returns what
//! the source said it returns, checked by running the emitted code; and the
//! recorded span is the one the source wrote, including when
//! `strip_typescript` rewrites the body out from under it.

use std::process::Command;

use oxc_coverage_instrument::{InstrumentOptions, InstrumentResult, Location, instrument};

fn instrument_js(source: &str) -> InstrumentResult {
    instrument(source, "concise.js", &InstrumentOptions::default()).expect("instrument")
}

fn instrument_ts(source: &str) -> InstrumentResult {
    let options = InstrumentOptions { strip_typescript: true, ..InstrumentOptions::default() };
    instrument(source, "concise.ts", &options).expect("instrument")
}

/// `((start_line, start_column), (end_line, end_column))`, in the 1-based-line
/// 0-based-column space Istanbul records.
type SpanTuple = ((u32, u32), (u32, u32));

fn position_at(source: &str, offset: usize) -> (u32, u32) {
    let before = &source[..offset];
    let line = u32::try_from(before.matches('\n').count() + 1).unwrap();
    let column = before.rfind('\n').map_or(offset, |newline| offset - newline - 1);
    (line, u32::try_from(column).unwrap())
}

/// The span `needle` occupies in `source`, as the coverage map would record it.
fn span_of(source: &str, needle: &str) -> SpanTuple {
    let start = source
        .find(needle)
        .unwrap_or_else(|| panic!("fixture does not contain {needle:?}: {source}"));
    (position_at(source, start), position_at(source, start + needle.len()))
}

fn span_tuple(loc: &Location) -> SpanTuple {
    ((loc.start.line, loc.start.column), (loc.end.line, loc.end.column))
}

/// Assert that the arrow's `fnMap` `loc` and one `statementMap` entry both
/// span exactly `body` as written in `source`.
fn assert_body_span_recorded(result: &InstrumentResult, source: &str, body: &str) {
    let expected = span_of(source, body);

    let fn_locs: Vec<SpanTuple> =
        result.coverage_map.fn_map.values().map(|entry| span_tuple(&entry.loc)).collect();
    assert!(
        fn_locs.contains(&expected),
        "no fnMap loc spans {body:?} (expected {expected:?}), got {fn_locs:?}"
    );

    let statement_spans: Vec<SpanTuple> =
        result.coverage_map.statement_map.values().map(span_tuple).collect();
    assert!(
        statement_spans.contains(&expected),
        "no statementMap entry spans {body:?} (expected {expected:?}), got {statement_spans:?}"
    );
}

/// Run `code` under node and return the last stdout line, which every runtime
/// fixture here uses to report its results as JSON.
fn run_node(code: &str) -> serde_json::Value {
    let output =
        Command::new("node").arg("--eval").arg(code).output().expect("node must be available");
    assert!(
        output.status.success(),
        "instrumented concise arrows failed to run: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let line = stdout.lines().last().unwrap_or_else(|| panic!("no output from node: {stdout}"));
    serde_json::from_str(line).unwrap_or_else(|error| panic!("bad JSON {line:?}: {error}"))
}

/// Append a reporter that prints the fixture's results together with the
/// function counters, so one run proves both that the arrows still return
/// their values and that each one was counted.
fn with_reporter(result: &InstrumentResult, filename: &str, results: &str) -> String {
    let path = serde_json::to_string(filename).unwrap();
    format!(
        "{code}\nPromise.resolve({results}).then((results) => {{\n  \
         console.log(JSON.stringify({{ results, f: globalThis.__coverage__[{path}].f }}));\n}});",
        code = result.code
    )
}

fn function_counters(report: &serde_json::Value) -> Vec<u64> {
    report["f"]
        .as_object()
        .expect("f counters")
        .values()
        .map(|count| count.as_u64().expect("counter"))
        .collect()
}

// Returned values

/// Nothing else in the suite runs an instrumented concise arrow, so a body
/// left un-returned (the block wrapper stays an `ExpressionStatement`) turns
/// every one of these into `undefined` while the coverage map still looks
/// right.
#[test]
fn instrumented_concise_arrows_return_their_values() {
    let source = "const double = (x) => x * 2;
const object = (x) => ({ v: x });
const curried = (x) => (y) => x + y;
const nested = (x) => [1, 2].map((y) => y + x);
const branchArm = (x) => (x > 0 ? 'pos' : 'neg');
const asyncBody = async (x) => x + 1;
";
    let result =
        instrument(source, "returns.js", &InstrumentOptions::default()).expect("instrument");
    let report = run_node(&with_reporter(
        &result,
        "returns.js",
        "Promise.all([double(2), object(6).v, curried(1)(2), nested(10), \
         branchArm(1), branchArm(-1), asyncBody(9)])",
    ));

    assert_eq!(
        report["results"],
        serde_json::json!([4, 6, 3, [11, 12], "pos", "neg", 10]),
        "instrumented concise arrows must return what the source returns"
    );
    let counters = function_counters(&report);
    assert_eq!(counters.len(), result.coverage_map.fn_map.len());
    assert!(
        counters.iter().all(|count| *count > 0),
        "every concise arrow that ran must have incremented its f counter, got {counters:?}"
    );
}

/// The TypeScript-stripping path builds the block before the strip pass rather
/// than during the traverse, so it forms the `return` from a body it wrapped
/// earlier. Same contract, different code path.
#[test]
fn instrumented_typescript_concise_arrows_return_their_values() {
    let source = "const asChain = (x: number) => x as unknown as string;
const satisfiesBody = (x: number) => ({ a: x }) satisfies { a: number };
const angleAssert = (x: unknown) => <string>x;
const nonNull = (x?: number) => x!;
const instantiate = (f: <T>(v: T) => T) => f<number>;
const identity = <T,>(v: T): T => v;
";
    let options = InstrumentOptions { strip_typescript: true, ..InstrumentOptions::default() };
    let result = instrument(source, "returns.ts", &options).expect("instrument");
    let report = run_node(&with_reporter(
        &result,
        "returns.ts",
        "[asChain(2), satisfiesBody(3).a, angleAssert('s'), nonNull(4), \
         instantiate(identity)(7), identity(8)]",
    ));

    assert_eq!(
        report["results"],
        serde_json::json!([2, 3, "s", 4, 7, 8]),
        "a stripped TypeScript wrapper must not cost the arrow its value"
    );
    assert!(
        function_counters(&report).iter().all(|count| *count > 0),
        "every stripped concise arrow that ran must have incremented its f counter"
    );
}

/// An ignored arrow gets no counters, but it is still rewritten into a block,
/// so it still needs its `return`. Registering "was concise" behind the pragma
/// gate loses that, and unbalances the record for every arrow after it.
#[test]
fn pragma_ignored_concise_arrow_still_returns_its_value() {
    let source = "/* istanbul ignore next */
const ignored = (x) => x * 3;
const counted = (x) => x * 5;
";
    let result =
        instrument(source, "ignored.js", &InstrumentOptions::default()).expect("instrument");
    assert_eq!(
        result.coverage_map.fn_map.len(),
        1,
        "only the counted arrow earns an fnMap entry: {:#?}",
        result.coverage_map.fn_map
    );

    let report = run_node(&with_reporter(&result, "ignored.js", "[ignored(3), counted(3)]"));
    assert_eq!(
        report["results"],
        serde_json::json!([9, 15]),
        "an ignored concise arrow still returns its value"
    );
    assert!(
        function_counters(&report).iter().all(|count| *count > 0),
        "the arrow after an ignored one must keep its own counter"
    );
}

/// `(filename, body, expression producing the results, expected results as
/// JSON)`. Each body is instrumented with an ignore-file pragma prepended.
type IgnoreFileCase = (&'static str, &'static str, &'static str, &'static str);

/// The TypeScript case carries every concise-body shape at once: plain,
/// object-literal, curried, and each TypeScript-only wrapper the strip pass
/// rewrites away. The JavaScript case is here because `strip_typescript` is an
/// option, not an extension: a `.js` bundle instrumented with stripping on
/// takes the same path.
const IGNORE_FILE_CASES: &[IgnoreFileCase] = &[
    (
        "ignored.ts",
        "const double = (x: number): number => x * 2;
const object = (x: number) => ({ v: x });
const curried = (x: number) => (y: number) => x + y;
const asChain = (x: number) => x as unknown as string;
const satisfiesBody = (x: number) => ({ a: x }) satisfies { a: number };
const angleAssert = (x: unknown) => <string>x;
const nonNull = (x?: number) => x!;
const identity = <T,>(v: T): T => v;
const instantiate = (f: <T>(v: T) => T) => f<number>;
",
        "[double(2), object(6).v, curried(1)(2), asChain(3), satisfiesBody(4).a, \
         angleAssert('s'), nonNull(5), instantiate(identity)(7), identity(8)]",
        r#"[4, 6, 3, 3, 4, "s", 5, 7, 8]"#,
    ),
    (
        "ignored.js",
        "const double = (x) => x * 2;
const object = (x) => ({ v: x });
const curried = (x) => (y) => x + y;
",
        "[double(2), object(6).v, curried(1)(2)]",
        "[4, 6, 3]",
    ),
];

/// An ignore-file pragma returns before the coverage traverse, and that
/// traverse is the only thing that turns a wrapped concise body back into a
/// `return`. Under `strip_typescript` the wrapping happens earlier, before the
/// pragma is acted on, so pre-expanding without that gate emits
/// `(x) => { x * 2; }` for every arrow in the file. There is no coverage map to
/// diff against istanbul and no counter to look wrong, only `undefined` where
/// the value used to be, so nothing short of running the emitted code sees it.
#[test]
fn ignore_file_concise_arrows_still_return_their_values_under_strip_typescript() {
    for tool in ["istanbul", "v8", "c8"] {
        for &(filename, body, results, expected) in IGNORE_FILE_CASES {
            let source = format!("/* {tool} ignore file */\n{body}");
            let options =
                InstrumentOptions { strip_typescript: true, ..InstrumentOptions::default() };
            let result = instrument(&source, filename, &options)
                .unwrap_or_else(|error| panic!("{tool} {filename}: {error}"));

            // Pin that this is the ignore-file path, so the case cannot decay
            // into an ordinary instrumented-arrow test if the pragma stops
            // being recognised.
            assert!(
                result.coverage_map.fn_map.is_empty() && !result.code.contains("cov_"),
                "{tool} {filename}: an ignored file earns no counters and no preamble: {}",
                result.code
            );

            let compact: String = result.code.chars().filter(|c| !c.is_whitespace()).collect();
            assert!(
                !compact.contains("=>{"),
                "{tool} {filename}: every body here is concise in the source, so none may be \
                 emitted as a block: {}",
                result.code
            );

            let report = run_node(&format!(
                "{code}\nconsole.log(JSON.stringify({results}));",
                code = result.code
            ));
            assert_eq!(
                report,
                serde_json::from_str::<serde_json::Value>(expected).unwrap(),
                "{tool} {filename}: an ignored file's concise arrows must still return their \
                 values, got {}",
                result.code
            );
        }
    }
}

// Coverage map shapes

#[test]
fn object_literal_body_is_counted_and_spans_the_literal() {
    let source = "const wrap = (x) => ({ v: x });";
    let result = instrument_js(source);

    assert_eq!(result.coverage_map.fn_map.len(), 1);
    assert_body_span_recorded(&result, source, "{ v: x }");
    assert!(result.code.contains(".f[0]"), "object-literal body must carry a function counter");
}

#[test]
fn async_concise_body_is_counted_and_spans_the_expression() {
    let source = "const load = async (x) => x + 1;";
    let result = instrument_js(source);

    assert_eq!(result.coverage_map.fn_map.len(), 1);
    assert_eq!(result.coverage_map.fn_map["0"].name, "load");
    assert_body_span_recorded(&result, source, "x + 1");
}

#[test]
fn curried_concise_arrows_each_get_a_counter_and_a_statement() {
    let source = "const add = (x) => (y) => x + y;";
    let result = instrument_js(source);

    assert_eq!(result.coverage_map.fn_map.len(), 2, "outer and inner arrow");
    // The outer body is the whole inner arrow; the inner body is `x + y`.
    assert_body_span_recorded(&result, source, "(y) => x + y");
    assert_body_span_recorded(&result, source, "x + y");
    assert!(result.code.contains(".f[0]") && result.code.contains(".f[1]"));
}

#[test]
fn concise_arrow_nested_in_a_concise_body_is_counted() {
    let source = "const scale = (x) => [1, 2].map((y) => y * x);";
    let result = instrument_js(source);

    assert_eq!(result.coverage_map.fn_map.len(), 2, "outer arrow and callback");
    assert_body_span_recorded(&result, source, "[1, 2].map((y) => y * x)");
    assert_body_span_recorded(&result, source, "y * x");
}

/// The stripping path has to reach a concise body nested inside another one:
/// the outer body is wrapped first, and the walk must keep descending into the
/// block it just built, or the inner arrow meets the strip pass unwrapped.
#[test]
fn concise_arrow_nested_in_a_stripped_body_keeps_its_span() {
    let source =
        "const scale = (x: number) => [1, 2].map((y: number) => (y * x) as unknown as string);";
    let result = instrument_ts(source);

    assert_eq!(result.coverage_map.fn_map.len(), 2, "outer arrow and callback");
    assert_body_span_recorded(
        &result,
        source,
        "[1, 2].map((y: number) => (y * x) as unknown as string)",
    );
    assert_body_span_recorded(&result, source, "(y * x) as unknown as string");
}

#[test]
fn concise_body_that_is_a_branch_keeps_both_branch_and_function_entries() {
    let source = "const pick = (x) => (x > 0 ? 'pos' : 'neg');";
    let result = instrument_js(source);

    assert_eq!(result.coverage_map.fn_map.len(), 1);
    assert_body_span_recorded(&result, source, "x > 0 ? 'pos' : 'neg'");

    let branch_types: Vec<&str> =
        result.coverage_map.branch_map.values().map(|entry| entry.branch_type.as_str()).collect();
    assert_eq!(branch_types, vec!["cond-expr"], "the arm branch must survive the body wrapping");
    assert!(
        result.code.contains(".b[0][0]") && result.code.contains(".b[0][1]"),
        "both arms must still increment their branch counters"
    );
}

// TypeScript-stripping path spans

/// `(name, source, body-as-written)` for every TypeScript-only wrapper that
/// `strip_typescript` rewrites away. The recorded span must be the wrapper's,
/// not the inner expression the strip pass leaves behind.
const TS_WRAPPER_BODIES: &[(&str, &str, &str)] = &[
    ("as", "const asChain = (x: number) => x as unknown as string;", "x as unknown as string"),
    (
        "satisfies",
        "const check = (x: number) => ({ a: x }) satisfies { a: number };",
        "({ a: x }) satisfies { a: number }",
    ),
    ("type-assertion", "const assert = (x: unknown) => <string>x;", "<string>x"),
    ("non-null", "const present = (x?: number) => x!;", "x!"),
    ("instantiation", "const bind = (f: <T>(v: T) => T) => f<number>;", "f<number>"),
];

#[test]
fn typescript_wrapper_bodies_keep_the_span_the_source_wrote() {
    for &(name, source, body) in TS_WRAPPER_BODIES {
        let result = instrument_ts(source);
        assert_eq!(result.coverage_map.fn_map.len(), 1, "{name}: one arrow");
        assert_body_span_recorded(&result, source, body);
        assert!(
            !result.code.contains(body),
            "{name}: the wrapper itself must be stripped from the emitted code: {}",
            result.code
        );
    }
}

/// A wrapper spanning several lines is where the shrink is visible to a
/// reporter: lcov and the HTML report attribute by line, so losing the
/// wrapper's lines moves the highlight.
#[test]
fn multi_line_typescript_wrapper_body_keeps_every_line() {
    let source = "const widen = (x: number) =>
  x as unknown as
    string;
";
    let result = instrument_ts(source);

    let loc = span_tuple(&result.coverage_map.fn_map["0"].loc);
    assert_eq!(
        loc,
        ((2, 2), (3, 10)),
        "the body runs from line 2 to line 3; a stripped-span read would report line 3 only"
    );
    let statement_spans: Vec<SpanTuple> =
        result.coverage_map.statement_map.values().map(span_tuple).collect();
    assert!(
        statement_spans.contains(&((2, 2), (3, 10))),
        "the body's statement entry must span both lines, got {statement_spans:?}"
    );
}

/// The stripping path wraps concise bodies before the strip pass and the plain
/// path wraps them during the traverse. Sources with no TypeScript in them
/// must not be able to tell the two apart.
#[test]
fn strip_typescript_leaves_plain_concise_arrow_output_unchanged() {
    let source = "const double = (x) => x * 2;
const curried = (x) => (y) => x + y;
const wrap = (x) => ({ v: x });
const nested = (x) => [1, 2].map((y) => y * x);
";
    let plain = instrument(source, "plain.js", &InstrumentOptions::default()).expect("plain");
    let stripped = instrument(
        source,
        "plain.js",
        &InstrumentOptions { strip_typescript: true, ..InstrumentOptions::default() },
    )
    .expect("stripped");

    assert_eq!(plain.code, stripped.code, "emitted code must not depend on the wrapping site");
    assert_eq!(
        serde_json::to_value(&plain.coverage_map).unwrap(),
        serde_json::to_value(&stripped.coverage_map).unwrap(),
        "coverage map must not depend on the wrapping site"
    );
}
