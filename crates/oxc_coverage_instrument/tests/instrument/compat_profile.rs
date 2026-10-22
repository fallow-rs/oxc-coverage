use oxc_coverage_instrument::{CompatProfile, InstrumentOptions, instrument};

fn istanbul(source: &str) -> oxc_coverage_instrument::InstrumentResult {
    let options =
        InstrumentOptions { compat: Some(CompatProfile::Istanbul), ..InstrumentOptions::default() };
    instrument(source, "compat.js", &options).expect("instrument with Istanbul profile")
}

#[test]
fn istanbul_profile_skips_logical_assignment_branches() {
    for source in ["let x; x ??= 1;", "let x; x ||= 1;", "let x = 1; x &&= 2;"] {
        let result = istanbul(source);
        assert!(result.coverage_map.branch_map.is_empty(), "unexpected branch for {source}");
    }
}

#[test]
fn istanbul_profile_uses_anonymous_names_for_inferred_functions() {
    let result = istanbul(
        "const f = function () {}; const g = () => 1; class C { method() {} } const o = { run() {} };",
    );
    let names =
        result.coverage_map.fn_map.values().map(|entry| entry.name.as_str()).collect::<Vec<_>>();

    assert_eq!(names, ["(anonymous_0)", "(anonymous_1)", "(anonymous_2)", "(anonymous_3)"]);
}

#[test]
fn istanbul_profile_keeps_anonymous_default_export_anonymous() {
    let result = istanbul("export default function () {}");

    assert_eq!(result.coverage_map.fn_map["0"].name, "(anonymous_0)");
}

#[test]
fn istanbul_profile_keeps_explicit_function_names() {
    let result = istanbul("function declared() {} const value = function explicit() {};");

    assert_eq!(result.coverage_map.fn_map["0"].name, "declared");
    assert_eq!(result.coverage_map.fn_map["1"].name, "explicit");
}

#[test]
fn istanbul_profile_overrides_callback_name_extension() {
    let options = InstrumentOptions {
        compat: Some(CompatProfile::Istanbul),
        name_callback_arguments: true,
        ..InstrumentOptions::default()
    };
    let result = instrument("items.map(() => 1);", "compat.js", &options).expect("instrument");

    assert_eq!(result.coverage_map.fn_map["0"].name, "(anonymous_0)");
}

#[test]
fn istanbul_profile_truncates_method_declaration_spans() {
    let result = istanbul("class C { method() {} }");

    let entry = &result.coverage_map.fn_map["0"];
    assert_eq!(entry.decl.end.column, entry.decl.start.column + 1);
}

#[test]
fn istanbul_profile_anchors_object_method_declarations_at_the_key() {
    let source = "const o = { execute() {} };";
    let result = istanbul(source);
    let entry = &result.coverage_map.fn_map["0"];
    let expected_start = u32::try_from(source.find("execute").expect("method key")).unwrap();

    assert_eq!(entry.decl.start.column, expected_start);
    assert_eq!(entry.decl.end.column, expected_start + 1);
}

#[test]
fn istanbul_profile_leaves_optional_chains_native() {
    let result = istanbul("const value = object?.nested?.value;");

    assert!(result.coverage_map.branch_map.is_empty());
    assert!(!result.code.contains("_oc("));
}

#[test]
fn istanbul_profile_emits_empty_coordinates_for_synthetic_else() {
    let result = istanbul("if (value) work();");
    let value: serde_json::Value =
        serde_json::from_str(&result.coverage_map_json).expect("coverage JSON");
    let alternate = &value["branchMap"]["0"]["locations"][1];

    assert_eq!(alternate["start"], serde_json::json!({}));
    assert_eq!(alternate["end"], serde_json::json!({}));
}

#[test]
fn default_profile_keeps_oxc_extensions() {
    let result = instrument(
        "let x; x ??= 1; const f = function () {}; class C { method() {} } const value = object?.x;",
        "default.js",
        &InstrumentOptions::default(),
    )
    .expect("instrument with defaults");

    assert!(
        result.coverage_map.branch_map.values().any(|entry| entry.branch_type == "binary-expr")
    );
    assert!(
        result.coverage_map.branch_map.values().any(|entry| entry.branch_type == "optional-chain")
    );
    assert_eq!(result.coverage_map.fn_map["0"].name, "f");
    assert_eq!(result.coverage_map.fn_map["1"].name, "method");
    assert!(
        result.coverage_map.fn_map["1"].decl.end.column
            > result.coverage_map.fn_map["1"].decl.start.column + 1
    );
}

