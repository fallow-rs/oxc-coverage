//! Top-level instrumentation API.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use oxc_allocator::Allocator;
use oxc_ast::{
    ast::{Expression, Program, Statement},
    builder::AstBuilder,
};
use oxc_codegen::{Codegen, CodegenOptions};
use oxc_coverage_types::{FileCoverage, UnhandledPragma};
use oxc_parser::{Parser, ParserReturn};
use oxc_semantic::{Scoping, SemanticBuilder};
use oxc_span::{SPAN, SourceType};
use oxc_transformer::{
    DecoratorOptions, JsxOptions, TransformOptions, Transformer, TypeScriptOptions,
};
use oxc_traverse::traverse_mut;

use crate::arrow_body::{PreExpandedArrows, pre_expand_concise_arrow_bodies};
use crate::coverage_builder::{CoverageMaps, build_file_coverage, build_function_identity_map};
use crate::pragma::PragmaMap;
use crate::transform::{
    CoverageState, CoverageTransform, PreambleInputs, TransformInit, djb31_hex,
    generate_cov_fn_name, generate_preamble_source,
};

/// Parser source type supplied explicitly by an embedding host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum InstrumentSourceType {
    /// ECMAScript module JavaScript.
    Module,
    /// Classic script JavaScript.
    Script,
    /// CommonJS JavaScript.
    CommonJs,
    /// JavaScript with JSX syntax.
    Jsx,
    /// TypeScript without JSX.
    Ts,
    /// TypeScript with JSX syntax.
    Tsx,
}

impl InstrumentSourceType {
    const fn into_oxc(self) -> SourceType {
        match self {
            Self::Module => SourceType::mjs(),
            Self::Script => SourceType::script(),
            Self::CommonJs => SourceType::cjs(),
            Self::Jsx => SourceType::jsx(),
            Self::Ts => SourceType::ts(),
            Self::Tsx => SourceType::tsx(),
        }
    }
}

/// Preset for matching another instrumenter's observable coverage shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CompatProfile {
    /// Match `istanbul-lib-instrument` wherever the typed coverage model can
    /// represent its output safely.
    Istanbul,
}

/// Options for the `instrument` function.
#[derive(Debug, Clone)]
pub struct InstrumentOptions {
    /// Optional compatibility preset. Defaults to the native Oxc behavior.
    pub compat: Option<CompatProfile>,
    /// Explicit parser source type. When unset, infer it from `filename` after
    /// removing any query or fragment suffix.
    pub source_type: Option<InstrumentSourceType>,
    /// Safe standalone identifier for the global coverage variable (default:
    /// `"__coverage__"`). Names inherited from `Object.prototype` are rejected.
    pub coverage_variable: String,
    /// Whether to generate a source map for the instrumented output.
    pub source_map: bool,
    /// Input source map JSON string from a prior transformation (e.g., TypeScript -> JS).
    /// When provided, this is stored on the `FileCoverage` as `inputSourceMap` so
    /// downstream tools (nyc, istanbul-reports) can chain back to the original source.
    pub input_source_map: Option<String>,
    /// When true AND [`InstrumentOptions::input_source_map`] is set, compose the
    /// input source map into the coverage map during instrumentation instead of
    /// embedding it for downstream composition. Has no effect when
    /// `input_source_map` is `None`.
    ///
    /// See [Composing an input source map](crate#composing-an-input-source-map)
    /// for what this changes about the emitted map and counters.
    ///
    /// Defaults to `false`.
    pub compose_input_source_map: bool,
    /// When true, adds truthy-value tracking (`bT`) for logical expression operands.
    /// This enables nyc-style logic coverage that tracks not just which branch was
    /// taken, but whether each operand evaluated to a truthy value.
    pub report_logic: bool,
    /// When true (the default), receiver-safe optional-chaining (`?.`) links are
    /// tracked as `optional-chain` branches: their operands are wrapped in a
    /// `cov_fn_oc` helper that records whether the value was nullish.
    /// Receiver-bound optional calls such as `object.method?.()` stay native so
    /// instrumentation cannot detach the method from its `this` value.
    ///
    /// Set to false to leave optional chains native: no `cov_fn_oc` helper is
    /// emitted and no `optional-chain` branch is registered. This matches
    /// `istanbul-lib-instrument` byte-for-byte on `?.` and avoids the
    /// per-operand helper-call overhead in optional-chain-dense hot paths.
    /// Statement, function, and other branch coverage are unaffected.
    ///
    /// Defaults to `true`.
    pub track_optional_chain: bool,
    /// Class method names to exclude from coverage instrumentation.
    /// Matches Istanbul's `ignoreClassMethods` behavior for class methods and
    /// named function expressions with a matching id.
    pub ignore_class_methods: Vec<String>,
    /// When true, run `oxc_transformer`'s TypeScript-strip pass on the parsed
    /// AST before coverage instrumentation. Set this when passing raw
    /// TypeScript source that has not been pre-transformed by Babel / tsc /
    /// esbuild.
    ///
    /// See [TypeScript and decorators](crate#typescript-and-decorators) for the
    /// positions the stripped output reports and what happens to raw TypeScript
    /// when this is left off.
    ///
    /// Defaults to `false`, for callers that supply JavaScript already
    /// transformed by Babel / tsc / esbuild.
    pub strip_typescript: bool,
    /// How decorator syntax is handled by the strip pass. See
    /// [`DecoratorMode`] for the variants and their semantics. Has no effect
    /// unless `strip_typescript` is also true.
    ///
    /// Defaults to [`DecoratorMode::PassThrough`]: decorator syntax flows
    /// through verbatim and a downstream tool is responsible for lowering it.
    pub decorator_mode: DecoratorMode,
    /// Whether the source is compiled under [`strictNullChecks`]. Only
    /// consulted when `decorator_mode` is
    /// [`DecoratorMode::ExperimentalWithMetadata`], where it decides how a
    /// nullable union is written into the emitted `design:type` metadata.
    ///
    /// See [TypeScript and decorators](crate#typescript-and-decorators) for the
    /// consumers that read that metadata and why a mismatch is silent.
    ///
    /// Defaults to `true`, matching both the oxc default and `tsc` under
    /// `strict`.
    ///
    /// [`strictNullChecks`]: https://www.typescriptlang.org/tsconfig#strictNullChecks
    pub strict_null_checks: bool,
    /// When true, attach an optional `x_fallow_functionMap` overlay to the
    /// resulting `FileCoverage`. The overlay carries a stable
    /// `fallow:fn:<hex>` identity per function, keyed by the same ids as
    /// `fnMap`, derived from `(path, name, decl span, loc span)`. Standard
    /// Istanbul consumers ignore the `x_`-prefixed field; downstream
    /// code-quality tools (Fallow et al.) use it to join AST inventories,
    /// runtime coverage, and source-mapped positions across runs without
    /// reconstructing identity from `(path, name, line, column)` after the
    /// fact.
    ///
    /// Defaults to `false`, which keeps the serialized map to the fields
    /// Istanbul consumers expect.
    pub function_identity_overlay: bool,
    /// When true, give a name to a function/arrow expression that is a direct
    /// argument of a call or `new` expression and has no other inferable name,
    /// derived from the callee: `arr.map(x => x)` gives `"map"`.
    ///
    /// See [Naming callback arguments](crate#naming-callback-arguments) for how
    /// this interacts with binding-inferred names and why it is opt-in.
    ///
    /// Defaults to `false`.
    pub name_callback_arguments: bool,
}

