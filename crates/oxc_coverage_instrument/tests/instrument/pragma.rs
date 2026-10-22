//! Coverage-pragma handling: `istanbul` / `v8` / `c8` ignore directives.
//!
//! Covers `ignore next` / `if` / `else` / `file` / `start` / `stop`, where a
//! pragma anchors, how far a skip reaches into a subtree, and what lands in
//! `unhandled_pragmas`.

use oxc_coverage_instrument::{InstrumentOptions, instrument};

fn instrument_js(source: &str) -> oxc_coverage_instrument::InstrumentResult {
    instrument(source, "test.js", &InstrumentOptions::default()).unwrap()
}

const ECMASCRIPT_LINE_TERMINATORS: [(&str, &str); 5] =
    [("LF", "\n"), ("CRLF", "\r\n"), ("CR", "\r"), ("LS", "\u{2028}"), ("PS", "\u{2029}")];

#[test]
fn pragma_ignore_else_on_chained_else_if_binds_to_inner_if() {
    // /* istanbul ignore else */ placed between `else` and the chained `if`
    // anchors on the inner `if`. The inner if's alternate arm drops; the
    // outer if is unaffected.
    let r = instrument_js(
        "function f(a) {\n  if (a === 1) { return 1; }\n  /* istanbul ignore else */\n  else if (a === 2) { return 2; }\n  else { return 3; }\n}",
    );
    let entries: Vec<(usize, String)> = r
        .coverage_map
        .branch_map
        .values()
        .map(|e| (e.locations.len(), e.branch_type.clone()))
        .collect();
    assert!(
        entries.iter().any(|(n, t)| *n == 2 && t == "if"),
        "outer if should keep both arms: {entries:?}"
    );
    assert!(
        entries.iter().any(|(n, t)| *n == 1 && t == "if"),
        "inner if's alternate arm should be dropped: {entries:?}"
    );
    assert!(r.unhandled_pragmas.is_empty());
}

#[test]
fn pragma_ignore_if_on_no_else_branch_anchors_surviving_arm() {
    // `/* istanbul ignore if */` on a no-else if drops the consequent arm.
    // The single surviving slot must still carry a real `Location`; an
    // empty `{ start: {}, end: {} }` here crashes downstream reporters.
    let result = instrument_js("function f(x) { /* istanbul ignore if */ if (x) { return 1; } }");
    assert_eq!(result.coverage_map.branch_map.len(), 1);
    let entry = result.coverage_map.branch_map.values().next().unwrap();
    assert_eq!(entry.locations.len(), 1);
    let json = serde_json::to_value(&result.coverage_map).unwrap();
    let arm = &json["branchMap"]["0"]["locations"][0];
    assert!(
        arm["start"].get("line").is_some(),
        "ignored-if surviving arm must have a real start line: {arm}"
    );
}

#[test]
fn pragma_ignore_file_supports_istanbul_v8_and_c8() {
    for tool in ["istanbul", "v8", "c8"] {
        let result =
            instrument_js(&format!("/* {tool} ignore file */\nfunction f() {{ return 1; }}"));
        assert!(result.coverage_map.fn_map.is_empty(), "{tool}");
        assert!(result.coverage_map.statement_map.is_empty(), "{tool}");
        assert!(result.coverage_map.branch_map.is_empty(), "{tool}");
        assert!(!result.code.contains("cov_"), "{tool}: an ignored file is returned unmodified");
    }
}

#[test]
fn pragma_istanbul_ignore_next_function() {
    let result = instrument_js(
        "/* istanbul ignore next */\nfunction ignored() { return 1; }\nfunction counted() { return 2; }",
    );
    let fn_names: Vec<&str> =
        result.coverage_map.fn_map.values().map(|f| f.name.as_str()).collect();
    assert!(fn_names.contains(&"counted"));
    assert!(!fn_names.contains(&"ignored"));
}

#[test]
fn pragma_ignore_next_arrow_function() {
    let result = instrument_js(
        "/* istanbul ignore next */\nconst ignored = () => 1;\nconst counted = () => 2;",
    );
    let fn_names: Vec<&str> =
        result.coverage_map.fn_map.values().map(|f| f.name.as_str()).collect();
    assert!(fn_names.contains(&"counted"));
    assert!(!fn_names.contains(&"ignored"));
}

#[test]
fn pragma_ignore_if_skips_consequent_counter() {
    let result = instrument_js(
        "function f(x) {\n  /* istanbul ignore if */\n  if (x < 0) { throw new Error(); } else { return x; }\n}",
    );
    assert_eq!(result.coverage_map.branch_map.len(), 1);
    assert_eq!(result.coverage_map.branch_map["0"].locations.len(), 1);
    assert!(result.code.contains(".b[0][0]"));
    assert!(!result.code.contains(".b[0][1]"));
    assert_eq!(
        result.coverage_map.statement_map.len(),
        2,
        "ignore if should also skip statement counters in the consequent arm"
    );
}