/// The source text a single-line coverage location covers.
fn covered_text<'s>(source: &'s str, loc: &oxc_coverage_instrument::Location) -> &'s str {
    assert_eq!(loc.start.line, loc.end.line, "single-line location expected");
    &source[loc.start.column as usize..loc.end.column as usize]
}

#[test]
fn istanbul_profile_excludes_parentheses_from_statement_spans() {
    let source = "const total = 1; const wrapped = ((total)); const ratio = (total - 1) / 2;";
    let result = istanbul(source);
    let texts = result
        .coverage_map
        .statement_map
        .values()
        .map(|loc| covered_text(source, loc))
        .collect::<Vec<_>>();

    assert_eq!(texts, ["1", "total", "(total - 1) / 2"]);
}

#[test]
fn istanbul_profile_excludes_parentheses_from_conditional_arms() {
    let source = "const c = 1; const v = c ? (c, 2) : ((c - 1));";
    let result = istanbul(source);
    let branch = &result.coverage_map.branch_map["0"];
    let arms = branch.locations.iter().map(|loc| covered_text(source, loc)).collect::<Vec<_>>();

    assert_eq!(arms, ["c, 2", "c - 1"]);
}

#[test]
fn istanbul_profile_excludes_parentheses_from_concise_arrow_bodies() {
    let source = "const f = (a, b) => (a, b);";
    let result = istanbul(source);
    let entry = &result.coverage_map.fn_map["0"];

    assert_eq!(covered_text(source, &entry.loc), "a, b");
    let body_statement = result
        .coverage_map
        .statement_map
        .values()
        .map(|loc| covered_text(source, loc))
        .find(|text| *text == "a, b");
    assert!(body_statement.is_some(), "missing body statement without parentheses");
}

#[test]
fn istanbul_profile_anchors_method_declarations_at_the_method_start() {
    let source =
        "const k = 'k'; class C { static make() {} *gen() {} async load() {} [k]() {} *[k]() {} }";
    let result = istanbul(source);
    let starts = result
        .coverage_map
        .fn_map
        .values()
        .map(|entry| &source[entry.decl.start.column as usize..])
        .map(|rest| rest.split_whitespace().next().unwrap_or_default())
        .collect::<Vec<_>>();

    assert_eq!(starts, ["static", "*gen()", "async", "[k]()", "*[k]()"]);
}

#[test]
fn istanbul_profile_takes_function_line_from_the_body() {
    let result = istanbul("function f(\n  a,\n  b,\n) {\n  return a;\n}");
    let entry = &result.coverage_map.fn_map["0"];

    assert_eq!(entry.decl.start.line, 1);
    assert_eq!(entry.line, 4);
}

/// Each function's `decl` start as `(line, column)`, sorted, because istanbul
/// registers a method before the functions in its decorators or computed key.
fn decl_starts(result: &oxc_coverage_instrument::InstrumentResult) -> Vec<(u32, u32)> {
    let mut starts = result
        .coverage_map
        .fn_map
        .values()
        .map(|entry| (entry.decl.start.line, entry.decl.start.column))
        .collect::<Vec<_>>();
    starts.sort_unstable();
    starts
}

#[test]
fn istanbul_profile_keeps_decorator_functions_off_the_method_declaration() {
    let result = istanbul("class C {\n  @dec(function () {})\n  m() {}\n}");

    // `m` is marked at its decorator (2:2), the decorator argument at its
    // own `function` keyword (2:7).
    assert_eq!(decl_starts(&result), [(2, 2), (2, 7)]);
}

#[test]
fn istanbul_profile_keeps_computed_key_functions_off_the_method_declaration() {
    let result = istanbul("class C {\n  [function () { return 'a'; }()]() {}\n}");

    assert_eq!(decl_starts(&result), [(2, 2), (2, 3)]);
}

#[test]
fn default_profile_keeps_the_method_key_as_declaration() {
    let source = "class C {\n  @Input({\n    required: true,\n  })\n  static set value(v) {}\n}";
    let result =
        instrument(source, "compat.js", &InstrumentOptions::default()).expect("instrument");
    let decl = &result.coverage_map.fn_map["0"].decl;

    assert_eq!((decl.start.line, decl.start.column, decl.end.column), (5, 13, 18));
}
