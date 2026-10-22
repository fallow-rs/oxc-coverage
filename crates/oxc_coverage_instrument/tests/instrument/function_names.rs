//! Default-profile `fnMap` names inferred from a binding, a key or a target.
//!
//! A name goes only to the function or arrow that is the direct value, also
//! through parentheses. It never goes to a function in a decorator, a computed
//! key, a member object, a class body or a later statement. A named function
//! expression keeps its own name, as `Function.prototype.name` does.

use oxc_coverage_instrument::{InstrumentOptions, instrument};

/// The `fnMap` names in id order.
fn names(source: &str) -> Vec<String> {
    let result = instrument(source, "names.js", &InstrumentOptions::default()).expect("instrument");
    let fn_map = &result.coverage_map.fn_map;
    (0..fn_map.len()).map(|id| fn_map[&id.to_string()].name.clone()).collect()
}

#[test]
fn decorator_function_does_not_take_the_method_name() {
    assert_eq!(names("class C {\n  @dec(function () {})\n  m() {}\n}"), ["m", "(anonymous_1)"]);
}

#[test]
fn decorator_arrow_does_not_take_the_method_name() {
    assert_eq!(names("class C {\n  @dec(() => 1)\n  m() {}\n}"), ["m", "(anonymous_1)"]);
}

#[test]
fn decorator_arrow_does_not_take_the_static_method_name() {
    assert_eq!(names("class C { @dec(() => 1) static m() {} }"), ["m", "(anonymous_1)"]);
}

#[test]
fn parameter_default_class_does_not_name_a_computed_method() {
    assert_eq!(names("function f(cb = class { [k]() {} }) {}"), ["f", "(anonymous_1)"]);
}

#[test]
fn parameter_default_class_does_not_name_a_body_arrow() {
    assert_eq!(names("function f(c = class {}) { return () => 1 }"), ["f", "(anonymous_1)"]);
}

#[test]
fn parameter_default_class_does_not_name_a_later_arrow() {
    assert_eq!(names("function f(c = class {}) {}\n[1].map(() => 1)"), ["f", "(anonymous_1)"]);
}

#[test]
fn destructuring_default_class_does_not_name_a_computed_method() {
    assert_eq!(names("const { cb = class { [k]() {} } } = o"), ["(anonymous_0)"]);
}

#[test]
fn assignment_class_does_not_name_its_members_or_later_arrows() {
    assert_eq!(names("o.x = class { [k]() {} }"), ["(anonymous_0)"]);
    assert_eq!(names("o.x = class { y = () => 1 }"), ["(anonymous_0)"]);
    assert_eq!(names("o.x = class {}; [1].map(() => 1)"), ["(anonymous_0)"]);
    assert_eq!(names("o.x = class {}; o.y ||= () => 1"), ["(anonymous_0)"]);
}

#[test]
fn assignment_name_skips_a_function_in_the_member_object() {
    assert_eq!(names("o[g(() => 1)].x = function () {}"), ["(anonymous_0)", "x"]);
}

#[test]
fn parenthesized_values_take_the_binding_name() {
    assert_eq!(names("const f = (function () {})"), ["f"]);
    assert_eq!(names("const f = (() => 1)"), ["f"]);
    assert_eq!(names("function f(cb = (() => 1)) {}"), ["f", "cb"]);
    assert_eq!(names("o.x = (function () {})"), ["x"]);
    assert_eq!(names("o.x = (() => 1)"), ["x"]);
    assert_eq!(names("export default (function () {})"), ["default"]);
}

#[test]
fn named_function_expression_keeps_its_own_name() {
    assert_eq!(names("const f = function g() {}"), ["g"]);
    assert_eq!(names("const o = { a: function b() {} }"), ["b"]);
    assert_eq!(names("o.x = function y() {}"), ["y"]);
    assert_eq!(names("function f(cb = function g() {}) {}"), ["f", "g"]);
}

#[test]
fn identifier_assignment_names_the_function() {
    assert_eq!(names("x = function () {}"), ["x"]);
    assert_eq!(names("let x; x = () => 1"), ["x"]);
}

#[test]
fn assignment_destructuring_default_names_the_function() {
    assert_eq!(names("({ a = () => 1 } = o)"), ["a"]);
    assert_eq!(names("({ b: a = () => 1 } = o)"), ["a"]);
    assert_eq!(names("[a = () => 1] = []"), ["a"]);
}

#[test]
fn existing_inferred_names_stay() {
    assert_eq!(names("const f = function () {}"), ["f"]);
    assert_eq!(names("const f = () => () => 1"), ["f", "(anonymous_1)"]);
    assert_eq!(names("function f(cb = () => 1) {}"), ["f", "cb"]);
    assert_eq!(names("function f(cb = g(() => 1)) {}"), ["f", "(anonymous_1)"]);
    assert_eq!(names("const { cb = () => 1 } = o"), ["cb"]);
    assert_eq!(names("const [a = () => 1] = []"), ["a"]);
    assert_eq!(names("o.x = function () {}"), ["x"]);
    assert_eq!(names("o['y'] = () => 1"), ["y"]);
    assert_eq!(names("export default () => 1"), ["default"]);
    assert_eq!(names("export default function () {}"), ["default"]);
    assert_eq!(names("export default function named() {}"), ["named"]);
    assert_eq!(names("({ m() {}, n: () => 1, get p() { return 1 } })"), ["m", "n", "get p"]);
    assert_eq!(names("({ a: (function () {}) })"), ["a"]);
    assert_eq!(names("class C { m(a = () => 1) {} }"), ["m", "a"]);
    assert_eq!(names("o.x = function (a = () => 1) {}"), ["x", "a"]);
    assert_eq!(names("class C { @dec(() => 1) x = 1; m() {} }"), ["(anonymous_0)", "m"]);
    assert_eq!(names("const o = { a: dec(() => 1) }"), ["(anonymous_0)"]);
    assert_eq!(names("const f = g(() => 1)"), ["(anonymous_0)"]);
}
