//! Tests for the experimental `timing` option: the body wrap, the functions it
//! leaves out, and the runtime data in `__fallow_timing__` for sync, async,
//! concise arrow, and throwing functions.

use std::num::NonZeroU32;
use std::process::Command;

use oxc_coverage_instrument::{InstrumentOptions, TimingOptions, instrument};
use serde_json::Value;

use crate::common::assert_reparses;

fn timing(sample_every: u32) -> InstrumentOptions {
    InstrumentOptions::default().with_timing(Some(TimingOptions {
        sample_every: NonZeroU32::new(sample_every).expect("non-zero"),
    }))
}

fn instrumented(source: &str, options: &InstrumentOptions) -> String {
    let result = instrument(source, "timing.js", options).expect("instrument");
    assert_reparses(&result.code, "timing.js");
    result.code
}

/// Run `code` followed by `driver` in Node and parse the JSON it prints.
fn run_node(code: &str, driver: &str) -> Value {
    let script = format!(
        "{code}\n;(async () => {{ {driver} }})().catch((error) => {{ console.error(error); process.exit(1); }});"
    );
    let output = Command::new("node")
        .arg("--input-type=commonjs")
        .arg("--eval")
        .arg(script)
        .output()
        .expect("node must be available");
    assert!(output.status.success(), "node failed: {}", String::from_utf8_lossy(&output.stderr));
    serde_json::from_slice(&output.stdout).expect("driver prints JSON")
}

/// The `fnMap` id of the function named `name`.
fn fn_id(source: &str, options: &InstrumentOptions, name: &str) -> usize {
    let result = instrument(source, "timing.js", options).expect("instrument");
    let id = result
        .coverage_map
        .fn_map
        .iter()
        .find(|(_, entry)| entry.name == name)
        .map_or_else(|| panic!("no fnMap entry named {name}"), |(id, _)| id.clone());
    id.parse().expect("numeric fnMap id")
}

const PRINT: &str =
    "const e = globalThis.__fallow_timing__['timing.js']; console.log(JSON.stringify(e));";

#[test]
fn off_by_default() {
    let code = instrumented("function f() { return 1; }", &InstrumentOptions::default());
    assert!(!code.contains("finally"), "{code}");
    assert!(!code.contains("__fallow_timing__"), "{code}");
}

#[test]
fn wraps_a_function_body_in_try_finally() {
    let code = instrumented("function f() { return 1; }", &timing(1));
    assert!(code.contains("try {"), "{code}");
    assert!(code.contains("} finally {"), "{code}");
    assert!(code.contains("__fallow_timing__"), "{code}");
}

#[test]
fn leaves_generators_out() {
    let source = "function* g() { yield 1; }\nasync function* h() { yield 2; }";
    let code = instrumented(source, &timing(1));
    assert!(!code.contains("finally"), "{code}");
}

#[test]
fn leaves_ignored_functions_out() {
    let source = "/* istanbul ignore next */\nfunction f() { return 1; }";
    let code = instrumented(source, &timing(1));
    assert!(!code.contains("finally"), "{code}");
}

#[test]
fn records_sync_calls_and_keeps_return_values() {
    let source = "function add(a, b) { return a + b; }";
    let options = timing(1);
    let id = fn_id(source, &options, "add");
    let data = run_node(
        &instrumented(source, &options),
        &format!(
            "let sum = 0; for (let i = 0; i < 10; i++) sum = add(sum, i); if (sum !== 45) throw new Error('sum ' + sum); {PRINT}"
        ),
    );
    assert_eq!(data["sampleEvery"], 1);
    assert_eq!(data["c"][id], 10);
    let buckets: u64 = data["b"][id].as_array().unwrap().iter().map(|b| b.as_u64().unwrap()).sum();
    assert_eq!(buckets, 10, "one bucket entry per sampled call: {data}");
}

#[test]
fn measures_an_async_function_until_it_settles() {
    let source =
        "async function wait(ms) { await new Promise((r) => setTimeout(r, ms)); return ms; }";
    let options = timing(1);
    let id = fn_id(source, &options, "wait");
    let data = run_node(
        &instrumented(source, &options),
        &format!("const v = await wait(30); if (v !== 30) throw new Error('value ' + v); {PRINT}"),
    );
    assert_eq!(data["c"][id], 1);
    let sum = data["s"][id].as_f64().unwrap();
    assert!(sum >= 25.0, "async time includes the await, got {sum} ms");
}

#[test]
fn measures_a_concise_async_arrow_until_it_settles() {
    let source = "const sleep = (ms) => new Promise((r) => setTimeout(r, ms));\nconst nap = async () => sleep(20).then(() => 7);";
    let options = timing(1);
    let id = fn_id(source, &options, "nap");
    let data = run_node(
        &instrumented(source, &options),
        &format!("const v = await nap(); if (v !== 7) throw new Error('value ' + v); {PRINT}"),
    );
    assert_eq!(data["c"][id], 1);
    assert!(data["s"][id].as_f64().unwrap() >= 15.0, "{data}");
}

