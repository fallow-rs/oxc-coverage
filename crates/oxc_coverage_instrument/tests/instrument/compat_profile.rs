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

/// Each function's name and `decl` start in `fnMap` id order.
fn functions_in_id_order(
    result: &oxc_coverage_instrument::InstrumentResult,
) -> Vec<(String, u32, u32)> {
    let fn_map = &result.coverage_map.fn_map;
    (0..fn_map.len())
        .map(|id| {
            let entry = &fn_map[&id.to_string()];
            (entry.name.clone(), entry.decl.start.line, entry.decl.start.column)
        })
        .collect()
}

fn decl_order(result: &oxc_coverage_instrument::InstrumentResult) -> Vec<(u32, u32)> {
    functions_in_id_order(result).into_iter().map(|(_, line, column)| (line, column)).collect()
}

fn default_profile(source: &str) -> oxc_coverage_instrument::InstrumentResult {
    instrument(source, "default.js", &InstrumentOptions::default()).expect("instrument")
}

#[test]
fn istanbul_profile_registers_a_method_before_its_decorator_function() {
    let result = istanbul("class C {\n  @dec(function () {})\n  m() {}\n}");

    assert_eq!(decl_order(&result), [(2, 2), (2, 7)]);
}

#[test]
fn istanbul_profile_registers_a_method_before_each_of_its_decorators() {
    let result = istanbul("class C {\n  @a(() => 1)\n  @b(function () {})\n  m() {}\n}");

    assert_eq!(decl_order(&result), [(2, 2), (2, 5), (3, 5)]);
}

#[test]
fn istanbul_profile_registers_a_method_before_a_class_in_its_decorator() {
    let result = istanbul("class C {\n  @dec(class { k() {} })\n  m() {}\n}");

    assert_eq!(decl_order(&result), [(2, 2), (2, 15)]);
}

#[test]
fn istanbul_profile_registers_class_and_method_decorators_in_source_order() {
    let source =
        "@d(() => 0)\nclass C {\n  @a(() => 1)\n  m() { return 1 }\n  [(() => 'k')()]() {}\n}";
    let result = istanbul(source);

    assert_eq!(decl_order(&result), [(1, 3), (3, 2), (3, 5), (5, 2), (5, 4)]);
}

#[test]
fn istanbul_profile_names_a_decorator_function_after_the_method() {
    let result = istanbul("class C {\n  @dec(function named() {})\n  m() {}\n}");
    let names = functions_in_id_order(&result).into_iter().map(|(name, ..)| name);

    assert_eq!(names.collect::<Vec<_>>(), ["(anonymous_0)", "named"]);
}

#[test]
fn istanbul_profile_registers_a_method_before_its_computed_key_function() {
    let result = istanbul("class C {\n  [function () { return 'a'; }()]() {}\n}");

    assert_eq!(decl_order(&result), [(2, 2), (2, 3)]);
}

#[test]
fn istanbul_profile_registers_an_object_method_before_its_computed_key_function() {
    let result = istanbul("({ [(() => 'k')()]() {} })");

    assert_eq!(decl_order(&result), [(1, 3), (1, 5)]);
}

#[test]
fn istanbul_profile_counts_a_computed_key_function_on_its_own_counter() {
    let result = istanbul("class C {\n  [function () { return 'a'; }()]() {}\n}");
    let key_counter = result.code.find(".f[1]").expect("key function counter");
    let method_counter = result.code.find(".f[0]").expect("method counter");

    // The key function comes first in the source, and it owns id 1.
    assert!(key_counter < method_counter, "{}", result.code);
}

#[test]
fn istanbul_profile_registers_a_typescript_method_before_its_decorator_function() {
    let options =
        InstrumentOptions { compat: Some(CompatProfile::Istanbul), ..InstrumentOptions::default() };
    let source = "class C {\n  @dec(() => 2)\n  m(@p(function () {}) a) {}\n}";
    let result = instrument(source, "compat.ts", &options).expect("instrument");

    assert_eq!(decl_order(&result), [(2, 2), (2, 7), (3, 7)]);
}

