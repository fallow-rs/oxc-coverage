# oxc_coverage_instrument

Istanbul-compatible JavaScript and TypeScript coverage instrumentation using the
Oxc AST.

> **Unofficial.** Independent community project built on Oxc crates. Not part
> of, affiliated with, or endorsed by the Oxc project.

## Overview

`instrument` parses a source file with `oxc_parser`, identifies every statement,
function, and branch, injects the counter expressions Istanbul reporters expect,
and returns the instrumented code together with the coverage map. The map
serializes to Istanbul's `coverage-final.json` shape, so Jest, Vitest, c8, nyc,
and Codecov consume it without a translation layer.

`swc-plugin-coverage-instrument` fills this role for SWC. Without an Oxc
equivalent, a tool built on `oxc_parser` that needs coverage instrumentation has
to pull in SWC or Babel.

## Key Features

| Dimension | Constructs |
|:----------|:-----------|
| Statements | Every executable statement |
| Functions | Declarations, expressions, arrows, class methods |
| Branches | `if`/`else`, ternary, `switch`, `&&`/`\|\|`/`??`, `??=`/`\|\|=`/`&&=`, default arguments, optional-chain links |
| Pragmas | `istanbul`, `v8`, and `c8` `ignore next/if/else/file/start/stop` |

On top of instrumentation, the crate re-exports the rest of the suite:
`remap_coverage` and friends from `oxc_coverage_source_maps`, `v8_to_istanbul`
from the V8 converter, and `FileCoverage`, `Location`, `Position`, and
`parse_coverage_map` from `oxc_coverage_types`. A consumer on the default path
therefore needs one dependency.

## Architecture