/// How `strip_typescript` handles decorator syntax.
///
/// The Rust API uses a single enum so invalid combinations (e.g. "emit
/// metadata without lowering decorators") are unrepresentable; the
/// upstream `oxc_transformer` decorator pass is gated on legacy-mode being
/// on, and metadata emission is only meaningful when lowering is active.
///
/// The napi surface keeps the two-optional-boolean shape
/// (`experimentalDecorators` + `emitDecoratorMetadata`) familiar from
/// `tsconfig.json` and reconstructs this enum on the adapter side,
/// returning a JS `Error` for the invalid combination instead of silently
/// promoting it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DecoratorMode {
    /// Decorator syntax flows through verbatim. A downstream tool (Babel,
    /// tsc, esbuild) is expected to lower it.
    #[default]
    PassThrough,
    /// Lower TypeScript `experimentalDecorators` syntax (the
    /// `@Injectable()` / `@Controller()` style used by NestJS, Angular,
    /// class-validator, TypeORM) into runtime `_decorate(...)` calls. No
    /// `design:type` / `design:paramtypes` / `design:returntype` metadata
    /// is emitted. Mirrors `experimentalDecorators: true` +
    /// `emitDecoratorMetadata: false` in `tsconfig.json`.
    ///
    /// The transformer emits ES module imports from
    /// `@oxc-project/runtime/helpers/*` at the top of the file; consumers
    /// must install `@oxc-project/runtime` (or provide an equivalent
    /// shim). See the README for details and troubleshooting.
    Experimental,
    /// Lower experimental decorators AND emit TypeScript-style decorator
    /// metadata (`design:type`, `design:paramtypes`, `design:returntype`)
    /// as `_decorateMetadata(...)` calls alongside each decorated class /
    /// method / property / accessor. Required for NestJS dependency
    /// injection, TypeORM column type inference, and class-validator's
    /// metadata-driven validation. Mirrors `experimentalDecorators: true`
    /// + `emitDecoratorMetadata: true` in `tsconfig.json`.
    ///
    /// Same `@oxc-project/runtime` requirement as [`Self::Experimental`].
    ExperimentalWithMetadata,
}

impl DecoratorMode {
    /// Whether legacy decorator lowering should be enabled on the upstream
    /// transformer (i.e. `DecoratorOptions::legacy`).
    #[must_use]
    pub const fn legacy(self) -> bool {
        matches!(self, Self::Experimental | Self::ExperimentalWithMetadata)
    }

    /// Whether `design:type` / `design:paramtypes` / `design:returntype`
    /// metadata should be emitted alongside lowered decorators.
    #[must_use]
    pub const fn emit_metadata(self) -> bool {
        matches!(self, Self::ExperimentalWithMetadata)
    }
}

impl Default for InstrumentOptions {
    fn default() -> Self {
        Self {
            compat: None,
            source_type: None,
            coverage_variable: "__coverage__".to_string(),
            source_map: false,
            input_source_map: None,
            compose_input_source_map: false,
            report_logic: false,
            track_optional_chain: true,
            ignore_class_methods: Vec::new(),
            strip_typescript: false,
            decorator_mode: DecoratorMode::PassThrough,
            strict_null_checks: true,
            function_identity_overlay: false,
            name_callback_arguments: false,
        }
    }
}

/// Consuming builder over [`InstrumentOptions::default`], one method per field.
///
/// Struct-literal construction stays available, but it breaks whenever a new
/// option is added; chaining off `default()` does not.
///
/// ```
/// use oxc_coverage_instrument::InstrumentOptions;
///
/// let options = InstrumentOptions::default()
///     .with_coverage_variable("__my_coverage__".to_string())
///     .with_report_logic(true);
///
/// assert_eq!(options.coverage_variable, "__my_coverage__");
/// assert!(options.report_logic);
/// ```
impl InstrumentOptions {
    /// Set [`InstrumentOptions::compat`].
    #[must_use]
    pub fn with_compat(mut self, compat: Option<CompatProfile>) -> Self {
        self.compat = compat;
        self
    }

    /// Set [`InstrumentOptions::source_type`].
    #[must_use]
    pub fn with_source_type(mut self, source_type: Option<InstrumentSourceType>) -> Self {
        self.source_type = source_type;
        self
    }

    /// Set [`InstrumentOptions::coverage_variable`].
    #[must_use]
    pub fn with_coverage_variable(mut self, name: String) -> Self {
        self.coverage_variable = name;
        self
    }

    /// Set [`InstrumentOptions::source_map`].
    #[must_use]
    pub fn with_source_map(mut self, source_map: bool) -> Self {
        self.source_map = source_map;
        self
    }

    /// Set [`InstrumentOptions::input_source_map`].
    #[must_use]
    pub fn with_input_source_map(mut self, input_source_map: Option<String>) -> Self {
        self.input_source_map = input_source_map;
        self
    }