#[test]
fn default_profile_keeps_the_method_name_off_a_named_decorator_function() {
    let result = default_profile("class C {\n  @dec(function named() {})\n  m() {}\n}");
    let names = functions_in_id_order(&result).into_iter().map(|(name, ..)| name);

    assert_eq!(names.collect::<Vec<_>>(), ["m", "named"]);
}

#[test]
fn default_profile_keeps_the_method_name_off_an_anonymous_decorator_function() {
    let result = default_profile("class C {\n  @dec(function () {})\n  m() {}\n}");

    assert_eq!(
        functions_in_id_order(&result),
        [("m".to_string(), 3, 2), ("(anonymous_1)".to_string(), 2, 7)]
    );
}

#[test]
fn default_profile_keeps_the_method_name_off_a_class_in_a_decorator() {
    let result = default_profile("class C {\n  @dec(class { k() {} })\n  m() {}\n}");
    let names = functions_in_id_order(&result).into_iter().map(|(name, ..)| name);

    assert_eq!(names.collect::<Vec<_>>(), ["m", "k"]);
}

#[test]
fn default_profile_registers_a_method_before_its_computed_key_function() {
    let result = default_profile("class C {\n  [function () { return 'a'; }()]() {}\n}");

    assert_eq!(decl_order(&result), [(2, 2), (2, 3)]);
}

#[test]
fn istanbul_profile_keeps_a_decorator_function_on_a_private_method() {
    let result =
        istanbul("class C {\n  @dec(function () { return 1; })\n  #p() { return 2; }\n  m() {}\n}");

    // A private method has no `fnMap` entry, and its decorator function keeps one.
    assert_eq!(decl_order(&result), [(2, 7), (4, 2)]);
}

#[test]
fn istanbul_profile_ignores_binding_names_for_inferred_functions() {
    let cases: [(&str, &[&str]); 8] = [
        ("class C {\n  @dec(() => 1)\n  m() {}\n}", &["(anonymous_0)", "(anonymous_1)"]),
        ("function f(cb = (() => 1)) {}", &["f", "(anonymous_1)"]),
        ("function f(c = class { [k]() {} }) {}", &["f", "(anonymous_1)"]),
        ("const f = (function () {})", &["(anonymous_0)"]),
        ("o.x = (() => 1); x = function () {}", &["(anonymous_0)", "(anonymous_1)"]),
        ("({ a = () => 1 } = o)", &["(anonymous_0)"]),
        ("const f = function g() {}", &["g"]),
        ("o.x = function y() {}", &["y"]),
    ];
    for (source, expected) in cases {
        let names = functions_in_id_order(&istanbul(source))
            .into_iter()
            .map(|(name, _, _)| name)
            .collect::<Vec<_>>();
        assert_eq!(names, expected, "{source}");
    }
}

/// The `(functions, branch location counts, statements)` of a coverage map.
type Shape = (usize, Vec<usize>, usize);

/// The [`Shape`] of `result`, in the order the coverage map holds it.
fn shape(result: &oxc_coverage_instrument::InstrumentResult) -> Shape {
    let map = &result.coverage_map;
    let branches = map.branch_map.values().map(|entry| entry.locations.len()).collect();
    (map.fn_map.len(), branches, map.statement_map.len())
}

#[test]
fn istanbul_profile_keeps_a_member_whose_pragma_follows_a_modifier_or_sits_in_its_key() {
    let cases: [(&str, Shape); 9] = [
        ("class A { static /* istanbul ignore next */ m() {} }", (1, vec![], 0)),
        ("class A { get /* istanbul ignore next */ m() { return 1; } }", (1, vec![], 1)),
        ("o = { async /* istanbul ignore next */ m() {} };", (1, vec![], 1)),
        ("o = { get /* istanbul ignore next */ m() { return 1; } };", (1, vec![], 2)),
        ("class A { [/* istanbul ignore next */ k]() {} }", (1, vec![], 0)),
        ("class A { @dec(() => 1) /* istanbul ignore next */ m() {} n() {} }", (3, vec![], 1)),
        ("o = { [/* istanbul ignore next */ (() => 'k')()]: 1 };", (1, vec![], 2)),
        ("o = { /* istanbul ignore next */ a: b || c };", (0, vec![2], 1)),
        ("o = { /* istanbul ignore next */ a: b ? c : d };", (0, vec![2], 1)),
    ];
    for (source, expected) in cases {
        assert_eq!(shape(&istanbul(source)), expected, "{source}");
    }
}