#[test]
fn pragma_ignore_else_skips_alternate_counter() {
    let result = instrument_js(
        "function f(x) {\n  /* istanbul ignore else */\n  if (x > 0) { return 'pos'; } else { return 'neg'; }\n}",
    );
    assert_eq!(result.coverage_map.branch_map.len(), 1);
    assert_eq!(result.coverage_map.branch_map["0"].locations.len(), 1);
    assert!(result.code.contains(".b[0][0]"));
    assert!(!result.code.contains(".b[0][1]"));
    assert_eq!(
        result.coverage_map.statement_map.len(),
        2,
        "ignore else should also skip statement counters in the alternate arm"
    );
}

#[test]
fn pragma_ignore_if_without_else_skips_consequent_statement_counter() {
    let result = instrument_js(
        "function f(x) {\n  /* istanbul ignore if */\n  if (x) return 1;\n  return 2;\n}",
    );
    assert_eq!(result.coverage_map.branch_map.len(), 1);
    assert_eq!(result.coverage_map.branch_map["0"].locations.len(), 1);
    assert_eq!(
        result.coverage_map.statement_map.len(),
        2,
        "ignore if should skip the inline consequent return statement"
    );
}

#[test]
fn block_ignore_pragmas_skip_statements_between_start_and_stop() {
    let source = "function f(x) {\n  /* v8 ignore start */\n  if (x) { return 1; }\n  return 2;\n  /* v8 ignore stop */\n}\nf(false);";
    let result = instrument_js(source);

    assert!(result.unhandled_pragmas.is_empty());
    assert_eq!(result.coverage_map.fn_map.len(), 1, "the enclosing function should still count");
    assert_eq!(result.coverage_map.branch_map.len(), 0, "ignored block should skip the if branch");
    assert_eq!(
        result.coverage_map.statement_map.len(),
        1,
        "only the call after the ignored block should remain counted"
    );
    let stmt = result.coverage_map.statement_map.values().next().unwrap();
    assert_eq!(stmt.start.line, 7);
}

#[test]
fn block_ignore_pragmas_support_istanbul_v8_and_c8() {
    for tool in ["istanbul", "v8", "c8"] {
        let source = format!(
            "/* {tool} ignore start */\nfunction ignored() {{ return 1; }}\n/* {tool} ignore stop */\nfunction counted() {{ return 2; }}"
        );
        let result = instrument_js(&source);
        let fn_names: Vec<&str> =
            result.coverage_map.fn_map.values().map(|f| f.name.as_str()).collect();

        assert!(result.unhandled_pragmas.is_empty(), "{tool} block pragmas should be handled");
        assert!(!fn_names.contains(&"ignored"), "{tool} block should skip ignored function");
        assert!(fn_names.contains(&"counted"), "{tool} block should stop before counted function");
    }
}

#[test]
fn pragma_v8_ignore_next() {
    let result =
        instrument_js("/* v8 ignore next */\nfunction ignored() {}\nfunction counted() {}");
    let fn_names: Vec<&str> =
        result.coverage_map.fn_map.values().map(|f| f.name.as_str()).collect();
    assert!(!fn_names.contains(&"ignored"));
    assert!(fn_names.contains(&"counted"));
}

#[test]
fn pragma_ignore_next_skips_class_property_initializer_subtree() {
    let source = "class C {\n  /* istanbul ignore next */\n  x = () => 1;\n}\na();";
    let result = instrument_js(source);
    assert!(result.unhandled_pragmas.is_empty());
    assert_eq!(
        result.coverage_map.statement_map.len(),
        1,
        "only the following `a();` should count"
    );
    assert_eq!(
        result.coverage_map.fn_map.len(),
        0,
        "arrow initializer inside ignored property must not count"
    );
    let stmt = result.coverage_map.statement_map.values().next().unwrap();
    assert_eq!(stmt.start.line, 5);
}

#[test]
fn ignore_class_methods_with_pragma_no_leak() {
    // When both ignoreClassMethods AND a pragma target the same method,
    // skip_next must not leak to the next statement after the method.
    let opts = InstrumentOptions {
        ignore_class_methods: vec!["render".to_string()],
        ..InstrumentOptions::default()
    };
    let result = instrument(
        "class App { /* istanbul ignore next */ render() { return 1; } update() { return 2; } }",
        "test.js",
        &opts,
    )
    .unwrap();
    // `render` is skipped by both the pragma and `ignoreClassMethods`, while
    // `update` stays counted.
    assert_eq!(result.coverage_map.fn_map.len(), 1, "Only update should have a function counter");
    assert_eq!(result.coverage_map.fn_map["0"].name, "update");
}

#[test]
fn unhandled_pragma_column_is_utf16_code_units() {
    // The unknown pragma's column must reflect UTF-16 code units so reporters
    // highlight the correct span when the source contains non-ASCII.
    let source = "const π = 1; /* istanbul ignore bogus */ const y = 2;";
    let result = instrument(source, "test.js", &InstrumentOptions::default()).unwrap();
    assert_eq!(result.unhandled_pragmas.len(), 1);
    let pragma = &result.unhandled_pragmas[0];
    assert_eq!(pragma.line, 1);
    // "const π = 1; " is 13 UTF-16 code units; the `/` of the comment starts at col 13.
    assert_eq!(pragma.column, 13, "pragma column should be UTF-16 code units, got {pragma:?}");
}