    /// Set [`InstrumentOptions::compose_input_source_map`].
    #[must_use]
    pub fn with_compose_input_source_map(mut self, compose: bool) -> Self {
        self.compose_input_source_map = compose;
        self
    }

    /// Set [`InstrumentOptions::report_logic`].
    #[must_use]
    pub fn with_report_logic(mut self, report_logic: bool) -> Self {
        self.report_logic = report_logic;
        self
    }

    /// Set [`InstrumentOptions::track_optional_chain`].
    #[must_use]
    pub fn with_track_optional_chain(mut self, track: bool) -> Self {
        self.track_optional_chain = track;
        self
    }

    /// Set [`InstrumentOptions::ignore_class_methods`].
    #[must_use]
    pub fn with_ignore_class_methods(mut self, names: Vec<String>) -> Self {
        self.ignore_class_methods = names;
        self
    }

    /// Set [`InstrumentOptions::strip_typescript`].
    #[must_use]
    pub fn with_strip_typescript(mut self, strip: bool) -> Self {
        self.strip_typescript = strip;
        self
    }

    /// Set [`InstrumentOptions::decorator_mode`].
    #[must_use]
    pub fn with_decorator_mode(mut self, mode: DecoratorMode) -> Self {
        self.decorator_mode = mode;
        self
    }

    /// Set [`InstrumentOptions::strict_null_checks`].
    #[must_use]
    pub fn with_strict_null_checks(mut self, strict_null_checks: bool) -> Self {
        self.strict_null_checks = strict_null_checks;
        self
    }

    /// Set [`InstrumentOptions::function_identity_overlay`].
    #[must_use]
    pub fn with_function_identity_overlay(mut self, overlay: bool) -> Self {
        self.function_identity_overlay = overlay;
        self
    }

    /// Set [`InstrumentOptions::name_callback_arguments`].
    #[must_use]
    pub fn with_name_callback_arguments(mut self, name_callback_arguments: bool) -> Self {
        self.name_callback_arguments = name_callback_arguments;
        self
    }
}

/// Result of instrumenting a source file.
///
/// Marked `#[non_exhaustive]` so a future output can be added without a
/// breaking change; construct it only through [`instrument`].
#[derive(Debug)]
#[non_exhaustive]
pub struct InstrumentResult {
    /// The instrumented source code with coverage counters injected.
    pub code: String,
    /// Istanbul-compatible coverage map for this file.
    pub coverage_map: FileCoverage,
    /// Pre-serialized JSON of `coverage_map`. Produced once internally for the
    /// preamble's `coverageData` literal and the hash guard, then exposed here
    /// so language bindings (napi-rs, etc.) and downstream JSON sinks can avoid
    /// a second serialization of the same `BTreeMap` tree.
    pub coverage_map_json: String,
    /// Output source map JSON string (only present if `InstrumentOptions::source_map` is true).
    pub source_map: Option<String>,
    /// Unhandled pragma comments found during instrumentation.
    /// Contains `/* istanbul ignore ... */` and `/* v8 ignore ... */` comments
    /// that were not processed. Callers should decide whether to warn or error.
    pub unhandled_pragmas: Vec<UnhandledPragma>,
}

/// Result of the experimental AST-level instrumentation entry point.
///
/// The program contains coverage counters after the call, but not the coverage
/// setup preamble. A host must insert or emit `preamble` after the source's
/// hashbang and directive prologue, then update semantic state for that new
/// declaration. `scoping` describes the counter-mutated program before that
/// host-owned insertion.
#[derive(Debug)]
#[non_exhaustive]
pub struct InstrumentProgramResult {
    /// Semantic scoping returned by `oxc_traverse` after counter injection.
    pub scoping: Scoping,
    /// Istanbul-compatible coverage metadata for the mutated program.
    pub coverage_map: FileCoverage,
    /// Pre-serialized form of `coverage_map` used by the setup preamble.
    pub coverage_map_json: String,
    /// Coverage setup source to insert after hashbang and directives. `None`
    /// when an ignore-file pragma suppresses the whole program.
    pub preamble: Option<String>,
    /// Coverage pragma comments that were found but not handled.
    pub unhandled_pragmas: Vec<UnhandledPragma>,
}