#[test]
fn istanbul_profile_ignores_a_member_whose_pragma_precedes_it() {
    for source in [
        "class A { /* istanbul ignore next */ static m() {} }",
        "class A { /* istanbul ignore next */ @dec(() => 1) m() {} }",
        "o = { /* istanbul ignore next */ async m() {} };",
        "o = { /* istanbul ignore next */ [k]() {} };",
    ] {
        assert_eq!(shape(&istanbul(source)).0, 0, "{source}");
    }
}

#[test]
fn istanbul_profile_keeps_a_switch_case_whose_first_statement_has_the_pragma() {
    let result =
        istanbul("switch (a) { case 1: /* istanbul ignore next */ b(); c(); case 2: d(); }");

    assert_eq!(shape(&result), (0, vec![2], 3));
    let ignored_case =
        istanbul("switch (a) { /* istanbul ignore next */ case 1: b(); case 2: d(); }");
    assert_eq!(shape(&ignored_case), (0, vec![1], 2));
}

#[test]
fn pragma_kind_ends_at_the_first_non_word_character() {
    for run in [istanbul, default_profile] {
        for (source, expected) in [
            ("/* istanbul ignore next: reason */ function f() {}", (0, vec![], 0)),
            ("/* istanbul ignore next-reason */ function f() {}", (0, vec![], 0)),
            ("x = /* istanbul ignore next, reason */ a || b;", (0, vec![], 1)),
            ("/* istanbul ignore if: why */ if (a) {}", (0, vec![1], 1)),
            ("/* istanbul ignore else:x */ if (a) {} else {}", (0, vec![1], 1)),
            ("/* istanbul ignore nextx */ function f() {}", (1, vec![], 0)),
        ] {
            assert_eq!(shape(&run(source)), expected, "{source}");
        }
    }
}

#[test]
fn istanbul_profile_reads_only_the_pragmas_that_istanbul_reads() {
    for source in [
        "/* c8 ignore next */ function f() {}",
        "/* v8 ignore next */ function f() {}",
        "/*! istanbul ignore next */ function f() {}",
        "/* istanbul ignore start */ function f() {} /* istanbul ignore stop */",
        "/* c8 ignore start */ function f() {} /* c8 ignore stop */",
    ] {
        let result = istanbul(source);
        assert_eq!(shape(&result).0, 1, "{source}");
        assert!(!result.unhandled_pragmas.is_empty(), "{source}");
        assert_eq!(shape(&default_profile(source)).0, 0, "{source}");
    }
    assert_eq!(shape(&istanbul("x = /* c8 ignore next */ (a || b);")).1, [2]);
    assert_eq!(shape(&istanbul("/* c8 ignore file */ f();")), (0, vec![], 1));
}

#[test]
fn ignored_default_ignores_its_subtree() {
    for run in [istanbul, default_profile] {
        for source in [
            "[/* istanbul ignore next */ a = () => 1] = [];",
            "({ k: /* istanbul ignore next */ a = () => 1 } = {});",
            "var [/* istanbul ignore next */ a = () => 1] = [];",
            "var { k: /* istanbul ignore next */ a = () => 1 } = {};",
            "[/* istanbul ignore next */ a = b || c] = [];",
        ] {
            assert_eq!(shape(&run(source)), (0, vec![], 1), "{source}");
        }
        assert_eq!(
            shape(&run("function f(/* istanbul ignore next */ a = b ? c : () => 1) {}")),
            (1, vec![], 0)
        );
    }
}