#[test]
fn pragma_ignore_next_does_not_leak_to_sibling_statement() {
    // Istanbul skips only the annotated statement, so the top-level `f();`
    // after the enclosing function still counts.
    let source = "function f() {\n  /* istanbul ignore next */\n  return 1;\n}\nf();";
    let result = instrument_js(source);
    assert_eq!(
        result.coverage_map.statement_map.len(),
        1,
        "only the annotated `return 1;` should be skipped; `f();` must still count"
    );
    let stmt = result.coverage_map.statement_map.values().next().unwrap();
    assert_eq!(stmt.start.line, 5);
}

#[test]
fn pragma_ignore_next_three_sibling_statements() {
    // The pragma applies to `a();` alone; `b();` and `c();` both stay counted.
    let source = "/* istanbul ignore next */\na();\nb();\nc();";
    let result = instrument_js(source);
    assert_eq!(result.coverage_map.statement_map.len(), 2);
    let lines: Vec<u32> =
        result.coverage_map.statement_map.values().map(|loc| loc.start.line).collect();
    assert!(lines.contains(&3), "b(); on line 3 should count");
    assert!(lines.contains(&4), "c(); on line 4 should count");
}

#[test]
fn pragma_ignore_next_skips_arrow_body_statements() {
    // Istanbul's `path.skip()` semantics: an ignored function or arrow skips
    // its whole subtree, statements in the body included.
    let source = "/* istanbul ignore next */\nfoo(() => bar());\nqux();";
    let result = instrument_js(source);
    let fn_names: Vec<&str> =
        result.coverage_map.fn_map.values().map(|f| f.name.as_str()).collect();
    assert!(fn_names.is_empty(), "arrow inside ignored statement must not count, got {fn_names:?}");
    assert_eq!(
        result.coverage_map.statement_map.len(),
        1,
        "only `qux();` should count; `bar()` inside the ignored arrow must be skipped"
    );
}

#[test]
fn pragma_ignore_next_skips_nested_function_body() {
    // An ignored function's body produces no statement or inner function
    // counters, however deeply nested.
    let source = "/* istanbul ignore next */\nfunction ignored() {\n  const x = 1;\n  return x + 1;\n}\nignored();";
    let result = instrument_js(source);
    assert_eq!(result.coverage_map.fn_map.len(), 0, "ignored function should not produce fn entry");
    assert_eq!(
        result.coverage_map.statement_map.len(),
        1,
        "only `ignored();` should count; body statements must be skipped"
    );
}

#[test]
fn pragma_ignore_next_skips_if_statement_subtree() {
    // `ignore next` on an `IfStatement` suppresses the branch entry and every
    // statement nested inside the if body.
    let source = "/* istanbul ignore next */\nif (!a) { const b = 2; }";
    let result = instrument_js(source);
    assert!(result.unhandled_pragmas.is_empty());
    assert_eq!(result.coverage_map.branch_map.len(), 0, "ignored if must not add a branch");
    assert_eq!(
        result.coverage_map.statement_map.len(),
        0,
        "ignored if body statements must not be counted"
    );
}

#[test]
fn pragma_ignore_next_skips_return_expression_subtree() {
    // `ignore next` on a `ReturnStatement` suppresses the branch counters of
    // expressions nested inside it.
    let source = "function f(x) {\n  /* c8 ignore next */\n  return x ? 1 : 2;\n}";
    let result = instrument_js(source);
    assert!(result.unhandled_pragmas.is_empty());
    assert_eq!(result.coverage_map.fn_map.len(), 1);
    assert_eq!(
        result.coverage_map.branch_map.len(),
        0,
        "ignored return must not add a ternary branch"
    );
    assert_eq!(
        result.coverage_map.statement_map.len(),
        0,
        "ignored return statement must not be counted"
    );
}

#[test]
fn pragma_ignore_next_if_does_not_leak_to_following_statement() {
    let source = "function f(x) {\n  /* v8 ignore next */\n  if (x) { return 1; }\n  return 2;\n}";
    let result = instrument_js(source);
    assert!(result.unhandled_pragmas.is_empty());
    assert_eq!(result.coverage_map.fn_map.len(), 1);
    assert_eq!(result.coverage_map.branch_map.len(), 0, "ignored if must not add a branch");
    assert_eq!(
        result.coverage_map.statement_map.len(),
        1,
        "only the following `return 2;` should remain counted"
    );
    let stmt = result.coverage_map.statement_map.values().next().unwrap();
    assert_eq!(stmt.start.line, 4);
}

#[test]
fn pragma_ignore_next_skips_object_method_subtree() {
    let source = "const obj = {\n  /* v8 ignore next */\n  method(x) {\n    const y = x.foo;\n    if (y) { y.bar = 1; }\n  },\n};";
    let result = instrument_js(source);
    assert!(result.unhandled_pragmas.is_empty());
    assert_eq!(result.coverage_map.fn_map.len(), 0, "ignored method should not add a function");
    assert_eq!(
        result.coverage_map.branch_map.len(),
        0,
        "ignored method body should not add branches"
    );
    assert_eq!(
        result.coverage_map.statement_map.len(),
        1,
        "only the object initializer should remain counted"
    );
}