/// Check whether a string is a valid JavaScript identifier (ASCII subset).
fn is_valid_js_identifier(s: &str) -> bool {
    !s.is_empty()
        && s.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_' || c == '$')
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

/// Names inherited by every ordinary JavaScript object. Using one as `gcv`
/// would reuse or mutate prototype state instead of creating an own coverage
/// store on the selected global object.
const OBJECT_PROTOTYPE_NAMES: [&str; 12] = [
    "__proto__",
    "constructor",
    "hasOwnProperty",
    "isPrototypeOf",
    "propertyIsEnumerable",
    "toLocaleString",
    "toString",
    "valueOf",
    "__defineGetter__",
    "__defineSetter__",
    "__lookupGetter__",
    "__lookupSetter__",
];

fn is_safe_coverage_variable(s: &str) -> bool {
    is_valid_js_identifier(s) && !OBJECT_PROTOTYPE_NAMES.contains(&s)
}

/// Serialize a `FileCoverage` to JSON.
///
/// `FileCoverage` is composed of `BTreeMap`, `Vec`, `String` and primitive
/// numbers, all with first-party serde implementations that cannot fail at
/// runtime, so the serialization is infallible.
fn serialize_coverage_map(coverage_map: &FileCoverage) -> String {
    serde_json::to_string(coverage_map).expect("FileCoverage serializes to JSON infallibly")
}

/// Instrument a JavaScript/TypeScript source file for coverage collection.
///
/// Parses the source with `oxc_parser`, collects statement/function/branch
/// locations via AST traversal, injects coverage counter expressions into
/// the AST, and emits the instrumented code via `oxc_codegen`.
///
/// # Errors
///
/// - [`InstrumentError::InvalidCoverageVariable`] if
///   [`InstrumentOptions::coverage_variable`] is not a safe standalone
///   JavaScript identifier
/// - [`InstrumentError::ParseError`] if `source` cannot be parsed
/// - [`InstrumentError::TransformError`] if
///   [`InstrumentOptions::strip_typescript`] is set and the strip pass reports
///   an error
/// - [`InstrumentError::PreambleError`] if the generated coverage setup cannot
///   be placed after the directive prologue
///
/// # Example
///
/// ```
/// use oxc_coverage_instrument::{instrument, InstrumentOptions};
///
/// let source = "function add(a, b) { return a + b; }";
/// let result = instrument(source, "add.js", &InstrumentOptions::default()).unwrap();
///
/// // coverage_map contains fnMap, statementMap, branchMap
/// assert_eq!(result.coverage_map.fn_map.len(), 1);
/// assert_eq!(result.coverage_map.fn_map["0"].name, "add");
/// ```
pub fn instrument(
    source: &str,
    filename: &str,
    options: &InstrumentOptions,
) -> Result<InstrumentResult, InstrumentError> {
    validate_coverage_variable(options)?;
    let allocator = Allocator::default();
    let mut parsed = parse_program(&allocator, source, filename, options.source_type)?;
    let prepared_input_source_map = prepare_input_source_map(options);

    let (pragmas, unhandled_pragmas) = PragmaMap::from_program(
        &parsed.program,
        source,
        options.compat == Some(CompatProfile::Istanbul),
    );
    // The strip pass inside `prepare_scoping` rewrites the body of a concise
    // arrow whose expression is a TypeScript wrapper (`(x: number) => x as T`)
    // down to the inner expression, which is all the span that survives. Giving
    // those bodies their block wrapper here, while the spans are still the ones
    // the source wrote, keeps the arrow's `fnMap` `loc` and its statement entry
    // whole. Nothing else between parse and traverse moves a span, so the
    // non-stripping paths leave the wrapping to the traverse itself.
    //
    // Wrapping is only half of the shape change: the `return` that keeps the
    // arrow yielding its value is formed on the way out of the coverage
    // traverse. An ignore-file pragma returns from
    // `instrument_program_with_pragmas` before that traverse, so wrapping there
    // would emit `(x) => { x; }` and silently return `undefined`. The gate below
    // mirrors that early return: pre-expand only when the traverse will follow.
    let pre_expanded_arrows = if options.strip_typescript && !pragmas.ignore_file {
        pre_expand_concise_arrow_bodies(&allocator, &mut parsed.program)
    } else {
        PreExpandedArrows::default()
    };
    let scoping = prepare_scoping(&allocator, filename, &mut parsed.program, options)?;
    let program_result = instrument_program_with_pragmas(InstrumentProgramInput {
        allocator: &allocator,
        program: &mut parsed.program,
        scoping,
        pragmas,
        unhandled_pragmas,
        pre_expanded_arrows,
        source,
        filename,
        options,
        prepared_input_source_map: prepared_input_source_map.as_ref(),
    })?;
    let InstrumentProgramResult {
        scoping,
        coverage_map,
        coverage_map_json,
        preamble,
        unhandled_pragmas,
    } = program_result;

    let Some(preamble) = preamble else {
        if !options.strip_typescript {
            return Ok(InstrumentResult {
                code: source.to_string(),
                coverage_map,
                coverage_map_json,
                source_map: None,
                unhandled_pragmas,
            });
        }
        let emitted = emit_code(EmitInputs {
            allocator: &allocator,
            program: &mut parsed.program,
            scoping,
            source,
            filename,
            preamble: None,
            options,
        })?;
        return Ok(InstrumentResult {
            code: emitted.code,
            coverage_map,
            coverage_map_json,
            source_map: None,
            unhandled_pragmas,
        });
    };
    let emitted = emit_code(EmitInputs {
        allocator: &allocator,
        program: &mut parsed.program,
        scoping,
        source,
        filename,
        preamble: Some(&preamble),
        options,
    })?;
    let source_map = emitted.source_map.as_deref().map(|sm| {
        oxc_coverage_source_maps::finalize_generated_source_map(
            sm,
            emitted.preamble_shift.map(|shift| oxc_coverage_source_maps::GeneratedLineShift {
                first_following_line: shift.first_following_line,
                line_delta: shift.line_delta,
            }),
            prepared_input_source_map.as_ref(),
            options.input_source_map.as_deref(),
        )
    });

    Ok(InstrumentResult {
        code: emitted.code,
        coverage_map,
        coverage_map_json,
        source_map,
        unhandled_pragmas,
    })
}

struct InstrumentProgramInput<'src, 'arena, 'a> {
    allocator: &'arena Allocator,
    program: &'a mut Program<'arena>,
    scoping: Scoping,
    pragmas: PragmaMap,
    unhandled_pragmas: Vec<UnhandledPragma>,
    /// Arrows whose concise body was already wrapped in a block before a
    /// span-rewriting pass ran. Empty when no such pass ran.
    pre_expanded_arrows: PreExpandedArrows,
    source: &'src str,
    filename: &'a str,
    options: &'a InstrumentOptions,
    prepared_input_source_map: Option<&'src oxc_coverage_source_maps::PositionRemapper>,
}

fn instrument_program_with_pragmas(
    input: InstrumentProgramInput<'_, '_, '_>,
) -> Result<InstrumentProgramResult, InstrumentError> {
    let InstrumentProgramInput {
        allocator,
        program,
        scoping,
        pragmas,
        unhandled_pragmas,
        pre_expanded_arrows,
        source,
        filename,
        options,
        prepared_input_source_map,
    } = input;
    validate_coverage_variable(options)?;
    if pragmas.ignore_file {
        // This return skips the traverse, and the traverse is what turns a
        // wrapped concise arrow body back into a `return`. Any caller that
        // pre-expands and then lands here would emit `(x) => { x; }`, so the
        // pre-expansion gate in `instrument` has to keep this path out.
        debug_assert!(
            pre_expanded_arrows.is_empty(),
            "concise arrow bodies were pre-expanded before an ignore-file return, \
             which skips the traverse that forms their `return`"
        );
        let coverage_map = empty_coverage(filename);
        let coverage_map_json = serialize_coverage_map(&coverage_map);
        return Ok(InstrumentProgramResult {
            scoping,
            coverage_map,
            coverage_map_json,
            preamble: None,
            unhandled_pragmas,
        });
    }

    let cov_fn_base = generate_cov_fn_name(filename);
    let cov_fn_name = resolve_cov_fn_name(&cov_fn_base, &scoping);
    let (transform, scoping) = run_coverage_transform(CoverageTransformRun {
        allocator,
        program,
        scoping,
        pragmas,
        pre_expanded_arrows,
        source,
        cov_fn_name: &cov_fn_name,
        options,
        eager_remapper: options
            .compose_input_source_map
            .then_some(prepared_input_source_map)
            .flatten(),
    });
    let needs_optional_chain_helper = transform.used_optional_chain_helper;
    let coverage_map =
        finalize_coverage_map(filename, transform, options, prepared_input_source_map);
    let (coverage_map_json, preamble) = build_instrument_preamble(
        &coverage_map,
        options,
        &cov_fn_name,
        needs_optional_chain_helper,
    );

    Ok(InstrumentProgramResult {
        scoping,
        coverage_map,
        coverage_map_json,
        preamble: Some(preamble),
        unhandled_pragmas,
    })
}

