//! Automated conformance test suite comparing against istanbul-lib-instrument.
//!
//! Prerequisites: run
//! `node crates/oxc_coverage_instrument/tests/conformance/generate-reference.mjs` to generate
//! reference data from Istanbul. The Rust tests then compare the instrumented output against
//! the canonical Istanbul output for shared fixtures.
//!
//! Each fixture is compared on function count, branch count, branch types,
//! per-branch location counts, statement count, and the shape of the
//! serialized coverage map.

use std::collections::BTreeMap;

use serde::Deserialize;

use oxc_coverage_instrument::{InstrumentOptions, instrument};

/// Deserialized Istanbul reference data (from generate-reference.mjs output).
///
/// Fields the assertions never read are still declared: deserializing them is
/// what fails the suite when a regenerated reference file loses a section or
/// changes its shape.
#[derive(Debug, Deserialize)]
struct IstanbulReference {
    #[expect(dead_code, reason = "deserialized from JSON for structural completeness")]
    path: String,
    statements: usize,
    functions: usize,
    branches: usize,
    #[serde(rename = "statementMap")]
    #[expect(dead_code, reason = "deserialized from JSON for structural completeness")]
    statement_map: BTreeMap<String, serde_json::Value>,
    #[serde(rename = "fnMap")]
    #[expect(dead_code, reason = "deserialized from JSON for structural completeness")]
    fn_map: BTreeMap<String, IstanbulFn>,
    #[serde(rename = "branchMap")]
    branch_map: BTreeMap<String, IstanbulBranch>,
}

#[derive(Debug, Deserialize)]
struct IstanbulFn {
    #[expect(dead_code, reason = "deserialized from JSON for structural completeness")]
    name: String,
    #[expect(dead_code, reason = "deserialized from JSON for structural completeness")]
    line: u32,
}

#[derive(Debug, Deserialize)]
struct IstanbulBranch {
    #[serde(rename = "type")]
    branch_type: String,
    #[expect(dead_code, reason = "deserialized from JSON for structural completeness")]
    line: u32,
    #[serde(rename = "locationCount")]
    location_count: usize,
}

fn load_reference(name: &str) -> IstanbulReference {
    let path = format!("{}/tests/conformance/reference/{name}.json", env!("CARGO_MANIFEST_DIR"));
    let content = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "Missing reference file {path}: {e}. Run: node crates/oxc_coverage_instrument/tests/conformance/generate-reference.mjs"
        )
    });
    serde_json::from_str(&content).unwrap_or_else(|e| panic!("Invalid reference JSON {path}: {e}"))
}

fn load_fixture(filename: &str) -> String {
    let path = format!("{}/tests/conformance/fixtures/{filename}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("Missing fixture {path}: {e}"))
}

fn instrument_fixture(filename: &str) -> oxc_coverage_instrument::InstrumentResult {
    let source = load_fixture(filename);
    let options = InstrumentOptions {
        strip_typescript: std::path::Path::new(filename)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("ts")),
        ..InstrumentOptions::default()
    };
    instrument(&source, filename, &options)
        .unwrap_or_else(|e| panic!("Instrumentation failed for {filename}: {e}"))
}