#[test]
fn pragma_ignore_next_skips_class_method_subtree() {
    let source = "class C {\n  /* istanbul ignore next */\n  render(x) {\n    if (x) { return 1; }\n    return 2;\n  }\n\n  update() { return 3; }\n}";
    let result = instrument_js(source);
    let fn_names: Vec<&str> =
        result.coverage_map.fn_map.values().map(|f| f.name.as_str()).collect();
    assert!(!fn_names.contains(&"render"), "ignored class method should not add a function");
    assert!(fn_names.contains(&"update"), "sibling class method should still be counted");
    assert_eq!(
        result.coverage_map.branch_map.len(),
        0,
        "ignored class method body should not add branches"
    );
}

#[test]
fn pragma_ignore_next_skips_class_getter_subtree() {
    let source = "class C {\n  /* istanbul ignore next */\n  get value() {\n    if (a) { return 1; }\n    return 2;\n  }\n}";
    let result = instrument_js(source);
    assert!(result.coverage_map.fn_map.is_empty(), "ignored getter should not add a function");
    assert!(
        result.coverage_map.branch_map.is_empty(),
        "ignored getter body should not add branches"
    );
}

#[test]
fn pragma_ignore_next_before_private_class_method_matches_istanbul_boundary() {
    let source = "class C {\n  /* istanbul ignore next */\n  #secret(x) { if (x) { return 1; } return 2; }\n  run(x) { return this.#secret(x); }\n}";
    let result = instrument_js(source);
    assert_eq!(
        result.coverage_map.fn_map.len(),
        1,
        "Istanbul does not apply method-key ignore pragmas to private methods"
    );
    assert_eq!(
        result.coverage_map.branch_map.len(),
        1,
        "private method body should still be instrumented"
    );
}

#[test]
fn pragma_before_function_valued_object_property_does_not_skip_value() {
    let source = "const obj = {\n  /* istanbul ignore next */\n  method: function (x) {\n    if (x) { return 1; }\n    return 2;\n  },\n};";
    let result = instrument_js(source);
    assert_eq!(
        result.coverage_map.fn_map.len(),
        1,
        "Istanbul does not apply a property-key pragma to a function-valued property"
    );
    assert_eq!(
        result.coverage_map.branch_map.len(),
        1,
        "function-valued property body should still be instrumented"
    );
}

#[test]
fn pragma_ignore_next_skips_ternary_branch_counter() {
    let source = "function f(x) {\n  return x.set ? { a: 1 } : /* v8 ignore next */ {};\n}";
    let result = instrument_js(source);
    assert!(result.unhandled_pragmas.is_empty());
    // Per istanbul, an ignored ternary arm collapses that arm out of the
    // branch map but the surviving arm continues to be tracked, so the
    // entry stays in `branchMap` with one location and one counter.
    assert_eq!(result.coverage_map.branch_map.len(), 1);
    let entry = result.coverage_map.branch_map.values().next().unwrap();
    assert_eq!(entry.locations.len(), 1);
    assert_eq!(result.coverage_map.b.get("0").map(Vec::len), Some(1));
    assert!(result.code.contains(".b[0][0]"));
    assert!(!result.code.contains(".b[0][1]"));
}

#[test]
fn pragma_ignore_next_skips_nested_object_spread_ternary_arm() {
    let source = "function f(x) {\n  return {\n    ...x,\n    ...(x.set\n      ? { a: 1 }\n      : /* v8 ignore next -- @preserve */\n        {}),\n  };\n}";
    let result = instrument_js(source);
    assert!(result.unhandled_pragmas.is_empty());
    assert_eq!(result.coverage_map.fn_map.len(), 1, "the enclosing function should still count");
    assert_eq!(result.coverage_map.branch_map.len(), 1);
    let entry = result.coverage_map.branch_map.values().next().unwrap();
    assert_eq!(entry.locations.len(), 1);
    assert_eq!(result.coverage_map.b.get("0").map(Vec::len), Some(1));
    assert!(result.code.contains(".b[0][0]"));
    assert!(!result.code.contains(".b[0][1]"));
}

#[test]
fn pragma_ignore_next_skips_default_arg_on_destructure_property() {
    // Shorthand object property with a default value.
    let r = instrument_js("function f({ /* istanbul ignore next */ y = 1 }) {}");
    assert!(
        r.coverage_map.branch_map.is_empty(),
        "inner default-arg should be suppressed: {:?}",
        r.coverage_map.branch_map
    );
    assert!(r.unhandled_pragmas.is_empty());

    // Named property (non-shorthand): pragma anchors on the BindingProperty's
    // start, the inner AssignmentPattern starts on the value name.
    let r = instrument_js("function f({ /* istanbul ignore next */ key: y = 1 } = {}) {}");
    assert_eq!(r.coverage_map.branch_map.len(), 1, "only the outer object default should remain");

    // Array element.
    let r = instrument_js("function f([/* istanbul ignore next */ a = 1] = []) {}");
    assert_eq!(r.coverage_map.branch_map.len(), 1, "only the outer array default should remain");

    // Plain formal parameter.
    let r = instrument_js("function f(/* istanbul ignore next */ z = 1) {}");
    assert!(r.coverage_map.branch_map.is_empty());

    // Sibling default-arg branches are unaffected.
    let r = instrument_js("function f({ x = 1, /* istanbul ignore next */ y = 2 } = {}) {}");
    let labels: Vec<&str> =
        r.coverage_map.branch_map.values().map(|e| e.branch_type.as_str()).collect();
    assert_eq!(
        labels,
        vec!["default-arg", "default-arg"],
        "outer object default and sibling `x = 1` survive"
    );
}