Instrumentation is a single `oxc_traverse` pass over the parsed AST, run after
`SemanticBuilder` has produced the scope tree. The visitor assigns ids, records
each `Location` in the coverage map, and injects the counter expression; codegen
then emits the rewritten AST and a source map from the instrumented output back
to the input. When an `inputSourceMap` is supplied it is composed with the
codegen map, so downstream remappers resolve positions to the original source.
The suite-level picture is in
[ARCHITECTURE.md](https://github.com/fallow-rs/oxc-coverage/blob/main/ARCHITECTURE.md).

## Usage

```rust
use oxc_coverage_instrument::{instrument, InstrumentOptions};

let source = "function add(a, b) { return a + b; }";
let result = instrument(source, "add.js", &InstrumentOptions::default()).unwrap();

assert_eq!(result.coverage_map.fn_map["0"].name, "add");
println!("{}", result.code);
```

Existing coverage data parses back into the same model:

```rust
use oxc_coverage_instrument::parse_coverage_map;

let json = std::fs::read_to_string("coverage-final.json").unwrap();
let map = parse_coverage_map(&json).unwrap();

for (path, coverage) in &map {
    println!("{}: {} statements, {} functions, {} branches",
        path, coverage.s.len(), coverage.f.len(), coverage.b.len());
}
```

### Experimental AST API

Enable the `ast-api` crate feature to instrument a `Program` and `Scoping`
already owned by an Oxc host. `instrument_program` injects counters into that
AST and returns the updated scoping, coverage map, serialized map, unhandled
pragmas, and setup preamble. It does not parse or generate code.

The host owns the surrounding pipeline. TypeScript and JSX must already be in
the intended transform state, and parser or output options do not run those
phases inside this entry point. The returned preamble is source metadata, not an
AST fragment. Insert or emit it after the hashbang and directive prologue, then
rebuild semantic state so its declaration is connected to the injected counter
references.

```toml
[dependencies]
oxc_coverage_instrument = { version = "0.12", features = ["ast-api"] }
```

The source-to-source `instrument` function uses the same internal AST pass and
continues to own parsing, optional TypeScript lowering, preamble insertion,
codegen, and output source-map generation. It inserts the emitted setup after
the hashbang and directive prologue, then shifts only the following generated
source-map lines. The setup does not require another parse or semantic pass.

### Composing the input source map eagerly

The default flow is lazy: `instrument()` embeds the `inputSourceMap` and a
downstream remap walks every entry back at report time. Collectors that dump
`window.__coverage__` directly pay that round trip once per collected file. Set
`compose_input_source_map: true` alongside `input_source_map` to fold the map in
during instrumentation instead. The resulting coverage map is keyed by the
original source path, carries original-source positions, and has no
`inputSourceMap`. The runtime `__coverage__` baked into the emitted code is keyed
the same way, so a later remap is a no-op.

An entry drops exactly when the lazy path with `RemapOptions { drop_unmapped:
true }` would drop it: both resolve each span through the same `getMapping`
lookup, so a statement whose generated column sits just before its line's first
mapping is kept by both. Dropping is unconditional here, because the eager path
bakes positions into the runtime `__coverage__` literal with no later remap
opportunity, and an unmapped entry would otherwise be stranded at a generated
coordinate past the end of the original file.

The composed result therefore keeps the same surviving entries at the same
original-source positions as instrument-without-compose followed by
`remap_coverage_with_options(.., RemapOptions { drop_unmapped: true })`, and the
two are byte-identical when nothing drops. When entries do drop, eager
composition and the map-level remap APIs renumber the surviving ids contiguously,
while the single-file `remap_coverage_with_options` preserves original ids
including gaps. Istanbul treats the two shapes as equivalent, because it merges
entries by location.

When the input map is unusable (no declared source, or it fails to parse),
composition backs off and the `inputSourceMap` stays embedded so the lazy path
still works. The flag has no effect when `input_source_map` is unset.

### Function identity overlay

Set `function_identity_overlay: true` to attach an `x_fallow_functionMap` to the
coverage map. The overlay carries a `fallow:fn:<8 hex>` identity per function,
keyed by the same ids as `fnMap`, computed as
`SHA-256(path + name + decl.start.line + "function")` truncated to the first 4
bytes. That is bit-equal to `fallow_cov_protocol::function_identity_id`, so
consumers can join the overlay against V8 dumps, Istanbul ingesters, and
source-mapped findings without recomputing.

Renaming a function or moving it to another line changes the id; column-level
edits on the same line do not. Columns survive on the overlay's `decl` and `loc`
fields for display and same-line disambiguation, but are excluded from the hash
so producers observing the same function at different positional fidelity agree
on the id.

This is not part of Istanbul. Standard consumers ignore `x_`-prefixed fields, so
with the option off the output stays byte-identical to what nyc, Vitest, Jest,
and Codecov expect. When an `inputSourceMap` is consumed the overlay still
references pre-remap positions, so a consumer that remaps downstream must
recompute identity against the post-remap positions. The remap pipeline does not
rewrite the overlay.

The path enters the hash verbatim from the `filename` argument. `./app.js`,
`app.js`, and `/abs/repo/app.js` all hash differently, so callers that need
stable ids across tools must normalize paths before instrumentation. Pick one
canonical form per project, typically a workspace-root-relative POSIX path.

## Istanbul conformance

Output is checked against `istanbul-lib-instrument` on a shared fixture corpus
covering every branch type, function form, Unicode columns, pragma boundaries,
hashbangs, directive prologues, binding collisions, class fields, stripped
TypeScript, and edge cases. The corpus lives in `tests/conformance/`. The suite
asserts that statement, function, and branch counts match exactly, that branch
types and per-branch location counts match, that the JSON field set matches, and
that the instrumented output re-parses as valid JavaScript.

CI also runs a blocking byte-for-byte diff over the same corpus under the strict
Istanbul profile, without divergence filters. That catches span-level and
counter-shape drift which count-only tests miss.

All `start.column` and `end.column` values in `statementMap`, `fnMap`,
`branchMap`, and `unhandledPragmas` are UTF-16 code units (JavaScript string
indices), matching Babel and `istanbul-lib-instrument`. Sources containing
non-ASCII characters (`π`, accented identifiers, emoji) produce the same column
numbers as the reference tool, pinned by the `26-non-ascii-identifiers.js`
fixture.

### Strict Istanbul compatibility profile

Set `compat: Some(CompatProfile::Istanbul)` in Rust or `compat: 'istanbul'` in
Node.js when the coverage shape must match `istanbul-lib-instrument` exactly.
The profile disables logical-assignment and optional-chain branches, uses
`(anonymous_N)` for inferred function names, truncates class and object method
declaration spans to the first character of the method, and emits Istanbul's
empty synthetic `else` locations. Explicitly named functions keep their names.

The profile reads only the `istanbul ignore next`, `if`, `else`, and `file`
pragmas, and reports the `v8`, `c8`, `start`, `stop`, and `/*!` forms in
`unhandledPragmas`. Each pragma applies to the node that Babel attaches it to
(see difference 8).

The profile is authoritative over the individual extension options. In
particular, optional-chain tracking and callback-argument name inference remain
off under the profile. With no profile, all existing Oxc defaults and the
extensions below remain unchanged.

## Differences from istanbul-lib-instrument

### 1. Logical-assignment operators are instrumented as branches

`x ??= y`, `x ||= y`, and `x &&= y` each contain a short-circuit conditional:
the right-hand side is evaluated, and the assignment happens, only when the left
operand matches the operator's polarity. This instrumenter emits one
`binary-expr` branch entry per logical assignment with two locations, left
always reached and right conditional. `istanbul-lib-instrument` has no
`AssignmentExpression` visitor entry and emits no branches for these operators.

The strict Istanbul profile disables these entries.

Pinned by `conformance.rs::logical_assignment_is_intentional_branch_superset`.

A codebase that uses `??=`, `||=`, or `&&=` heavily will see a higher branch
denominator, and so a slightly lower branch percentage, after switching from
`@vitest/coverage-istanbul`. To rebaseline CI thresholds:

```bash
vitest run --coverage --coverage.reporter=json-summary
jq '.total.branches.pct' coverage/coverage-summary.json
```

### 2. Inferred function names over `(anonymous_N)`

For an anonymous function expression assigned to a binding or declared as a
class method, this instrumenter uses the name the JavaScript runtime assigns to
`Function.prototype.name`:

| Source | `fnMap[].name` here | istanbul |
|---|---|---|
| `const f = function() {}` | `f` | `(anonymous_0)` |
| `const g = () => 1` | `g` | `(anonymous_0)` |
| `x = () => 1` | `x` | `(anonymous_0)` |
| `o.x = (function() {})` | `x` | `(anonymous_0)` |
| `const f = function g() {}` | `g` | `g` |
| `class C { bar() {} }` | `bar` | `(anonymous_0)` |
| `(function() {})()` | `(anonymous_0)` | `(anonymous_0)` |

Pinned by `conformance.rs::fn_name_inference_is_intentional_superset`.

The strict Istanbul profile uses `(anonymous_N)` instead.

### 3. Full method-key spans in `fnMap[*].decl`

For class and object methods with a non-computed key, the whole key is the
declaration span. `istanbul-lib-instrument` marks only the first character of
the method, which can be a decorator or a modifier, so
`class C { static bar() {} }` gives `bar` here and `s` there. A computed key
keeps the one-character marker. The byte-diff check accepts a key span only
between that marker and the body. It still pins the line, the body `loc`, and
every non-method declaration span.

The strict Istanbul profile uses the truncated span.

### 4. Real coordinates for synthetic `else` arms

For an `if` with no `else`, `istanbul-lib-instrument` records the synthetic
alternate slot as `{ start: {}, end: {} }`. This instrumenter anchors it as a
real zero-width `Location` at the consequent's end. Reporters that read
`loc.start.line` on every arm crash on the empty form; real coordinates make the
slot safe to walk without special-casing. The same applies to the surviving arm
when `/* istanbul ignore if */` drops the consequent of a no-else `if`.

The strict Istanbul profile emits the empty Istanbul location.

### 5. Optional-chain short-circuits tracked as branches

Receiver-safe `?.` links appear in `branchMap` as `optional-chain` entries with
two arms: arm 0 when the observed value is `null` or `undefined` and the link
short-circuits, arm 1 when it continues. Receiver-bound optional calls such as
`object.method?.()` stay native so instrumentation preserves their `this`
binding. `istanbul-lib-instrument` does not track optional chains. Reporters
that walk `branchMap` by shape pick the entries up automatically; reporters
that hard-code the Istanbul type names need to learn the label.

Set `track_optional_chain: false` to opt out. Optional chains are then left
native, with no `_oc` helper and no `optional-chain` branches, which matches
`istanbul-lib-instrument` byte for byte on `?.` and removes the per-operand
helper call in optional-chain-dense code. Statement, function, and other branch
coverage are unaffected. Defaults to `true`.

The strict Istanbul profile always leaves optional chains native.

### 6. Callback-argument names from the callee (off by default)

Section 2 recovers names from a binding. A function passed directly as a call or
`new` argument has no binding, so both instrumenters fall back to
`(anonymous_N)`. In callback-heavy code (route handlers, `.map`, `.filter`,
promise `.then`, `describe`, `it`, `new Promise`) that fallback dominates the
`fnMap`.

Set `name_callback_arguments: true` to name these from the callee:

| Source | with the option | default |
|---|---|---|
| `arr.map((x) => x)` | `map` | `(anonymous_0)` |
| `el.addEventListener("click", () => {})` | `addEventListener` | `(anonymous_0)` |
| `new Promise((resolve) => {})` | `Promise` | `(anonymous_0)` |
| `(function () {})()` | `(anonymous_0)` | `(anonymous_0)` |

Only the callee is used, never a sibling string argument such as a route path or
a test description: the traversal ancestor for an argument position exposes the
callee but not the other arguments. A binding name and an explicit named
function expression both take precedence; this only replaces the
`(anonymous_N)` fallback. Because the name comes from the callee rather than a
running counter, it is stable across rebuilds, where the `(anonymous_N)` index
renumbers whenever an unrelated function is added. Defaults to `false`, so
default output stays byte-identical to what Istanbul consumers expect.

### 7. Anonymous class-field values lose inferred runtime names

Class-field initializer counters use Istanbul-style sequence wrapping:
`field = (++cov.s[N], function () {})`. The wrapper keeps the counter inside
the original field and therefore does not add enumerable properties or change
`Object.keys()` output. It also prevents JavaScript NamedEvaluation from
inferring `"field"` as the anonymous function, arrow, or class value's runtime
`name`; those values keep an empty name after instrumentation. Explicitly named
function and class expressions keep their declared names.

Pinned by `integration.rs::sequence_wrapped_class_field_functions_are_anonymous`
and the per-kind class-field runtime counter tests.

The `default-arg` counter wraps a default value the same way, so an anonymous
function default of a parameter or a destructuring target
(`({ a = () => 1 } = o)`) also keeps an empty runtime `name`. Istanbul does the
same.

### 8. Ignore pragmas that reach further than in Istanbul

These `ignore next` placements ignore more than Istanbul does:

| Source | Here | istanbul |
|---|---|---|
| `({ /* istanbul ignore next */ a: b \|\| c })` | value ignored | counted |
| `var { /* istanbul ignore next */ a = 1 } = o` | default ignored | counted |
| `switch (x) { case 1: /* istanbul ignore next */ y(); }` | case ignored | `y()` ignored |
| `class A { static /* istanbul ignore next */ m() {} }` | method ignored | counted |
| `if (a) {} /* istanbul ignore else */ else if (b) {} else {}` | inner `else` ignored | no effect |
| `if (a) {} /* istanbul ignore next */ else if (b) {}` | `else if` ignored | no effect |
| `if (x) { /* istanbul ignore next */ }` then `{ a(); }` | next block ignored | no effect |

A pragma after a modifier (`static`, `get`, `set`, `async`, `*`) applies to the
method. A pragma before `else if` applies to the chained `if`, and a pragma
alone in an empty block applies to the next block or JSX element. This
instrumenter also reads the `v8 ignore` and `c8 ignore` forms and the `/*!`
legal-comment form at every placement. The strict Istanbul profile does none of
this and follows Istanbul.

All other placements follow Istanbul in both profiles. Istanbul wraps some
values in a counter with Babel's `replaceWith`, which moves the leading
comments of the value to the wrapper, so an `ignore next` before such a value
has no effect: `var x = /* istanbul ignore next */ a || b` keeps its branch,
and `class A { m = /* istanbul ignore next */ () => 1 }` keeps its function. A
pragma before a whole default, conditional, or logical expression ignores
everything in it: `function f(/* istanbul ignore next */ x = () => 1) {}`
ignores the arrow too. The same holds for a pragma before a block body, a
`catch`, `finally` or `try` block, a variable declarator
(`var /* .. */ a = b || c`), or a class declaration (`export default /* .. */
class {}`). A class expression stays counted. Before the expression of a concise
arrow body, the pragma ignores that expression but not the statement counter of
the body: `() => /* istanbul ignore next */ a || b` keeps its statement and
loses its branch.

### 9. Consequent location for arm 0 of an `if` (off by default)

`istanbul-lib-instrument` records the whole `IfStatement` as the location of
arm 0 of an `if` branch. Arm 0 then covers the condition and the `else` too, so
a consumer cannot read from `branchMap` which lines run only when the
condition is true. Set `if_consequent_location: true` (npm:
`ifConsequentLocation`) to record the consequent instead:

| Location | default | with the option |
|---|---|---|
| branch `loc` | the whole `if` | the whole `if` |
| arm 0 | the whole `if` | the consequent block or statement |
| arm 1, with `else` | the alternate | the alternate |
| arm 1, no `else` | zero width at the consequent end | the same |

For `if (a) return;` arm 0 is `return;`. In an `else if` chain, arm 1 of the
outer branch is the inner `if`, and arm 0 of the inner branch is its own
consequent. The option changes no counter, no statement or function entry, and
no other branch type. V8 conversion already resolves arm 0 against the
consequent body, so its counts do not change. Defaults to `false`, because
reporters that highlight arm 0 expect the Istanbul range. The strict Istanbul
profile ignores the option.

Pinned by `if_consequent_location.rs`.

This crate is the entry point of the oxc-coverage suite; the source-map,
V8, and reporting layers live in sibling crates.