macro_rules! conformance_test {
    ($test_name:ident, $fixture:literal) => {
        conformance_test!($test_name, $fixture, ".js");
    };
    ($test_name:ident, $fixture:literal, $extension:literal) => {
        mod $test_name {
            use super::*;

            #[test]
            fn function_count_matches_istanbul() {
                let reference = load_reference($fixture);
                let coverage = instrument_fixture(concat!($fixture, $extension)).coverage_map;

                assert_eq!(
                    coverage.fn_map.len(),
                    reference.functions,
                    "{}: function count mismatch (ours={}, istanbul={})",
                    $fixture,
                    coverage.fn_map.len(),
                    reference.functions
                );
            }

            #[test]
            fn branch_count_matches_istanbul() {
                let reference = load_reference($fixture);
                let coverage = instrument_fixture(concat!($fixture, $extension)).coverage_map;

                assert_eq!(
                    coverage.branch_map.len(),
                    reference.branches,
                    "{}: branch count mismatch (ours={}, istanbul={})",
                    $fixture,
                    coverage.branch_map.len(),
                    reference.branches
                );
            }

            #[test]
            fn branch_types_match_istanbul() {
                let reference = load_reference($fixture);
                let coverage = instrument_fixture(concat!($fixture, $extension)).coverage_map;

                let mut our_types: Vec<&str> =
                    coverage.branch_map.values().map(|b| b.branch_type.as_str()).collect();
                our_types.sort();
                let mut istanbul_types: Vec<&str> =
                    reference.branch_map.values().map(|b| b.branch_type.as_str()).collect();
                istanbul_types.sort();
                assert_eq!(
                    our_types, istanbul_types,
                    "{}: branch types mismatch (ours={:?}, istanbul={:?})",
                    $fixture, our_types, istanbul_types
                );
            }

            #[test]
            fn branch_location_counts_match_istanbul() {
                let reference = load_reference($fixture);
                let coverage = instrument_fixture(concat!($fixture, $extension)).coverage_map;

                let our_branches: Vec<usize> =
                    coverage.branch_map.values().map(|b| b.locations.len()).collect();
                let istanbul_branches: Vec<usize> =
                    reference.branch_map.values().map(|b| b.location_count).collect();
                assert_eq!(
                    our_branches, istanbul_branches,
                    "{}: branch location counts mismatch (ours={:?}, istanbul={:?})",
                    $fixture, our_branches, istanbul_branches
                );
            }

            #[test]
            fn statement_count_matches_istanbul() {
                let reference = load_reference($fixture);
                let coverage = instrument_fixture(concat!($fixture, $extension)).coverage_map;

                assert_eq!(
                    coverage.statement_map.len(),
                    reference.statements,
                    "{}: statement count mismatch (ours={}, istanbul={})",
                    $fixture,
                    coverage.statement_map.len(),
                    reference.statements
                );
            }

            #[test]
            fn json_structure_valid() {
                let result = instrument_fixture(concat!($fixture, $extension));
                let json = serde_json::to_value(&result.coverage_map).unwrap();

                assert!(json["path"].is_string(), "{}: missing path", $fixture);
                assert!(json["statementMap"].is_object(), "{}: missing statementMap", $fixture);
                assert!(json["fnMap"].is_object(), "{}: missing fnMap", $fixture);
                assert!(json["branchMap"].is_object(), "{}: missing branchMap", $fixture);
                assert!(json["s"].is_object(), "{}: missing s", $fixture);
                assert!(json["f"].is_object(), "{}: missing f", $fixture);
                assert!(json["b"].is_object(), "{}: missing b", $fixture);

                assert_eq!(
                    json["s"].as_object().unwrap().len(),
                    json["statementMap"].as_object().unwrap().len(),
                    "{}: s/statementMap size mismatch",
                    $fixture
                );
                assert_eq!(
                    json["f"].as_object().unwrap().len(),
                    json["fnMap"].as_object().unwrap().len(),
                    "{}: f/fnMap size mismatch",
                    $fixture
                );
                assert_eq!(
                    json["b"].as_object().unwrap().len(),
                    json["branchMap"].as_object().unwrap().len(),
                    "{}: b/branchMap size mismatch",
                    $fixture
                );
            }

            #[test]
            fn output_is_valid_js() {
                let result = instrument_fixture(concat!($fixture, $extension));
                let allocator = oxc_allocator::Allocator::default();
                let source_type = oxc_span::SourceType::from_path(concat!($fixture, $extension))
                    .unwrap_or_default();
                let parsed = oxc_parser::Parser::new(&allocator, &result.code, source_type).parse();
                assert!(
                    !parsed.diagnostics.has_errors(),
                    "{}: instrumented code has parse errors: {:?}",
                    $fixture,
                    parsed.diagnostics.errors().map(|e| e.to_string()).collect::<Vec<_>>()
                );
            }
        }
    };
}

conformance_test!(c01_function_declaration, "01-function-declaration");
conformance_test!(c02_function_expression, "02-function-expression");
conformance_test!(c03_arrow_expression, "03-arrow-expression");
conformance_test!(c04_arrow_block, "04-arrow-block");
conformance_test!(c05_class_methods, "05-class-methods");
conformance_test!(c06_if_else, "06-if-else");
conformance_test!(c07_if_no_else, "07-if-no-else");
conformance_test!(c08_ternary, "08-ternary");
conformance_test!(c09_switch_cases, "09-switch-cases");
conformance_test!(c10_logical_and, "10-logical-and");
conformance_test!(c11_logical_or, "11-logical-or");
conformance_test!(c12_logical_chain, "12-logical-chain");
conformance_test!(c13_nullish_coalescing, "13-nullish-coalescing");
conformance_test!(c14_nested_if, "14-nested-if");
conformance_test!(c15_multiple_functions, "15-multiple-functions");
conformance_test!(c16_for_loop, "16-for-loop");
conformance_test!(c17_for_of, "17-for-of");
conformance_test!(c18_while_loop, "18-while-loop");
conformance_test!(c19_do_while, "19-do-while");
conformance_test!(c20_default_params, "20-default-params");
conformance_test!(c21_try_catch, "21-try-catch");
conformance_test!(c22_complex_mixed, "22-complex-mixed");
conformance_test!(c23_empty_function, "23-empty-function");
conformance_test!(c24_nested_functions, "24-nested-functions");
conformance_test!(c25_variable_declarations, "25-variable-declarations");
conformance_test!(c26_non_ascii_identifiers, "26-non-ascii-identifiers");
conformance_test!(c27_pragma_branch_boundaries, "27-pragma-branch-boundaries");
conformance_test!(c28_hashbang, "28-hashbang");
conformance_test!(c29_directive_prologue, "29-directive-prologue");
conformance_test!(c30_coverage_binding_collisions, "30-coverage-binding-collisions");
conformance_test!(c31_class_fields, "31-class-fields");
conformance_test!(c32_typescript_stripped, "32-typescript-stripped", ".ts");
conformance_test!(c33_typescript_concise_arrows, "33-typescript-concise-arrows", ".ts");
conformance_test!(c34_parenthesized_expressions, "34-parenthesized-expressions");
conformance_test!(c35_method_modifiers, "35-method-modifiers");
conformance_test!(c36_multiline_parameters, "36-multiline-parameters");