/// Inject coverage counters into a host-owned Oxc program without parsing or
/// code generation.
///
/// The supplied `scoping` must describe `program`, and every program span must
/// refer to `source_text`. The host remains responsible for TypeScript or JSX
/// lowering; parser and output options on [`InstrumentOptions`] do not run
/// those phases here. Coverage-shape, source-map composition and naming options
/// still apply.
///
/// The returned `preamble` is source metadata rather than an AST fragment. It
/// must be inserted or emitted after the program's hashbang and directives.
/// Rebuild semantic state after inserting it, because it declares the binding
/// referenced by the injected counters.
///
/// # Errors
///
/// Returns [`InstrumentError::InvalidCoverageVariable`] when the configured
/// coverage variable is not a safe standalone JavaScript identifier.
///
/// # Example
///
/// ```
/// use oxc_allocator::Allocator;
/// use oxc_coverage_instrument::{InstrumentOptions, instrument_program};
/// use oxc_parser::Parser;
/// use oxc_semantic::SemanticBuilder;
/// use oxc_span::SourceType;
///
/// let source = "function answer() { return 42; }";
/// let allocator = Allocator::default();
/// let mut parsed = Parser::new(&allocator, source, SourceType::mjs()).parse();
/// assert!(!parsed.diagnostics.has_errors());
/// let scoping = SemanticBuilder::new().build(&parsed.program).semantic.into_scoping();
/// let result = instrument_program(
///     &allocator,
///     &mut parsed.program,
///     scoping,
///     source,
///     "answer.js",
///     &InstrumentOptions::default(),
/// )?;
///
/// assert!(result.preamble.is_some());
/// assert_eq!(result.coverage_map.fn_map["0"].name, "answer");
/// # Ok::<(), oxc_coverage_instrument::InstrumentError>(())
/// ```
#[cfg(feature = "ast-api")]
pub fn instrument_program<'arena>(
    allocator: &'arena Allocator,
    program: &mut Program<'arena>,
    scoping: Scoping,
    source_text: &str,
    filename: &str,
    options: &InstrumentOptions,
) -> Result<InstrumentProgramResult, InstrumentError> {
    let (pragmas, unhandled_pragmas) = PragmaMap::from_program(
        program,
        source_text,
        options.compat == Some(CompatProfile::Istanbul),
    );
    let prepared_input_source_map = prepare_input_source_map(options);
    instrument_program_with_pragmas(InstrumentProgramInput {
        allocator,
        program,
        scoping,
        pragmas,
        unhandled_pragmas,
        // The host owns TypeScript lowering on this path, so nothing here
        // rewrites a span before the traverse and every concise body is still
        // one when the arrow is entered.
        pre_expanded_arrows: PreExpandedArrows::default(),
        source: source_text,
        filename,
        options,
        prepared_input_source_map: prepared_input_source_map.as_ref(),
    })
}

const COVERAGE_BINDING_SUFFIXES: [&str; 4] = ["", "_bt", "_oc", "_temp"];

fn resolve_cov_fn_name(base: &str, scoping: &Scoping) -> String {
    if coverage_binding_is_available(base, scoping) {
        return base.to_string();
    }

    let mut suffix = 1_u32;
    loop {
        let candidate = format!("{base}_{suffix}");
        if coverage_binding_is_available(&candidate, scoping) {
            return candidate;
        }
        suffix = suffix.checked_add(1).expect("coverage binding suffix space exhausted");
    }
}

fn coverage_binding_is_available(candidate: &str, scoping: &Scoping) -> bool {
    let declared_names_available = scoping.iter_bindings().all(|(_, bindings)| {
        bindings.keys().all(|symbol| {
            COVERAGE_BINDING_SUFFIXES
                .iter()
                .all(|suffix| !symbol_is_coverage_binding(symbol.as_str(), candidate, suffix))
        })
    });
    declared_names_available
        && scoping.root_unresolved_references().keys().all(|symbol| {
            COVERAGE_BINDING_SUFFIXES
                .iter()
                .all(|suffix| !symbol_is_coverage_binding(symbol.as_str(), candidate, suffix))
        })
}

fn symbol_is_coverage_binding(symbol: &str, candidate: &str, suffix: &str) -> bool {
    symbol.len() == candidate.len().saturating_add(suffix.len())
        && symbol.starts_with(candidate)
        && symbol.ends_with(suffix)
}

fn validate_coverage_variable(options: &InstrumentOptions) -> Result<(), InstrumentError> {
    if is_safe_coverage_variable(&options.coverage_variable) {
        return Ok(());
    }
    Err(InstrumentError::InvalidCoverageVariable(options.coverage_variable.clone()))
}

fn prepare_scoping<'arena>(
    allocator: &'arena Allocator,
    filename: &str,
    program: &mut Program<'arena>,
    options: &InstrumentOptions,
) -> Result<Scoping, InstrumentError> {
    // `with_enum_eval` pre-computes TypeScript enum member values into `Scoping`.
    // Since oxc 0.140 the transformer asserts on it when lowering an `enum`, so
    // this is required on the scoping handed to `strip_typescript_pass` below.
    // The V8-collect path builds its own scoping for the traverse pass only, and
    // never reaches the transformer, so it does not pay for enum evaluation.
    let scoping =
        SemanticBuilder::new().with_enum_eval(true).build(program).semantic.into_scoping();
    if !options.strip_typescript {
        return Ok(scoping);
    }
    strip_typescript_pass(allocator, filename, program, scoping, options)
}