#[test]
fn istanbul_profile_keeps_a_default_whose_pragma_belongs_to_the_property() {
    for (source, expected) in [
        ("var { /* istanbul ignore next */ a = () => 1 } = {};", (1, vec![1], 2)),
        ("function f({ /* istanbul ignore next */ k: a = () => 1 }) {}", (2, vec![1], 1)),
    ] {
        assert_eq!(shape(&istanbul(source)), expected, "{source}");
    }
}

#[test]
fn parameter_default_location_starts_at_the_binding() {
    let options = |compat| InstrumentOptions { compat, ..InstrumentOptions::default() };
    for compat in [Some(CompatProfile::Istanbul), None] {
        for (source, start) in [
            ("class A { constructor(public a = () => 1) {} }", 29),
            ("class A { constructor(private readonly a = 1) {} }", 39),
            ("class A { m(@dec a = 1) {} }", 17),
        ] {
            let result = instrument(source, "compat.ts", &options(compat)).expect("instrument");
            let entry = &result.coverage_map.branch_map["0"];
            assert_eq!(entry.loc.start.column, start, "{source}");
        }
        let ignored = "class A { constructor(public /* istanbul ignore next */ a = () => 1) {} }";
        let result = instrument(ignored, "compat.ts", &options(compat)).expect("instrument");
        assert_eq!(shape(&result), (1, vec![], 0));
    }
    let kept = "class A { constructor(/* istanbul ignore next */ public a = () => 1) {} }";
    let result =
        instrument(kept, "compat.ts", &options(Some(CompatProfile::Istanbul))).expect("instrument");
    assert_eq!(shape(&result), (2, vec![1], 1));
}

/// The class from fallow-rs/oxc-coverage-instrument#303: a private and a public
/// method with the same body.
const PRIVATE_TWIN: &str = "class Twin {
  #privateMethod(value){
    if(value===0)return 10
    return 70
  }
  publicMethod(value){
    if(value===0)return 10
    return 70
  }
  callPrivate(value){return this.#privateMethod(value)}
}
";

#[test]
fn instrumentation_keeps_private_methods_out_of_fn_map_in_both_profiles() {
    // Only the V8 collect path records a private method. Instrumentation keeps
    // the Istanbul shape: the private body gets statement and branch counters,
    // but no function counter and no `fnMap` entry.
    for (profile, result) in
        [("istanbul", istanbul(PRIVATE_TWIN)), ("default", default_profile(PRIVATE_TWIN))]
    {
        let map = &result.coverage_map;
        let lines = decl_order(&result).into_iter().map(|(line, _)| line).collect::<Vec<_>>();
        assert_eq!(lines, [6, 10], "{profile}: only the public methods are in fnMap");
        assert!(map.fn_map.values().all(|entry| !entry.name.contains("privateMethod")));
        assert_eq!(map.branch_map.len(), 2, "{profile}: both bodies keep their branches");
        assert_eq!(map.statement_map.len(), 7, "{profile}: both bodies keep their statements");
        assert_eq!(result.code.matches(".f[").count(), 2, "{profile}: two function counters");
    }
}

/// Babel attaches a comment after `export const` to the declarator and a
/// comment after `export default` to the class declaration, and istanbul
/// ignores each subtree. Both profiles follow it.
#[test]
fn ignore_next_after_export_targets_the_declarator_or_the_class() {
    let sources = [
        "export const /* istanbul ignore next */ f = () => 1;",
        "export default /* istanbul ignore next */ class { m() { return 1; } }",
    ];
    for source in sources {
        for options in [
            InstrumentOptions {
                compat: Some(CompatProfile::Istanbul),
                ..InstrumentOptions::default()
            },
            InstrumentOptions::default(),
        ] {
            let map = instrument(source, "compat.js", &options).expect("instrument").coverage_map;
            assert!(
                map.fn_map.is_empty() && map.statement_map.is_empty(),
                "{source} ({:?}): fn {:?}, statements {:?}",
                options.compat,
                map.fn_map,
                map.statement_map
            );
        }
    }
}
