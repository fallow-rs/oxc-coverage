//! Snapshot tests for coverage map output and instrumented code.
//!
//! The snapshots pin the Istanbul-compatible coverage map format and the
//! emitted code, so a change to either shows up as a diff.

use insta::{assert_json_snapshot, assert_snapshot};
use oxc_coverage_instrument::{InstrumentOptions, instrument};

fn instrument_js(source: &str) -> oxc_coverage_instrument::InstrumentResult {
    instrument(source, "test.js", &InstrumentOptions::default()).unwrap()
}

/// The instrumented code without the coverage setup function and its first
/// call, which would otherwise dominate every code snapshot.
fn code_without_preamble(code: &str) -> &str {
    const SETUP_END: &str = "return actualCoverage; }\n";
    let Some(index) = code.find(SETUP_END) else { return code };
    let call = &code[index + SETUP_END.len()..];
    call.find('\n').map_or(call, |end| &call[end + 1..])
}

// Coverage map snapshots

#[test]
fn snapshot_simple_function_coverage_map() {
    let result = instrument_js("function add(a, b) { return a + b; }");
    assert_json_snapshot!("simple_function_coverage_map", result.coverage_map);
}

#[test]
fn snapshot_if_else_coverage_map() {
    let result = instrument_js(
        "function check(x) {\n  if (x > 0) {\n    return 'positive';\n  } else {\n    return 'non-positive';\n  }\n}",
    );
    assert_json_snapshot!("if_else_coverage_map", result.coverage_map);
}

#[test]
fn snapshot_arrow_function_coverage_map() {
    let result =
        instrument_js("const double = (x) => x * 2;\nconst add = (a, b) => { return a + b; };");
    assert_json_snapshot!("arrow_function_coverage_map", result.coverage_map);
}

#[test]
fn snapshot_class_coverage_map() {
    let result = instrument_js(
        "class Calculator {\n  add(a, b) { return a + b; }\n  sub(a, b) { return a - b; }\n}",
    );
    assert_json_snapshot!("class_coverage_map", result.coverage_map);
}

#[test]
fn snapshot_switch_coverage_map() {
    let result = instrument_js(
        "switch(x) {\n  case 1: y = 'one'; break;\n  case 2: y = 'two'; break;\n  default: y = 'other';\n}",
    );
    assert_json_snapshot!("switch_coverage_map", result.coverage_map);
}

#[test]
fn snapshot_logical_expression_coverage_map() {
    let result = instrument_js("const result = a && b || c;");
    assert_json_snapshot!("logical_expression_coverage_map", result.coverage_map);
}

#[test]
fn snapshot_nullish_coalescing_coverage_map() {
    let result = instrument_js("const x = a ?? b;");
    assert_json_snapshot!("nullish_coalescing_coverage_map", result.coverage_map);
}

#[test]
fn snapshot_for_loop_coverage_map() {
    let result = instrument_js("for (let i = 0; i < 10; i++) {\n  console.log(i);\n}");
    assert_json_snapshot!("for_loop_coverage_map", result.coverage_map);
}

// Instrumented code snapshots

#[test]
fn snapshot_simple_function_code() {
    let result = instrument_js("function add(a, b) { return a + b; }");
    assert_snapshot!("simple_function_code", code_without_preamble(&result.code));
}

#[test]
fn snapshot_arrow_expression_body_code() {
    let result = instrument_js("const double = (x) => x * 2;");
    assert_snapshot!("arrow_expression_body_code", code_without_preamble(&result.code));
}

#[test]
fn snapshot_if_else_code() {
    let result = instrument_js(
        "if (x > 0) {\n  console.log('positive');\n} else {\n  console.log('negative');\n}",
    );
    assert_snapshot!("if_else_code", code_without_preamble(&result.code));
}

#[test]
fn snapshot_ternary_code() {
    let result = instrument_js("const x = condition ? 'yes' : 'no';");
    assert_snapshot!("ternary_code", code_without_preamble(&result.code));
}

#[test]
fn snapshot_logical_expression_code() {
    let result = instrument_js("const x = a || b && c;");
    assert_snapshot!("logical_expression_code", code_without_preamble(&result.code));
}

#[test]
fn snapshot_comprehensive_example() {
    let source = r"function greet(name) {
  if (name) {
    return 'Hello, ' + name;
  } else {
    return 'Hello, stranger';
  }
}

const double = (x) => x * 2;

class Calculator {
  divide(a, b) {
    return b !== 0 ? a / b : 0;
  }
}

const result = greet('world') || double(21);";
    let result = instrument_js(source);
    assert_snapshot!("comprehensive_example_code", code_without_preamble(&result.code));
    assert_json_snapshot!("comprehensive_example_coverage_map", result.coverage_map);
}