struct CoverageTransformRun<'src, 'arena, 'a> {
    allocator: &'arena Allocator,
    program: &'a mut Program<'arena>,
    scoping: Scoping,
    pragmas: PragmaMap,
    pre_expanded_arrows: PreExpandedArrows,
    source: &'src str,
    cov_fn_name: &'src str,
    options: &'a InstrumentOptions,
    eager_remapper: Option<&'src oxc_coverage_source_maps::PositionRemapper>,
}

fn run_coverage_transform<'src, 'arena>(
    input: CoverageTransformRun<'src, 'arena, '_>,
) -> (CoverageTransform<'src, 'arena>, Scoping) {
    let CoverageTransformRun {
        allocator,
        program,
        scoping,
        pragmas,
        pre_expanded_arrows,
        source,
        cov_fn_name,
        options,
        eager_remapper,
    } = input;
    let mut transform = CoverageTransform::new(TransformInit {
        allocator,
        source,
        cov_fn_name,
        report_logic: options.report_logic,
        track_optional_chain: options.track_optional_chain
            && options.compat != Some(CompatProfile::Istanbul),
        istanbul_compat: options.compat == Some(CompatProfile::Istanbul),
        ignore_class_methods: options.ignore_class_methods.clone(),
        name_callback_arguments: options.name_callback_arguments,
        // Istanbul gives a private method no fnMap entry.
        record_private_methods: false,
        pre_expanded_arrows,
        eager_remapper,
    });
    let state = CoverageState { pragmas };
    let scoping = traverse_mut(&mut transform, allocator, program, scoping, state);
    (transform, scoping)
}

fn build_instrument_preamble(
    coverage_map: &FileCoverage,
    options: &InstrumentOptions,
    cov_fn_name: &str,
    // Whether the transform wrapped any optional-chain link, so the preamble
    // declares the `cov_fn_oc` helper.
    needs_optional_chain_helper: bool,
) -> (String, String) {
    // Serialize once and reuse it for both the hash guard and the coverageData
    // literal.
    let coverage_json = serialize_coverage_map(coverage_map);
    let coverage_hash = djb31_hex(&coverage_json);
    let preamble = generate_preamble_source(&PreambleInputs {
        coverage: coverage_map,
        coverage_json: &coverage_json,
        coverage_hash: &coverage_hash,
        coverage_var: &options.coverage_variable,
        cov_fn_name,
        report_logic: options.report_logic,
        needs_optional_chain_helper,
    });
    (coverage_json, preamble)
}

/// Run `oxc_transformer`'s TypeScript-strip pass on the parsed program in
/// place. Returns the updated `Scoping` produced by the transformer (the
/// semantic state changes as type-only nodes are removed). Surviving nodes
/// retain their original `Span` values, so positions still refer to the
/// original TypeScript source offsets.
///
/// # Errors
///
/// Returns [`InstrumentError::TransformError`] if the transformer reports a
/// diagnostic at error severity.
fn strip_typescript_pass<'arena>(
    allocator: &'arena Allocator,
    filename: &str,
    program: &mut Program<'arena>,
    scoping: Scoping,
    options: &InstrumentOptions,
) -> Result<Scoping, InstrumentError> {
    // `JsxOptions::default()` calls `JsxOptions::enable()`, which would
    // rewrite `<div>` to `React.createElement` / `_jsx` on `.tsx` input.
    // Strip-pass only removes type syntax; JSX must round-trip unchanged
    // so codegen can emit it verbatim. Pin the JSX pass off explicitly.
    // `typescript` is also listed explicitly so a future change to
    // `TransformOptions::default()` cannot silently alter the strip pass.
    //
    // `decorator` defaults to `legacy: false, emit_decorator_metadata: false`,
    // which makes the decorator pass a no-op (syntax flows through verbatim).
    // Callers can opt into legacy lowering and metadata emission via
    // `InstrumentOptions::decorator_mode`, and control how nullable unions are
    // written into that metadata via `InstrumentOptions::strict_null_checks`.
    let transform_options = TransformOptions {
        typescript: TypeScriptOptions::default(),
        jsx: JsxOptions::disable(),
        decorator: DecoratorOptions {
            legacy: options.decorator_mode.legacy(),
            emit_decorator_metadata: options.decorator_mode.emit_metadata(),
            strict_null_checks: options.strict_null_checks,
        },
        ..TransformOptions::default()
    };
    let transformer = Transformer::new(allocator, Path::new(filename), &transform_options);
    let ret = transformer.build_with_scoping(scoping, program);
    // `diagnostics` carries warnings as well as errors, so gate on error
    // severity: a warning-only transform is not a failure.
    if ret.diagnostics.has_errors() {
        return Err(InstrumentError::TransformError(
            ret.diagnostics.errors().map(|e| format!("{e}")).collect::<Vec<_>>(),
        ));
    }
    Ok(ret.scoping)
}

fn parse_program<'a>(
    allocator: &'a Allocator,
    source: &'a str,
    filename: &str,
    source_type: Option<InstrumentSourceType>,
) -> Result<ParserReturn<'a>, InstrumentError> {
    let filename = filename.split(['?', '#']).next().unwrap_or(filename);
    let source_type = source_type.map_or_else(
        || SourceType::from_path(filename).unwrap_or_default(),
        InstrumentSourceType::into_oxc,
    );
    let parsed = Parser::new(allocator, source, source_type).parse();
    // As in `strip_typescript_pass`: gate on error severity, not on any diagnostic.
    if parsed.diagnostics.has_errors() {
        Err(InstrumentError::ParseError(
            parsed.diagnostics.errors().map(|e| format!("{e}")).collect::<Vec<_>>().join("; "),
        ))
    } else {
        Ok(parsed)
    }
}

/// A `FileCoverage` with every map empty, for a source suppressed by
/// `/* istanbul ignore file */`.
fn empty_coverage(filename: &str) -> FileCoverage {
    build_file_coverage(CoverageMaps {
        path: filename.to_string(),
        statement_locs: Vec::new(),
        fn_entries: Vec::new(),
        branch_entries: Vec::new(),
        logical_branch_ids: Vec::new(),
    })
}