#[test]
fn pragma_ignore_next_prunes_empty_ternary_branch() {
    let source =
        "function f(x) {\n  return x ? /* v8 ignore next */ 1 : /* v8 ignore next */ 2;\n}";
    let result = instrument_js(source);
    assert!(result.unhandled_pragmas.is_empty());
    assert_eq!(
        result.coverage_map.branch_map.len(),
        0,
        "branches with no instrumented paths should be pruned like Istanbul"
    );
    assert_eq!(result.coverage_map.b.len(), 0);
}

#[test]
fn pragma_ignore_next_keeps_branch_entry_when_one_arm_is_ignored() {
    let cases = [
        "function f(x) { return x ?? /* v8 ignore next -- @preserve */ [] }",
        "function f(x) { return x || /* v8 ignore next -- @preserve */ true }",
        "function f(x) { return x && /* v8 ignore next -- @preserve */ true }",
        "function f(x) { return x && /* v8 ignore next */ true }",
        "function f(x) { return x ? 1 : /* v8 ignore next -- @preserve */ 2 }",
    ];

    for source in cases {
        let result = instrument_js(source);
        assert!(result.unhandled_pragmas.is_empty());
        assert_eq!(
            result.coverage_map.branch_map.len(),
            1,
            "the surviving arm of an ignore-next branch must stay tracked:\n{source}"
        );
        let entry = result.coverage_map.branch_map.values().next().unwrap();
        assert_eq!(entry.locations.len(), 1, "ignored arm must be dropped:\n{source}");
        assert_eq!(
            result.coverage_map.b.get("0").map(Vec::len),
            Some(1),
            "hit array must match branchMap:\n{source}"
        );
        assert!(
            result.code.contains(".b[0][0]"),
            "surviving arm counter must be emitted:\n{source}"
        );
        assert!(
            !result.code.contains(".b[0][1]"),
            "ignored arm counter must not be emitted:\n{source}"
        );
    }
}

#[test]
fn pragma_ignore_next_skips_jsx_attribute_value_subtree() {
    let cases = [
        (
            "function f() {\n  return <Tag\n    /* v8 ignore next -- @preserve */\n    onClick={() => doSomething()}\n  />\n}",
            1,
            0,
        ),
        (
            "function f(pass) {\n  return <Tag\n    /* v8 ignore next -- @preserve */\n    text={pass ? 'Pass' : 'Fail'}\n  />\n}",
            1,
            0,
        ),
        (
            "function f(name) {\n  return <Tag\n    /* v8 ignore next -- @preserve */\n    name={name || 'fallback'}\n  />\n}",
            1,
            0,
        ),
    ];

    for (source, expected_fns, expected_branches) in cases {
        let result = instrument(source, "test.tsx", &InstrumentOptions::default()).unwrap();
        assert!(result.unhandled_pragmas.is_empty());
        assert_eq!(
            result.coverage_map.fn_map.len(),
            expected_fns,
            "JSX attribute pragma should skip nested functions:\n{source}"
        );
        assert_eq!(
            result.coverage_map.branch_map.len(),
            expected_branches,
            "JSX attribute pragma should skip nested branches:\n{source}"
        );
    }
}

#[test]
fn jsx_expression_container_pragma_ignore_next_skips_next_child_subtree() {
    let source = "function f(x) {\n  return <div>\n    {/* v8 ignore next -- @preserve */}\n    {x ? <a/> : <b/>}\n  </div>\n}";
    let result = instrument(source, "test.tsx", &InstrumentOptions::default()).unwrap();
    assert!(result.unhandled_pragmas.is_empty());
    assert_eq!(
        result.coverage_map.branch_map.len(),
        0,
        "JSX comment-style ignore next should skip the following JSX child"
    );
}

#[test]
fn pragma_ignore_next_prunes_empty_logical_expression_branch() {
    let source =
        "function f(a, b) {\n  return /* v8 ignore next */ a && /* v8 ignore next */ b;\n}";
    let result = instrument_js(source);
    assert!(result.unhandled_pragmas.is_empty());
    assert_eq!(
        result.coverage_map.branch_map.len(),
        0,
        "logical branches with no instrumented leaves should be pruned like Istanbul"
    );
    assert_eq!(result.coverage_map.b.len(), 0);
}