#[test]
fn concise_sync_arrow_keeps_its_value() {
    let source = "const double = (x) => x * 2;";
    let options = timing(1);
    let id = fn_id(source, &options, "double");
    let data = run_node(
        &instrumented(source, &options),
        &format!("if (double(21) !== 42) throw new Error('value'); {PRINT}"),
    );
    assert_eq!(data["c"][id], 1);
}

#[test]
fn records_a_call_that_throws_and_rethrows_the_error() {
    let source = "function boom() { throw new TypeError('bad'); }\nasync function reject() { await null; throw new RangeError('late'); }";
    let options = timing(1);
    let boom = fn_id(source, &options, "boom");
    let reject = fn_id(source, &options, "reject");
    let data = run_node(
        &instrumented(source, &options),
        &format!(
            "try {{ boom(); throw new Error('no throw'); }} catch (e) {{ if (!(e instanceof TypeError)) throw e; }}
             try {{ await reject(); throw new Error('no reject'); }} catch (e) {{ if (!(e instanceof RangeError)) throw e; }}
             {PRINT}"
        ),
    );
    assert_eq!(data["c"][boom], 1);
    assert_eq!(data["c"][reject], 1);
}

#[test]
fn samples_one_call_in_n_per_function() {
    let source = "function tick() { return 1; }";
    let options = timing(10);
    let id = fn_id(source, &options, "tick");
    let data = run_node(
        &instrumented(source, &options),
        &format!("for (let i = 0; i < 1000; i++) tick(); {PRINT}"),
    );
    assert_eq!(data["c"][id], 100, "countdown sampling takes exactly one call in ten: {data}");
}

#[test]
fn keeps_this_arguments_and_recursion() {
    let source = "class C { constructor(n) { this.n = n; } fact(k) { return k <= 1 ? 1 : k * this.fact(k - 1); } }\nfunction count() { return arguments.length; }";
    let options = timing(1);
    let data = run_node(
        &instrumented(source, &options),
        &format!(
            "if (new C(1).fact(5) !== 120) throw new Error('fact'); if (count(1, 2, 3) !== 3) throw new Error('arguments'); {PRINT}"
        ),
    );
    let total: u64 = data["c"].as_array().unwrap().iter().map(|c| c.as_u64().unwrap()).sum();
    assert_eq!(total, 1 + 5 + 1, "constructor, five fact calls, one count call: {data}");
}

#[test]
fn async_return_of_a_promise_counts_once_and_keeps_its_value_when_unsampled() {
    let source = "const later = (v) => new Promise((r) => setTimeout(() => r(v), 1));\nasync function pass(v) { if (v < 0) { return later(-v); } return later(v); }";
    let options = timing(3);
    let id = fn_id(source, &options, "pass");
    let data = run_node(
        &instrumented(source, &options),
        &format!(
            "let total = 0; for (let i = 1; i <= 9; i++) total += await pass(i % 2 ? i : -i); if (total !== 45) throw new Error('total ' + total); {PRINT}"
        ),
    );
    assert_eq!(data["c"][id], 3, "one call in three, recorded once each: {data}");
    let buckets: u64 = data["b"][id].as_array().unwrap().iter().map(|b| b.as_u64().unwrap()).sum();
    assert_eq!(buckets, 3, "{data}");
}

#[test]
fn async_function_returning_a_rejected_promise_rejects_and_is_recorded() {
    let source = "const fail = () => new Promise((_, j) => setTimeout(() => j(new SyntaxError('x')), 15));\nasync function relay() { return fail(); }";
    let options = timing(1);
    let id = fn_id(source, &options, "relay");
    let data = run_node(
        &instrumented(source, &options),
        &format!(
            "try {{ await relay(); throw new Error('no reject'); }} catch (e) {{ if (!(e instanceof SyntaxError)) throw e; }} {PRINT}"
        ),
    );
    assert_eq!(data["c"][id], 1);
    assert!(data["s"][id].as_f64().unwrap() >= 10.0, "{data}");
}

#[test]
fn keeps_derived_constructors_and_hoisted_declarations() {
    let source = "class A { constructor(v) { this.v = v; } }\nclass B extends A { constructor() { super(hoisted()); function hoisted() { return 3; } } }";
    let options = timing(1);
    let data = run_node(
        &instrumented(source, &options),
        &format!("if (new B().v !== 3) throw new Error('value'); {PRINT}"),
    );
    let total: u64 = data["c"].as_array().unwrap().iter().map(|c| c.as_u64().unwrap()).sum();
    assert_eq!(total, 3, "two constructors and one hoisted function: {data}");
}