fn build_coverage_map(filename: &str, transform: CoverageTransform<'_, '_>) -> FileCoverage {
    build_file_coverage(CoverageMaps {
        path: filename.to_string(),
        statement_locs: transform.statement_map,
        fn_entries: transform.fn_map,
        branch_entries: transform.branch_map,
        logical_branch_ids: transform.logical_branch_ids,
    })
}

fn prepare_input_source_map(
    options: &InstrumentOptions,
) -> Option<oxc_coverage_source_maps::PositionRemapper> {
    if !options.compose_input_source_map && !options.source_map {
        return None;
    }
    options
        .input_source_map
        .as_deref()
        .and_then(oxc_coverage_source_maps::PositionRemapper::from_json)
}

fn finalize_coverage_map(
    filename: &str,
    transform: CoverageTransform<'_, '_>,
    options: &InstrumentOptions,
    prepared_input_source_map: Option<&oxc_coverage_source_maps::PositionRemapper>,
) -> FileCoverage {
    let overlay_conflict = transform.eager_function_overlay_conflict;
    let mut coverage_map = build_coverage_map(filename, transform);
    if options.function_identity_overlay {
        // Dropped before composition, so the composed result is overlay-free
        // the way the deferred `merge_functions` conflict rule leaves it.
        coverage_map.x_fallow_function_map = (!overlay_conflict)
            .then(|| build_function_identity_map(&coverage_map.path, &coverage_map.fn_map));
    }

    // Fold the embedded `inputSourceMap` in before the map is serialized into
    // the preamble's `coverageData` literal, so the runtime `__coverage__`
    // ships original-source positions and a later `remap_coverage` is a no-op.
    // Ordered after the function-identity overlay so the overlay's ids stay
    // derived from pre-remap positions, which is what instrument-then-remap
    // produces (the remap pipeline does not rewrite the overlay). An unusable
    // input map yields `None` here, leaving the embedded map in place for the
    // lazy remap path.
    if options.compose_input_source_map
        && options.input_source_map.is_some()
        && let Some(composed) =
            prepared_input_source_map.and_then(|prepared| prepared.remap_coverage(&coverage_map))
    {
        return composed;
    }

    if let Some(input_sm) = options.input_source_map.as_deref() {
        coverage_map.input_source_map = serde_json::from_str(input_sm).ok();
    }

    coverage_map
}

/// Output of [`collect_for_v8_to_istanbul`]: the Istanbul `FileCoverage`
/// (statement / function / branch maps) plus a side-table of body byte spans
/// keyed by surviving branch id. Both are needed by `v8_to_istanbul` to
/// resolve V8 hit counts; the body byte spans solve the if-arm 0 case where
/// the istanbul-reported location (the whole `IfStatement`) does not match
/// any V8 block range.
pub struct V8CollectResult {
    /// Location maps with all hit counts still zero.
    pub coverage_map: FileCoverage,
    /// `arm_body_byte_spans["<branch_id>"][<arm_idx>]` is the `(start, end)`
    /// byte range of the arm body when known, or `(0, 0)` when the body span
    /// is synthetic / unknown (e.g. a synthesized else-arm).
    pub arm_body_byte_spans: BTreeMap<String, Vec<(u32, u32)>>,
}

/// Parse, scan pragmas, traverse the AST, and build the `FileCoverage` map
/// without performing codegen, preamble emission, or coverage-map JSON
/// serialization. This is the visit-only path `v8_to_istanbul` uses; the
/// codegen + preamble + hash work that `instrument()` performs is dead work
/// when the caller only needs the location maps to intersect against V8
/// UTF-16 ranges.
///
/// # Errors
///
/// Returns [`InstrumentError::ParseError`] if `source` cannot be parsed.
pub fn collect_for_v8_to_istanbul(
    source: &str,
    filename: &str,
) -> Result<V8CollectResult, InstrumentError> {
    let allocator = Allocator::default();
    let mut parsed = parse_program(&allocator, source, filename, None)?;

    let (pragmas, _unhandled_pragmas) = PragmaMap::from_program(&parsed.program, source, false);
    if pragmas.ignore_file {
        return Ok(V8CollectResult {
            coverage_map: empty_coverage(filename),
            arm_body_byte_spans: BTreeMap::new(),
        });
    }

    let scoping = SemanticBuilder::new().build(&parsed.program).semantic.into_scoping();
    let cov_fn_name = generate_cov_fn_name(filename);

    let mut transform = CoverageTransform::new(TransformInit {
        allocator: &allocator,
        source,
        cov_fn_name: &cov_fn_name,
        report_logic: false,
        // V8-collect builds the location maps that V8 UTF-16 ranges intersect
        // against, so optional-chain branches stay tracked (the default); this
        // path emits no runtime helper, only the maps.
        track_optional_chain: true,
        istanbul_compat: false,
        ignore_class_methods: Vec::new(),
        // V8-collect has no options to wire, so it keeps Istanbul naming for
        // callback arguments. The fnMap names do reach consumers: a caller can
        // match a function entry on its name, line, decl, and loc.
        name_callback_arguments: false,
        // V8 reports a function range for a private method, so the method
        // needs its own fnMap entry. Without it, a consumer cannot match that
        // range to the method.
        record_private_methods: true,
        // V8-collect parses and traverses in one step, with no strip pass in
        // between, so no arrow body is wrapped ahead of the traverse.
        pre_expanded_arrows: PreExpandedArrows::default(),
        // V8-collect never composes an input source map; gate is a no-op.
        eager_remapper: None,
    });
    let state = CoverageState { pragmas };
    let _scoping = traverse_mut(&mut transform, &allocator, &mut parsed.program, scoping, state);

    // Built before `branch_map` moves into `build_file_coverage`, which drops
    // branches with no locations but assigns ids from the pre-filter
    // `enumerate` index. The keys here use that same index, so they line up
    // with the surviving entries.
    let arm_body_byte_spans = collect_arm_body_byte_spans(&transform);

    let coverage_map = build_coverage_map(filename, transform);
    Ok(V8CollectResult { coverage_map, arm_body_byte_spans })
}