#[test]
fn pragma_ignore_next_skips_intervening_comments() {
    let cases = [
        "function f() {\n  // v8 ignore next -- @preserve\n  // @ts-ignore: unrelated\n  if (typeof globalThis !== 'undefined') { console.log('ok') }\n}",
        "function f() {\n  /* v8 ignore next -- @preserve */\n  /* unrelated comment */\n  if (typeof globalThis !== 'undefined') { console.log('ok') }\n}",
    ];

    for source in cases {
        let result = instrument_js(source);
        assert!(result.unhandled_pragmas.is_empty());
        assert_eq!(result.coverage_map.branch_map.len(), 0, "branch should be ignored:\n{source}");
        assert_eq!(
            result.coverage_map.statement_map.len(),
            0,
            "statement subtree should be ignored:\n{source}"
        );
    }
}

#[test]
fn pragma_ignore_next_skips_logical_expression_containers() {
    let cases = [
        "function f(child) {\n  return update(\n    /* v8 ignore next -- @preserve */\n    child.attributes || {},\n    {a: 1},\n  )\n}",
        "function f(h) {\n  return {\n    applicant: h.applicantTemplate.workflow.id,\n    /* v8 ignore next -- @preserve */\n    lender: h.lenderTemplate?.workflow.id ?? '',\n  }\n}",
    ];

    for source in cases {
        let result = instrument_js(source);
        assert!(result.unhandled_pragmas.is_empty());
        assert_eq!(
            result.coverage_map.branch_map.len(),
            0,
            "logical branch should be ignored:\n{source}"
        );
    }
}

#[test]
fn pragma_ignore_next_skips_ternary_expression_containers() {
    let cases = [
        "function f(getSimilarNodes, node) {\n  return getSimilarNodes(\n    'state',\n    /* v8 ignore next -- @preserve */\n    node.type === 'integration'\n      ? node.properties.name\n      : undefined,\n    'nodeId',\n  )\n}",
        "function f(items) {\n  return items\n    .sort((a, b) =>\n      /* v8 ignore next -- @preserve */ a.ranking < b.ranking ? -1 : 1,\n    )\n}",
    ];

    for source in cases {
        let result = instrument_js(source);
        assert!(result.unhandled_pragmas.is_empty());
        assert_eq!(
            result.coverage_map.branch_map.len(),
            0,
            "ternary branch should be ignored:\n{source}"
        );
    }
}

#[test]
fn pragma_ignore_next_skips_switch_case_branches() {
    let cases = [
        (
            "function f(item) {\n  switch (item.type) {\n    case 'html': return 'a'\n    /* v8 ignore next -- @preserve */\n    case 'link': return 'b'\n  }\n}",
            vec![2, 3],
        ),
        (
            "function f(item) {\n  switch (item.type) {\n    case 'html': return 'a'\n    /* v8 ignore start -- @preserve */\n    case 'link': return 'b'\n    /* v8 ignore stop -- @preserve */\n  }\n}",
            vec![2, 3],
        ),
        (
            "function f(item) {\n  switch (item.type) {\n    case 'html': return 'a'\n    case 'link':\n      /* v8 ignore next -- @preserve */\n      return 'b'\n  }\n}",
            vec![2, 3],
        ),
        (
            "function f(item) {\n  switch (item.type) {\n    case 'html': return 'a'\n    /* istanbul ignore next */\n    default: return 'b'\n  }\n}",
            vec![2, 3],
        ),
    ];

    for (source, expected_statement_lines) in cases {
        let result = instrument_js(source);
        assert!(result.unhandled_pragmas.is_empty());
        assert_eq!(
            result.coverage_map.branch_map.len(),
            1,
            "switch branch should remain:\n{source}"
        );
        assert_eq!(
            result.coverage_map.branch_map["0"].locations.len(),
            1,
            "ignored case should be pruned from switch branch locations:\n{source}"
        );
        let statement_lines: Vec<u32> =
            result.coverage_map.statement_map.values().map(|loc| loc.start.line).collect();
        assert_eq!(
            statement_lines, expected_statement_lines,
            "ignored case consequent statements should be pruned:\n{source}"
        );
    }
}

#[test]
fn pragma_whitespace_tolerance_matches_canonical() {
    // Istanbul accepts any ASCII whitespace between the tool name, `ignore` and
    // the kind keyword, so every variant behaves like the canonical
    // single-space form.
    let canonical = "function f() {\n  /* istanbul ignore next */\n  return 1;\n}\nf();";
    let reference = instrument_js(canonical);
    let ref_stmts = reference.coverage_map.statement_map.len();
    let ref_fns = reference.coverage_map.fn_map.len();
    assert!(reference.unhandled_pragmas.is_empty());

    let variants = [
        "function f() {\n  /* istanbul\tignore next */\n  return 1;\n}\nf();",
        "function f() {\n  /*   istanbul   ignore   next   */\n  return 1;\n}\nf();",
        "function f() {\n  /* istanbul\n     ignore\n     next */\n  return 1;\n}\nf();",
        "function f() {\n  /*\tistanbul\tignore\tnext\t*/\n  return 1;\n}\nf();",
    ];
    for src in variants {
        let r = instrument_js(src);
        assert_eq!(
            r.coverage_map.statement_map.len(),
            ref_stmts,
            "variant should match canonical pragma behavior:\n{src}"
        );
        assert_eq!(r.coverage_map.fn_map.len(), ref_fns);
        assert!(r.unhandled_pragmas.is_empty(), "variant should be recognized: {src}");
    }
}

