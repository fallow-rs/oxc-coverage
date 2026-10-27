//! Istanbul-compatible JavaScript/TypeScript coverage instrumentation using the Oxc AST.
//!
//! This crate parses JS/TS source with [`oxc_parser`], identifies statements,
//! functions, and branches, injects coverage counter expressions, and emits
//! instrumented code. The coverage map output is compatible with Istanbul's
//! `coverage-final.json` format (consumed by Jest, Vitest, c8, nyc, Codecov).
//!
//! ## Example
//!
//! ```
//! use oxc_coverage_instrument::{instrument, InstrumentOptions};
//!
//! let source = "function add(a, b) { return a + b; }";
//! let result = instrument(source, "add.js", &InstrumentOptions::default()).unwrap();
//!
//! println!("Instrumented code:\n{}", result.code);
//! println!("Functions found: {}", result.coverage_map.fn_map.len());
//! ```
//!
//! ## Coverage model
//!
//! The coverage map tracks three dimensions:
//!
//! - **Statements**: every executable statement gets a counter
//! - **Functions**: every function declaration, expression, arrow, and method
//! - **Branches**: if/else, ternary, switch cases, logical &&/||
//!
//! Function names are inferred from the binding an anonymous function is
//! attached to (declarator, property key, assignment target) where
//! `istanbul-lib-instrument` emits `(anonymous_N)`. See the README section
//! "Differences from istanbul-lib-instrument" for the full list.
//!
//! ## Options
//!
//! [`InstrumentOptions`] defaults to the extended Oxc coverage shape described
//! in the README. Set [`InstrumentOptions::compat`] to
//! [`CompatProfile::Istanbul`] for strict `istanbul-lib-instrument` parity.
//! The fields whose semantics do not fit on a single line are expanded below.
//! An experimental host-owned AST entry point is available with the `ast-api`
//! crate feature. The default feature set exposes only the source-to-source API.
//!
//! ### Composing an input source map
//!
//! [`InstrumentOptions::compose_input_source_map`] folds
//! [`InstrumentOptions::input_source_map`] into the coverage map during
//! instrumentation instead of embedding it for downstream composition.
//!
//! The resulting [`FileCoverage`] (and the `coverageData` literal baked into
//! the instrumented code's preamble, hence the runtime coverage variable)
//! carries original-source positions, is re-keyed by the original source
//! `path`, and has no `inputSourceMap` field. A subsequent [`remap_coverage`]
//! / `remapCoverageMap` on the result is a no-op. This trades the
//! per-collection remap round-trip (instrument, then walk every entry through
//! its embedded map at report time) for a one-time composition at instrument
//! time.
//!
//! A coverage point whose positions do not remap through the input source map
//! is not instrumented at all: it gets no `statementMap` / `fnMap` /
//! `branchMap` entry and no counter in the emitted code, so the runtime
//! `__coverage__` object and the emitted counters cannot disagree. Composition
//! is then a pure remap of the surviving positions, and never emits past-EOF
//! entries.
//!
//! If the input map is unusable (declares no source, fails to parse) the gate
//! is off and the embedded `inputSourceMap` is left in place so the lazy remap
//! path still works.
//!
//! ### TypeScript and decorators
//!
//! [`InstrumentOptions::strip_typescript`] runs `oxc_transformer`'s
//! TypeScript-strip pass on the parsed AST before coverage instrumentation.
//! Set it when passing raw TypeScript that has not been pre-transformed by
//! Babel / tsc / esbuild. The output is instrumented JavaScript whose
//! `statementMap` / `branchMap` positions reference the original TypeScript
//! byte offsets, because surviving AST nodes retain their `Span` through the
//! strip pass. **If it is left off and raw TypeScript is passed, the output
//! contains TypeScript syntax and is not executable as JavaScript** (no error
//! is returned). JSX is preserved verbatim on `.tsx` files: the codegen pass
//! emits it unchanged.
//!
//! By default, decorator syntax (Stage 3 and legacy `experimentalDecorators`
//! alike) flows through unchanged. NestJS / Angular / TypeORM users who need
//! `@Injectable()` / `@Controller()` classes lowered into `_decorate(...)`
//! calls, with or without `design:type` / `design:paramtypes` metadata, set
//! [`InstrumentOptions::decorator_mode`] to [`DecoratorMode::Experimental`] or
//! [`DecoratorMode::ExperimentalWithMetadata`].
//!
//! [`InstrumentOptions::strict_null_checks`] is only consulted under
//! [`DecoratorMode::ExperimentalWithMetadata`], where it decides how a
//! nullable union is written into the emitted `design:type` metadata. With
//! `true`, `foo: string | null` emits `Object`, matching what `tsc` does under
//! `strictNullChecks`. With `false`, `null` and `undefined` are elided from
//! the union first, so the same property emits `String`. Getting this wrong is
//! silent: the instrumented code still runs, but NestJS dependency injection,
//! TypeORM column inference, and class-validator all read that metadata and
//! will see a different type than `tsc` would have produced. Set it to match
//! the `tsconfig.json` the source is compiled with.
//!
//! ### Naming callback arguments
//!
//! [`InstrumentOptions::name_callback_arguments`] names a function or arrow
//! expression that is a direct argument of a call or `new` expression and has
//! no other inferable name, deriving the name from the callee:
//! `arr.map(x => x)` gives `"map"`, `el.addEventListener("click", () => {})`
//! gives `"addEventListener"`, `new Promise((res) => {})` gives `"Promise"`.
//! `istanbul-lib-instrument` leaves these `(anonymous_N)`, so this is an
//! opt-in enhancement rather than the default. Names inferred from a binding
//! (variable declarator, property key, assignment target, default value) still
//! take precedence; only the `(anonymous_N)` fallback is replaced. Because the
//! name comes from the callee it is stable across rebuilds, where the
//! `(anonymous_N)` counter renumbers as unrelated functions are added, which
//! matters for downstream tools that key function identity on the name.
//!
//! Only the callee is used, never a sibling string argument such as a route
//! path or a test description: the traversal ancestor for an argument position
//! exposes the callee but not the other arguments.
//!
//! ### The `if` consequent location
//!
//! An `if` branch has two arms in `branchMap`. `istanbul-lib-instrument`
//! records the whole `IfStatement` as the location of arm 0, so arm 0 does not
//! tell which lines run only when the condition is true. With
//! [`InstrumentOptions::if_consequent_location`], arm 0 records the consequent:
//! the block, or the single statement, after the condition.
//!
//! | Arm | Default | With the option |
//! |---|---|---|
//! | `loc` of the branch | the whole `if` | the whole `if` |
//! | arm 0 | the whole `if` | the consequent |
//! | arm 1, with `else` | the alternate | the alternate |
//! | arm 1, no `else` | zero width at the consequent end | the same |
//!
//! For `if (a) { b(); } else if (c) { d(); }`, arm 1 of the outer branch is the
//! inner `if`, and arm 0 of the inner branch is `{ d(); }`. The option changes
//! no counter and no other entry of the coverage map. The strict Istanbul
//! profile ignores it.
//!
//! ## References
//!
//! - <https://github.com/istanbuljs/istanbuljs/tree/istanbul-lib-instrument-v6.0.3/packages/istanbul-lib-instrument>

mod arrow_body;
mod coverage_builder;
mod instrument;
mod pragma;
mod source_text;
mod transform;
mod v8_to_istanbul;

pub use instrument::{
    CompatProfile, DecoratorMode, InstrumentError, InstrumentOptions, InstrumentResult,
    InstrumentSourceType, instrument,
};
#[cfg(feature = "ast-api")]
pub use instrument::{InstrumentProgramResult, instrument_program};
pub use oxc_coverage_source_maps::{
    PositionRemapper, RemapOptions, SourceMapStore, remap_coverage, remap_coverage_map,
    remap_coverage_map_with_loader, remap_coverage_map_with_loader_and_options,
    remap_coverage_map_with_options, remap_coverage_with_loader,
    remap_coverage_with_loader_and_options, remap_coverage_with_options,
};
pub use oxc_coverage_types::{
    BranchEntry, CoverageMapValidationError, FileCoverage, FnEntry, Location, Position,
    UnhandledPragma, parse_coverage_map, parse_coverage_map_validated,
};
pub use v8_to_istanbul::{
    V8CoverageRange, V8FunctionCoverage, V8ToIstanbulError, v8_to_istanbul,
    v8_to_istanbul_with_loader,
};