fn collect_arm_body_byte_spans(
    transform: &CoverageTransform<'_, '_>,
) -> BTreeMap<String, Vec<(u32, u32)>> {
    let mut spans: BTreeMap<String, Vec<(u32, u32)>> = BTreeMap::new();
    for (idx, body_spans) in transform.branch_arm_body_byte_spans.iter().enumerate() {
        let surviving =
            transform.branch_map.get(idx).is_some_and(|entry| !entry.locations.is_empty());
        if surviving {
            spans.insert(idx.to_string(), body_spans.clone());
        }
    }
    spans
}

struct EmitInputs<'a, 'arena> {
    allocator: &'arena Allocator,
    program: &'a mut Program<'arena>,
    scoping: Scoping,
    source: &'a str,
    filename: &'a str,
    preamble: Option<&'a str>,
    options: &'a InstrumentOptions,
}

struct EmitResult {
    code: String,
    source_map: Option<String>,
    preamble_shift: Option<PreambleShift>,
}

#[derive(Clone, Copy)]
struct PreambleShift {
    first_following_line: u32,
    line_delta: u32,
}

fn emit_code(inputs: EmitInputs<'_, '_>) -> Result<EmitResult, InstrumentError> {
    let EmitInputs { allocator, program, scoping, source, filename, preamble, options } = inputs;
    let preamble_marker = if preamble.is_some() {
        Some(insert_preamble_marker(allocator, program, source)?)
    } else {
        None
    };
    let codegen_options = CodegenOptions {
        source_map_path: if options.source_map { Some(PathBuf::from(filename)) } else { None },
        ..CodegenOptions::default()
    };
    let codegen_ret = Codegen::new()
        .with_options(codegen_options)
        .with_source_text(source)
        .with_scoping(Some(scoping))
        .build(program);
    let source_map = codegen_ret.map.map(|sm| sm.to_json_string());
    let Some(preamble) = preamble else {
        return Ok(EmitResult { code: codegen_ret.code, source_map, preamble_shift: None });
    };
    let marker = preamble_marker.ok_or_else(|| {
        InstrumentError::PreambleError("coverage setup has no insertion marker".to_string())
    })?;
    let (code, preamble_shift) = replace_preamble_marker(&codegen_ret.code, marker, preamble)?;
    Ok(EmitResult { code, source_map, preamble_shift: Some(preamble_shift) })
}

const PREAMBLE_MARKER_BASE: &str = "__oxc_coverage_preamble_marker__";

fn insert_preamble_marker<'arena>(
    allocator: &'arena Allocator,
    program: &mut Program<'arena>,
    source: &str,
) -> Result<&'arena str, InstrumentError> {
    let mut marker = PREAMBLE_MARKER_BASE.to_string();
    let mut suffix = 0_u32;
    while source.contains(&marker) {
        suffix = suffix.checked_add(1).ok_or_else(|| {
            InstrumentError::PreambleError("insertion marker suffix exceeds u32".to_string())
        })?;
        marker = format!("{PREAMBLE_MARKER_BASE}_{suffix}");
    }
    let marker = allocator.alloc_str(&marker);
    let ast = AstBuilder::new(allocator);
    let expression = Expression::new_identifier(SPAN, marker, &ast);
    let statement = Statement::new_expression_statement(SPAN, expression, &ast);
    // Preserved comments can add output lines before a directive. Making the
    // marker the first body statement lets codegen place it after every
    // directive and any comments belonging to that prologue.
    program.body.insert(0, statement);
    Ok(marker)
}

fn replace_preamble_marker(
    code: &str,
    marker: &str,
    preamble: &str,
) -> Result<(String, PreambleShift), InstrumentError> {
    let marker_statement = format!("{marker};");
    let marker_start = code.find(&marker_statement).ok_or_else(|| {
        InstrumentError::PreambleError("generated output omitted the insertion marker".to_string())
    })?;
    let mut marker_end = marker_start.saturating_add(marker_statement.len());
    if code.as_bytes().get(marker_end) == Some(&b'\n') {
        marker_end = marker_end.saturating_add(1);
    }
    let marker_line = u32::try_from(
        code[..marker_start].bytes().filter(|byte| *byte == b'\n').count(),
    )
    .map_err(|_| InstrumentError::PreambleError("insertion line exceeds u32".to_string()))?;
    let preamble_lines = u32::try_from(preamble.bytes().filter(|byte| *byte == b'\n').count())
        .map_err(|_| InstrumentError::PreambleError("setup line count exceeds u32".to_string()))?;
    if preamble_lines == 0 || !preamble.ends_with('\n') {
        return Err(InstrumentError::PreambleError(
            "generated coverage setup must end with a newline".to_string(),
        ));
    }
    let mut output = String::with_capacity(
        code.len()
            .saturating_sub(marker_end.saturating_sub(marker_start))
            .saturating_add(preamble.len()),
    );
    output.push_str(&code[..marker_start]);
    output.push_str(preamble);
    output.push_str(&code[marker_end..]);
    Ok((
        output,
        PreambleShift {
            first_following_line: marker_line.saturating_add(1),
            line_delta: preamble_lines.saturating_sub(1),
        },
    ))
}

/// Error type for instrumentation failures.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum InstrumentError {
    /// The source could not be parsed.
    ParseError(String),
    /// The generated coverage setup could not be placed in the emitted output.
    PreambleError(String),
    /// The coverage variable is not a standalone safe JavaScript identifier.
    InvalidCoverageVariable(String),
    /// The TypeScript strip pass produced diagnostics. Only emitted when
    /// `InstrumentOptions::strip_typescript` is enabled. The vector
    /// contains one entry per transformer diagnostic so callers can
    /// surface them individually instead of string-scraping a joined
    /// message.
    TransformError(Vec<String>),
}

impl std::fmt::Display for InstrumentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ParseError(msg) => write!(f, "parse error: {msg}"),
            Self::PreambleError(msg) => write!(f, "coverage preamble error: {msg}"),
            Self::TransformError(msgs) => write!(f, "transform error: {}", msgs.join("; ")),
            Self::InvalidCoverageVariable(name) => {
                write!(
                    f,
                    "invalid coverage variable: {name:?} must be a standalone JavaScript identifier that is not inherited from Object.prototype"
                )
            }
        }
    }
}

impl std::error::Error for InstrumentError {}