#[test]
fn pragma_block_ignore_start_without_stop_extends_to_end_of_file() {
    // An `ignore start` with no matching `ignore stop` ignores the remainder of
    // the file, so no statement after the start comment is counted.
    let source = "function before() { return 1; }\n\
                  /* istanbul ignore start */\n\
                  function ignored_a() { return 2; }\n\
                  function ignored_b() { return 3; }\n";
    let result = instrument(source, "test.js", &InstrumentOptions::default()).unwrap();
    assert_eq!(
        result.coverage_map.statement_map.len(),
        1,
        "only `before`'s body statement should be counted"
    );
    let names: Vec<&str> = result.coverage_map.fn_map.values().map(|f| f.name.as_str()).collect();
    assert_eq!(names, vec!["before"], "fnMap should only include `before`");
}

#[test]
fn pragma_shaped_comments_without_a_tool_and_ignore_prefix_do_not_register() {
    // A comment needs a known tool word followed by `ignore` to count as a
    // pragma, known or unknown; anything else leaves the next statement counted.
    for comment in ["/* eslint ignore next */", "/* istanbul */", "/* istanbul something-else */"] {
        let result = instrument_js(&format!("{comment}\nconst x = 1;\n"));
        assert_eq!(result.coverage_map.statement_map.len(), 1, "{comment}");
        assert!(result.unhandled_pragmas.is_empty(), "{comment}");
    }
}

#[test]
fn pragma_attaches_to_token_after_intervening_block_comment() {
    // The pragma is followed by an unrelated /* ... */ block, then a
    // statement. The next-token scanner must skip past the closing `*/` and
    // attach the pragma to the statement that follows.
    let source = "/* istanbul ignore next */ /* unrelated */ const x = 1;\nconst y = 2;\n";
    let result = instrument(source, "test.js", &InstrumentOptions::default()).unwrap();
    let lines: Vec<u32> =
        result.coverage_map.statement_map.values().map(|loc| loc.start.line).collect();
    assert_eq!(lines, vec![2], "only line 2 (`y`) should be counted, got {lines:?}");
}

#[test]
fn pragma_attaches_to_token_after_intervening_line_comment() {
    // A `//` line comment between the pragma and its target: the scanner has
    // to skip past the newline and attach to the statement that follows.
    let source = "/* istanbul ignore next */ // throwaway line\nconst x = 1;\nconst y = 2;\n";
    let result = instrument(source, "test.js", &InstrumentOptions::default()).unwrap();
    let lines: Vec<u32> =
        result.coverage_map.statement_map.values().map(|loc| loc.start.line).collect();
    assert_eq!(lines, vec![3], "only line 3 (`y`) should be counted, got {lines:?}");
}

#[test]
fn unhandled_pragma_on_later_line_uses_correct_line_number() {
    // The other unknown-pragma cases all sit on line 1, so this one is placed
    // on line 3 to exercise the line/column lookup past the first line start.
    let source = "const a = 1;\nconst b = 2;\n/* istanbul ignore bogus */ const c = 3;\n";
    let result = instrument(source, "test.js", &InstrumentOptions::default()).unwrap();
    assert_eq!(result.unhandled_pragmas.len(), 1);
    let pragma = &result.unhandled_pragmas[0];
    assert_eq!(pragma.line, 3, "pragma should be reported on line 3, got {pragma:?}");
    assert_eq!(pragma.column, 0, "pragma column should be 0 (start of line 3), got {pragma:?}");
}

#[test]
fn pragma_diagnostics_follow_ecmascript_line_terminators() {
    for (name, terminator) in ECMASCRIPT_LINE_TERMINATORS {
        let source = format!("first();{terminator}/* istanbul ignore bogus */ second();");
        let result = instrument(&source, "test.js", &InstrumentOptions::default()).unwrap();
        let pragma = &result.unhandled_pragmas[0];
        assert_eq!((pragma.line, pragma.column), (2, 0), "{name}");
    }
}

#[test]
fn unknown_pragma_populates_unhandled_pragmas() {
    let result = instrument_js("/* istanbul ignore banana */\nfunction f() { return 1; }");
    assert!(!result.unhandled_pragmas.is_empty());
    assert!(result.unhandled_pragmas[0].comment.contains("banana"));
    assert_eq!(result.unhandled_pragmas[0].line, 1);
}

/// The `(functions, branch location counts, statements)` of `source`, in the
/// order the coverage map holds them.
fn coverage_shape(source: &str) -> (usize, Vec<usize>, usize) {
    let map = instrument_js(source).coverage_map;
    let branches = map.branch_map.values().map(|entry| entry.locations.len()).collect();
    (map.fn_map.len(), branches, map.statement_map.len())
}

#[test]
fn pragma_before_parentheses_ignores_the_wrapped_function() {
    for source in [
        "x = /* istanbul ignore next */ (function () {});",
        "x = /* istanbul ignore next */ (() => 1);",
        "f(/* istanbul ignore next */ (() => 1));",
        "x = [/* istanbul ignore next */ ((function () {}))];",
        "x = (/* istanbul ignore next */ (function () {}));",
    ] {
        assert_eq!(coverage_shape(source), (0, vec![], 1), "{source}");
    }
}

