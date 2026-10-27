//! Tests for the opt-in `if_consequent_location` option. Without the option,
//! arm 0 of an `if` branch records the whole `IfStatement`, as
//! `istanbul-lib-instrument` does. With the option, arm 0 records the
//! consequent. Arm 1, the umbrella `loc`, and the counters do not change.

use oxc_coverage_instrument::{
    BranchEntry, CompatProfile, InstrumentOptions, InstrumentResult, Location, instrument,
};

type Range = (u32, u32, u32, u32);

fn run(source: &str, options: &InstrumentOptions) -> InstrumentResult {
    instrument(source, "if.js", options).expect("instrument")
}

fn with_option(source: &str) -> InstrumentResult {
    run(source, &InstrumentOptions { if_consequent_location: true, ..InstrumentOptions::default() })
}

const fn range(loc: &Location) -> Range {
    (loc.start.line, loc.start.column, loc.end.line, loc.end.column)
}

/// The 1-based line and 0-based columns of `needle` on line 1 of `source`.
fn single_line_range(source: &str, needle: &str) -> Range {
    let start = u32::try_from(source.find(needle).expect("needle in source")).unwrap();
    let end = start + u32::try_from(needle.len()).unwrap();
    (1, start, 1, end)
}

fn if_branches(result: &InstrumentResult) -> Vec<&BranchEntry> {
    result.coverage_map.branch_map.values().filter(|entry| entry.branch_type == "if").collect()
}

#[test]
fn default_keeps_the_whole_if_statement_for_arm_zero() {
    let source = "if (a) { b(); } else { c(); }";
    let result = run(source, &InstrumentOptions::default());
    let branch = if_branches(&result)[0];

    assert_eq!(range(&branch.locations[0]), range(&branch.loc));
}

#[test]
fn option_reports_the_consequent_block_for_arm_zero() {
    let source = "if (a) { b(); } else { c(); }";
    let result = with_option(source);
    let branch = if_branches(&result)[0];

    assert_eq!(range(&branch.loc), single_line_range(source, source));
    assert_eq!(range(&branch.locations[0]), single_line_range(source, "{ b(); }"));
    assert_eq!(range(&branch.locations[1]), single_line_range(source, "{ c(); }"));
}

#[test]
fn option_reports_a_statement_consequent_without_a_block() {
    let source = "if (a) b();";
    let result = with_option(source);
    let branch = if_branches(&result)[0];

    assert_eq!(range(&branch.locations[0]), single_line_range(source, "b();"));
}

#[test]
fn option_keeps_the_zero_width_implicit_else_at_the_consequent_end() {
    let source = "if (a) { b(); }";
    let result = with_option(source);
    let branch = if_branches(&result)[0];
    let consequent = single_line_range(source, "{ b(); }");

    assert_eq!(range(&branch.locations[0]), consequent);
    assert_eq!(range(&branch.locations[1]), (1, consequent.3, 1, consequent.3));
}

#[test]
fn option_reports_each_consequent_of_an_else_if_chain() {
    let source = "if (a) { b(); } else if (c) { d(); }";
    let result = with_option(source);
    let branches = if_branches(&result);
    let outer = branches[0];
    let inner = branches[1];

    assert_eq!(range(&outer.locations[0]), single_line_range(source, "{ b(); }"));
    assert_eq!(range(&outer.locations[1]), single_line_range(source, "if (c) { d(); }"));
    assert_eq!(range(&inner.locations[0]), single_line_range(source, "{ d(); }"));
}

#[test]
fn option_spans_lines_for_a_multi_line_consequent() {
    let source = "if (a) {\n  b();\n} else {\n  c();\n}\n";
    let result = with_option(source);
    let branch = if_branches(&result)[0];

    assert_eq!(range(&branch.locations[0]), (1, 7, 3, 1));
    assert_eq!(range(&branch.locations[1]), (3, 7, 5, 1));
}

#[test]
fn option_changes_no_counter_and_no_other_map_entry() {
    let source = "function f(a) { if (a) { return 1; } return a ? 2 : 3; }";
    let default_result = run(source, &InstrumentOptions::default());
    let option_result = with_option(source);
    let default_map = &default_result.coverage_map;
    let option_map = &option_result.coverage_map;

    assert_eq!(default_map.statement_map.len(), option_map.statement_map.len());
    assert_eq!(default_map.fn_map.len(), option_map.fn_map.len());
    assert_eq!(default_map.branch_map.len(), option_map.branch_map.len());
    for (key, default_entry) in &default_map.branch_map {
        let option_entry = &option_map.branch_map[key];
        assert_eq!(range(&default_entry.loc), range(&option_entry.loc));
        assert_eq!(default_entry.locations.len(), option_entry.locations.len());
        if default_entry.branch_type != "if" {
            for (left, right) in default_entry.locations.iter().zip(&option_entry.locations) {
                assert_eq!(range(left), range(right));
            }
        }
    }
}

#[test]
fn option_keeps_arm_zero_absent_under_an_ignore_if_pragma() {
    let source = "/* istanbul ignore if */\nif (a) { b(); } else { c(); }";
    let result = with_option(source);
    let branch = if_branches(&result)[0];

    assert_eq!(branch.locations.len(), 1);
    assert_eq!(range(&branch.locations[0]), (2, 21, 2, 29));
}

#[test]
fn istanbul_profile_overrides_the_option() {
    let source = "if (a) { b(); } else { c(); }";
    let options = InstrumentOptions {
        compat: Some(CompatProfile::Istanbul),
        if_consequent_location: true,
        ..InstrumentOptions::default()
    };
    let result = run(source, &options);
    let branch = if_branches(&result)[0];

    assert_eq!(range(&branch.locations[0]), range(&branch.loc));
}

#[test]
fn builder_sets_the_option() {
    let options = InstrumentOptions::default().with_if_consequent_location(true);

    assert!(options.if_consequent_location);
    assert!(!InstrumentOptions::default().if_consequent_location);
}