#[test]
fn pragma_before_parentheses_owned_by_a_call_keeps_the_function() {
    for source in [
        "x = /* istanbul ignore next */ (function () {})();",
        "x = /* istanbul ignore next */ (function () {}).call(this);",
        "x = /* istanbul ignore next */ (a, function () {});",
    ] {
        assert_eq!(coverage_shape(source).0, 1, "{source}");
    }
}

#[test]
fn pragma_before_parenthesized_logical_operand_drops_the_operand() {
    assert_eq!(coverage_shape("x = a || /* istanbul ignore next */ (b);").1, [1]);
    assert_eq!(coverage_shape("x = a || /* istanbul ignore next */ (b && c) || d;").1, [2]);
    assert_eq!(coverage_shape("x = /* istanbul ignore next */ (a || b);").1, Vec::<usize>::new());
    assert_eq!(coverage_shape("f(/* istanbul ignore next */ ((a || b)));").1, Vec::<usize>::new());
}

#[test]
fn pragma_on_parenthesized_conditional_arm_drops_the_arm() {
    assert_eq!(coverage_shape("x = a ? (/* istanbul ignore next */ b) : c;").1, [1]);
    assert_eq!(coverage_shape("x = a ? b : (/* istanbul ignore next */ c);").1, [1]);
    assert_eq!(
        coverage_shape("x = a ? /* istanbul ignore next */ (function () {}) : c;"),
        (0, vec![1], 1)
    );
    assert_eq!(
        coverage_shape("x = /* istanbul ignore next */ (a ? b : c);").1,
        Vec::<usize>::new()
    );
}

#[test]
fn ignored_conditional_and_logical_expressions_ignore_their_subtree() {
    for source in [
        "x = /* istanbul ignore next */ a || (b ? c : d);",
        "x = /* istanbul ignore next */ a || function () {};",
        "x = /* istanbul ignore next */ a ? (b || c) : d;",
        "x = /* istanbul ignore next */ (a ? b : c) ? d : e;",
        "x = /* istanbul ignore next */ (a) ? function () {} : d;",
    ] {
        assert_eq!(coverage_shape(source), (0, vec![], 1), "{source}");
    }
}

#[test]
fn pragma_in_concise_arrow_body_keeps_the_body_statement() {
    for source in [
        "g = () => /* istanbul ignore next */ a;",
        "g = () => /* istanbul ignore next */ (a);",
        "g = () => /* istanbul ignore next */ (a ? b : c);",
        "g = () => /* istanbul ignore next */ a ? b : c;",
    ] {
        assert_eq!(coverage_shape(source), (1, vec![], 2), "{source}");
    }
    assert_eq!(coverage_shape("g = () => /* istanbul ignore next */ (() => 1);"), (1, vec![], 2));
}

#[test]
fn pragma_before_a_rewrapped_value_does_not_ignore_it() {
    // The variable initializer, the class field value and the default value
    // each get a counter wrapped around them, which moves the comment off the
    // value. Only a hoisted function initializer keeps it.
    assert_eq!(coverage_shape("var y = /* istanbul ignore next */ a || b;").1, [2]);
    assert_eq!(coverage_shape("var y = /* istanbul ignore next */ (a ? b : c);").1, [2]);
    assert_eq!(coverage_shape("var y = /* istanbul ignore next */ (function () {});").0, 0);
    assert_eq!(
        coverage_shape("for (var y = /* istanbul ignore next */ function () {};;) break;").0,
        1
    );
    assert_eq!(coverage_shape("class A { m = /* istanbul ignore next */ () => 1; }").0, 1);
    assert_eq!(coverage_shape("class A { m = /* istanbul ignore next */ (a || b); }").1, [2]);
    assert_eq!(coverage_shape("function f(p = /* istanbul ignore next */ () => 1) {}").0, 2);
    assert_eq!(coverage_shape("function f(p = /* istanbul ignore next */ (b || c)) {}").1, [1, 2]);
    assert_eq!(coverage_shape("var { q = /* istanbul ignore next */ function () {} } = o;").0, 1);
}

#[test]
fn pragma_owned_by_a_call_or_sequence_keeps_the_function_at_the_same_start() {
    for source in [
        "x = /* istanbul ignore next */ function () {}.call(this);",
        "x = (/* istanbul ignore next */ function () {}, b);",
        "x = /* istanbul ignore next */ (function () {}, b);",
    ] {
        assert_eq!(coverage_shape(source).0, 1, "{source}");
    }
    // The logical expression owns the pragma here, so its subtree is ignored.
    assert_eq!(
        coverage_shape("x = /* istanbul ignore next */ function () {}.x || b;"),
        (0, vec![], 1)
    );
}

#[test]
fn pragma_before_an_assignment_target_default_does_not_ignore_it() {
    assert_eq!(coverage_shape("({ a = /* istanbul ignore next */ (() => 1) } = o);").0, 1);
    assert_eq!(coverage_shape("[a = /* istanbul ignore next */ (b || c)] = o;").1, [1, 2]);
}
