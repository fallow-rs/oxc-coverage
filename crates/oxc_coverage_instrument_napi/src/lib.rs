//! Node.js bindings for `oxc_coverage_instrument`.
//!
//! Exposes instrumentation, coverage remapping, V8-to-Istanbul conversion,
//! coverage summaries and the Istanbul reporters to JavaScript via napi-rs.

#![expect(
    clippy::needless_pass_by_value,
    reason = "napi-derive requires owned types in JS-facing signatures"
)]
#![expect(
    clippy::implicit_hasher,
    reason = "napi-derive cannot lower a generic BuildHasher parameter into the napi function signature"
)]
#![expect(
    clippy::disallowed_types,
    reason = "napi-derive keys its TypeScript emit on the `HashMap` ident to produce `Record<K, V>`"
)]

pub mod report;

use std::{collections::HashMap, fmt};

use napi_derive::napi;
use oxc_coverage_instrument::{
    CompatProfile as CoreCompatProfile, DecoratorMode,
    InstrumentSourceType as CoreInstrumentSourceType, RemapOptions as CoreRemapOptions,
};

/// Compatibility preset for the generated coverage shape.
#[napi(string_enum = "lowercase")]
#[derive(serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CompatProfile {
    /// Match `istanbul-lib-instrument` wherever the typed model permits it.
    Istanbul,
}

impl From<CompatProfile> for CoreCompatProfile {
    fn from(profile: CompatProfile) -> Self {
        match profile {
            CompatProfile::Istanbul => Self::Istanbul,
        }
    }
}

/// Parser source type supplied explicitly by an embedding host.
#[napi(string_enum = "lowercase")]
#[derive(serde::Deserialize)]
#[serde(rename_all = "lowercase")]
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

impl From<InstrumentSourceType> for CoreInstrumentSourceType {
    fn from(source_type: InstrumentSourceType) -> Self {
        match source_type {
            InstrumentSourceType::Module => Self::Module,
            InstrumentSourceType::Script => Self::Script,
            InstrumentSourceType::CommonJs => Self::CommonJs,
            InstrumentSourceType::Jsx => Self::Jsx,
            InstrumentSourceType::Ts => Self::Ts,
            InstrumentSourceType::Tsx => Self::Tsx,
        }
    }
}

/// Options for the instrument function.
#[napi(object)]
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InstrumentOptions {
    /// Compatibility preset. Use `istanbul` for strict Istanbul output shape.
    pub compat: Option<CompatProfile>,
    /// Explicit parser source type. Overrides filename inference.
    pub source_type: Option<InstrumentSourceType>,
    /// Safe standalone identifier for the global coverage variable. Defaults
    /// to `__coverage__`. Names inherited from `Object.prototype` are rejected.
    pub coverage_variable: Option<String>,
    /// Generate a source map for the instrumented output. Defaults to `false`.
    pub source_map: Option<bool>,
    /// Input source map JSON string from a prior transformation.
    pub input_source_map: Option<String>,
    /// Compose `inputSourceMap` into the coverage map during instrumentation.
    ///
    /// The returned `coverageMap`, and the `__coverage__` object baked into the
    /// instrumented `code`, carry original-source positions, are keyed by the
    /// original source path, and embed no `inputSourceMap`, so
    /// `remapCoverageMap` on the result is a no-op.
    ///
    /// A coverage point whose positions have no mapping in `inputSourceMap` is
    /// not instrumented at all: it gets neither a `statementMap` / `fnMap` /
    /// `branchMap` entry nor a counter in the emitted `code`, so the runtime
    /// `__coverage__` object and the emitted counters always agree. Has no
    /// effect when `inputSourceMap` is unset or unusable.
    ///
    /// Defaults to `false`.
    pub compose_input_source_map: Option<bool>,
    /// Track truthy-value counts (`bT`) for logical expression operands.
    /// Defaults to `false`.
    pub report_logic: Option<bool>,
    /// Track receiver-safe optional-chaining (`?.`) links as branches.
    /// Receiver-bound optional calls stay native to preserve `this`.
    ///
    /// When `false`, optional chains are left native: no `_oc` helper is emitted
    /// and no `optional-chain` branches are registered. This matches
    /// `istanbul-lib-instrument`, which does not track `?.` as a branch, and
    /// avoids the per-operand helper call in optional-chain-dense hot paths.
    /// Statement and other branch coverage are unaffected.
    ///
    /// Defaults to `true`.
    pub track_optional_chain_branches: Option<bool>,
    /// Class method names to exclude from coverage instrumentation.
    pub ignore_class_methods: Option<Vec<String>>,
    /// Run the TypeScript-strip pass before instrumentation.
    ///
    /// Set this when passing raw TypeScript that has not been pre-transformed by
    /// Babel / tsc / esbuild. When `false`, raw TypeScript passes straight
    /// through: the output contains TypeScript syntax and is not executable as
    /// JavaScript, and no error is returned.
    ///
    /// Defaults to `false`, for callers that supply already-transformed
    /// JavaScript.
    pub strip_typescript: Option<bool>,
    /// Lower TypeScript `experimentalDecorators` syntax (`@Injectable()` /
    /// `@Controller()` style) into runtime `_decorate(...)` calls.
    ///
    /// Mirrors the `experimentalDecorators` flag in `tsconfig.json`. The
    /// instrumented output imports from `@oxc-project/runtime` at execution
    /// time. Has no effect unless `stripTypescript` is also `true`.
    ///
    /// Defaults to `false`.
    pub experimental_decorators: Option<bool>,
    /// Emit TypeScript decorator metadata (`design:type`, `design:paramtypes`,
    /// `design:returntype`) alongside each decorated class, method and property.
    ///
    /// Mirrors the `emitDecoratorMetadata` flag in `tsconfig.json`. Required by
    /// NestJS dependency injection, TypeORM column type inference and
    /// class-validator.
    ///
    /// Requires `experimentalDecorators: true`; the pair
    /// `(experimentalDecorators: false, emitDecoratorMetadata: true)` throws a
    /// JS `Error` rather than being silently promoted. The instrumented output
    /// imports from `@oxc-project/runtime` at execution time. Has no effect
    /// unless `stripTypescript` is also `true`.
    ///
    /// Defaults to `false`.
    pub emit_decorator_metadata: Option<bool>,
    /// Whether the source is compiled under `strictNullChecks`. Mirrors the
    /// `strictNullChecks` flag in `tsconfig.json`.
    ///
    /// Only consulted when `emitDecoratorMetadata` is `true`, where it decides
    /// how a nullable union is written into the emitted `design:type`. With
    /// `true`, `foo: string | null` emits `Object`, matching what `tsc` does
    /// under `strictNullChecks`. With `false`, `null` and `undefined` are elided
    /// from the union first, so the same property emits `String`.
    ///
    /// A mismatch is silent: the code still runs, but NestJS dependency
    /// injection, TypeORM column inference and class-validator read that
    /// metadata and see a different type than `tsc` produced. Set it to match
    /// the `tsconfig.json` the source is compiled with.
    ///
    /// Defaults to `true`, matching `tsc` under `strict`.
    pub strict_null_checks: Option<bool>,
    /// Attach an `x_fallow_functionMap` overlay to the returned coverage map.
    ///
    /// The overlay carries a stable `fallow:fn:<hex>` identity per function,
    /// keyed by the same ids as `fnMap` and derived from
    /// `(path, name, decl span, loc span)`. Istanbul consumers ignore the
    /// `x_`-prefixed field; downstream code quality tools use it as a long-lived
    /// join key.
    ///
    /// Defaults to `false`, which keeps the JSON output byte-identical to what
    /// Istanbul consumers expect.
    pub function_identity_overlay: Option<bool>,
    /// Name an otherwise-anonymous callback argument after its callee.
    ///
    /// `arr.map(cb)` -> `"map"`,
    /// `el.addEventListener("click", () => {})` -> `"addEventListener"`,
    /// `new Promise((res) => {})` -> `"Promise"`.
    ///
    /// `istanbul-lib-instrument` leaves these `(anonymous_N)`. Names inferred
    /// from a binding (variable declarator, property key, assignment target,
    /// default value) still win; this only replaces the `(anonymous_N)` fallback
    /// with a callee-derived name, which is also stable across rebuilds because
    /// the `(anonymous_N)` counter renumbers. Only the callee is used, never a
    /// sibling string argument.
    ///
    /// Defaults to `false`.
    pub name_callback_arguments: Option<bool>,
    /// Record the consequent as the location of arm 0 of an `if` branch.
    ///
    /// `istanbul-lib-instrument` records the whole `if` statement for arm 0.
    /// With this option, arm 0 of `if (a) { b(); } else { c(); }` is
    /// `{ b(); }`. Arm 1 is the alternate, or a zero-width location at the
    /// consequent end when there is no `else`, with or without this option.
    /// The `loc` of the branch stays the whole `if` statement, and no counter
    /// changes. `compat: 'istanbul'` ignores this option.
    ///
    /// Defaults to `false`.
    pub if_consequent_location: Option<bool>,
}

/// Options for `remapCoverageMap` and `remapCoverageMapWithLoader`.
#[napi(object)]
pub struct RemapOptions {
    /// Prune statement / function / branch entries whose positions cannot be
    /// looked up in the source map, along with their matching `s` / `f` / `b` /
    /// `bT` hit-count slots.
    ///
    /// Drop semantics mirror `istanbul-lib-source-maps`'s `transformer.js`:
    /// statements drop when start or end fails to remap; functions drop when any
    /// of `decl` / `loc` start or end fails (a matching `x_fallow_functionMap`
    /// overlay entry drops with the function); branch arms drop per arm, and the
    /// whole branch drops when no arms survive or retained mapped arms resolve
    /// to different sources. Branch ownership comes from those retained arms,
    /// and an unmapped umbrella `loc` falls back to the first retained arm.
    ///
    /// Defaults to `false`, which keeps unmapped entries at their
    /// generated-output positions.
    pub drop_unmapped: Option<bool>,
}

/// A coverage pragma comment that was found but not handled.
#[napi(object)]
pub struct UnhandledPragma {
    /// The full comment text.
    pub comment: String,
    /// 1-based line number.
    pub line: u32,
    /// 0-based column.
    pub column: u32,
}

/// Result of instrumenting a source file.
#[napi(object)]
pub struct InstrumentResult {
    /// The instrumented source code with coverage counters injected.
    pub code: String,
    /// Istanbul-compatible coverage map as a JSON string.
    /// Parse with `JSON.parse()` to get the coverage object.
    pub coverage_map: String,
    /// Output source map JSON string. Present only when the `sourceMap` option
    /// is `true`.
    pub source_map: Option<String>,
    /// Unhandled pragma comments found during instrumentation.
    pub unhandled_pragmas: Vec<UnhandledPragma>,
}

/// Instrument a JavaScript or TypeScript source file for coverage collection.
///
/// Parses `source` with Oxc, injects Istanbul-compatible coverage counters via
/// AST mutation, and returns the instrumented code with a coverage map.
///
/// # Errors
///
/// Returns an error if:
///   * `emitDecoratorMetadata` is `true` while `experimentalDecorators` is not
///   * `coverageVariable` is not a safe standalone JavaScript identifier
///   * `source` fails to parse
///   * the TypeScript-strip pass reports a diagnostic
///   * generated output cannot place the coverage setup after directives
#[napi(
    ts_args_type = "source: string, filename: string, options?: InstrumentOptions | undefined | null"
)]
pub fn instrument(
    source: String,
    filename: String,
    options: Option<serde_json::Value>,
) -> napi::Result<InstrumentResult> {
    let options =
        options.map(serde_json::from_value::<InstrumentOptions>).transpose().map_err(|error| {
            napi::Error::new(
                napi::Status::InvalidArg,
                format!("invalid instrument options: {error}"),
            )
        })?;
    let opts = core_instrument_options_from(options)?;
    let result =
        oxc_coverage_instrument::instrument(&source, &filename, &opts).map_err(generic_failure)?;

    let unhandled_pragmas = result
        .unhandled_pragmas
        .into_iter()
        .map(|p| UnhandledPragma { comment: p.comment, line: p.line, column: p.column })
        .collect();

    Ok(InstrumentResult {
        code: result.code,
        coverage_map: result.coverage_map_json,
        source_map: result.source_map,
        unhandled_pragmas,
    })
}

/// Remap a `coverage-final.json`-shaped JSON string through each entry's
/// embedded `inputSourceMap`.
///
/// Entries without an `inputSourceMap` are returned unchanged under their
/// original key. Entries with one are walked through the map and re-keyed by the
/// original source path, with `sourceRoot` joined per `istanbul-lib-source-maps`
/// semantics. A generated file that maps to several original sources fans out to
/// one entry per source; entries from several chunks that resolve to the same
/// path merge by Istanbul location identity and sum their counters. Equivalent
/// to `createSourceMapStore().transformCoverage(coverageMap)` in the Vitest
/// istanbul reporter path.
///
/// Every returned entry satisfies the Istanbul merge invariant
/// `keys(s) ⊆ keys(statementMap)`, and the same for `f`/`fnMap` and
/// `b`/`bT`/`branchMap`: an orphan counter, an `s`/`f`/`b` key with no matching
/// location-map entry, is dropped rather than passed through. Such an orphan
/// crashes `istanbul-lib-coverage`'s `CoverageMap.merge`, and therefore
/// `nyc report`, with "Cannot destructure property 'start' of 'undefined'"
/// ([#107](https://github.com/fallow-rs/oxc-coverage/issues/107)). It
/// can reach the input when an upstream instrumenter incremented a counter whose
/// map slot was later pruned. Dropping it is a no-op on already-consistent
/// coverage.
///
/// For nyc's disk-read flow (Mode A fallback) use
/// [`remap_coverage_map_with_loader`] and supply a `Record<string, string>` of
/// preloaded maps keyed by `FileCoverage` path.
///
/// An unmapped entry in a multi-source map is omitted whatever `options` says,
/// because it has no safe original owner. Pass `{ dropUnmapped: true }` to prune
/// unmappable entries in the single-source case too; see [`RemapOptions`] for
/// the per-kind semantics.
///
/// # Errors
///
/// Returns an error if `coverage_json` is not an Istanbul `CoverageMap`, or if
/// the remapped map fails to serialize.
#[napi]
pub fn remap_coverage_map(
    coverage_json: String,
    options: Option<RemapOptions>,
) -> napi::Result<String> {
    let parsed = oxc_coverage_instrument::parse_coverage_map(&coverage_json)
        .map_err(|e| invalid_coverage_json_error(&coverage_json, e))?;
    let core_options = core_remap_options_from(options);
    let remapped = oxc_coverage_instrument::remap_coverage_map_with_options(&parsed, core_options);
    serde_json::to_string(&remapped).map_err(generic_failure)
}

/// Like [`remap_coverage_map`], but with a preloaded map dictionary used as the
/// Mode A disk-read fallback.
///
/// Entries whose path is a key in `source_maps` and which carry no embedded
/// `inputSourceMap` use the dictionary's value as the source map JSON. Each
/// value must be a valid source map JSON string; an entry whose value fails to
/// parse passes through unremapped.
///
/// The dictionary form matches the Jest / nyc / istanbul JS workflow, where the
/// caller has already read the maps from disk before calling the converter.
/// [`oxc_coverage_instrument::SourceMapStore`] (Mode B continuous remap) is not
/// exposed to JS.
///
/// # Errors
///
/// Returns an error if `coverage_json` is not an Istanbul `CoverageMap`, or if
/// the remapped map fails to serialize.
#[napi]
pub fn remap_coverage_map_with_loader(
    coverage_json: String,
    source_maps: HashMap<String, String>,
    options: Option<RemapOptions>,
) -> napi::Result<String> {
    let parsed = oxc_coverage_instrument::parse_coverage_map(&coverage_json)
        .map_err(|e| invalid_coverage_json_error(&coverage_json, e))?;
    let core_options = core_remap_options_from(options);
    let mut store = oxc_coverage_instrument::SourceMapStore::new();
    for (path, coverage) in &parsed {
        if coverage.input_source_map.is_none()
            && let Some(json) = source_maps.get(path)
        {
            store.add_map_json(path.clone(), json);
        }
    }
    let remapped = store.transform_coverage_map_with_options(&parsed, core_options);
    serde_json::to_string(&remapped).map_err(generic_failure)
}

/// Convert V8 UTF-16 range coverage into Istanbul `FileCoverage` JSON.
///
/// `v8FunctionsJson` is the JSON array shape that the V8 inspector emits under
/// `Profiler.takePreciseCoverage().result[].functions`, the same shape Node's
/// `--experimental-coverage` and `@vitest/coverage-v8` consume.
///
/// `wrapperLength` is an explicit UTF-16 code-unit base for producers that
/// report wrapper-shifted ranges. It defaults to 0, which is correct for
/// source-relative Node inspector coverage.
///
/// Returns a JSON object compatible with Istanbul's `FileCoverage`. Statement,
/// function and branch counts come from the V8 ranges.
///
/// A branch arm with its own V8 range gets the count of that range. V8
/// removes a range whose count equals the count of its parent range, so an arm
/// with no range gets a count from its parent:
///
/// - A `cond-expr` arm, or the implicit `else` of an `if`, gets the count of the
///   smallest range that contains the start of the branch, minus the counts of
///   the other arms.
/// - An `if` body, a `switch` case, a `binary-expr` operand and a `default-arg`
///   value get the count of the smallest range that contains the arm.
///
/// V8 has no range for a default value, so a `default-arg` arm gets the call
/// count of its function, not the number of calls that used the default. An
/// `optional-chain` arm with no range gets 0. V8 has no range for the right
/// side of a logical assignment (`||=`, `&&=`, `??=`), so that arm gets 0.
/// When a pragma removes one arm
/// of a `cond-expr` or an `if`, the count of that arm is unknown: the other
/// `cond-expr` arm gets the count of the smallest range that contains it, and
/// an implicit `else` gets 0.
///
/// A count comes only from a block-coverage record. When the smallest
/// container is a function-only range (`isBlockCoverage: false`), the arm
/// gets 0.
///
/// When the source ends with a base64 or percent-encoded
/// `//# sourceMappingURL=data:application/json;...` trailer, the embedded map is
/// decoded and attached to the result as `inputSourceMap`. For external
/// `//# sourceMappingURL=foo.js.map` references, use
/// [`v8_to_istanbul_with_loader`] and pass a dictionary of URL to map JSON.
///
/// If the returned object has `inputSourceMap` set, chain `remapCoverageMap`
/// next to resolve coverage positions back to the original source; otherwise the
/// inline map rides along and downstream JS reporters that also call into
/// `istanbul-lib-source-maps` may double-remap.
///
/// # Errors
///
/// Returns an error if `v8_functions_json` is not a V8 function-coverage array,
/// if `source` fails to parse, or if the result fails to serialize.
#[napi]
pub fn v8_to_istanbul(
    source: String,
    filename: String,
    v8_functions_json: String,
    wrapper_length: Option<u32>,
) -> napi::Result<String> {
    let functions = parse_v8_functions(&v8_functions_json)?;
    let result = oxc_coverage_instrument::v8_to_istanbul(
        &source,
        &filename,
        &functions,
        wrapper_length.unwrap_or(0),
    )
    .map_err(generic_failure)?;
    serde_json::to_string(&result).map_err(generic_failure)
}

/// Like [`v8_to_istanbul`], but resolves external `//# sourceMappingURL=`
/// references from the preloaded `external_source_maps` dictionary.
///
/// The key is the URL as it appears in the source's trailing comment, for
/// example `foo.js.map`; the value is the map's JSON content. If the source has
/// an inline data-URL map the dictionary is not consulted. An entry whose value
/// fails to parse leaves `inputSourceMap` unset.
///
/// # Errors
///
/// Returns an error if `v8_functions_json` is not a V8 function-coverage array,
/// if `source` fails to parse, or if the result fails to serialize.
#[napi]
pub fn v8_to_istanbul_with_loader(
    source: String,
    filename: String,
    v8_functions_json: String,
    external_source_maps: HashMap<String, String>,
    wrapper_length: Option<u32>,
) -> napi::Result<String> {
    let functions = parse_v8_functions(&v8_functions_json)?;
    let result = oxc_coverage_instrument::v8_to_istanbul_with_loader(
        &source,
        &filename,
        &functions,
        wrapper_length.unwrap_or(0),
        |url| external_source_maps.get(url).cloned(),
    )
    .map_err(generic_failure)?;
    serde_json::to_string(&result).map_err(generic_failure)
}

fn generic_failure<E: fmt::Display>(err: E) -> napi::Error {
    napi::Error::new(napi::Status::GenericFailure, err.to_string())
}

fn parse_v8_functions(
    json: &str,
) -> napi::Result<Vec<oxc_coverage_instrument::V8FunctionCoverage>> {
    serde_json::from_str(json).map_err(|e| {
        napi::Error::new(napi::Status::InvalidArg, format!("invalid V8 functions JSON: {e}"))
    })
}

/// Build a `napi::Error` for unparsable coverage JSON, adding a hint when the
/// input is a single `FileCoverage` rather than a `CoverageMap`.
///
/// The raw serde error names the inner struct ("expected struct FileCoverage"),
/// so it reads as a `FileCoverage` shape problem when the real problem is the
/// outer container.
fn invalid_coverage_json_error<E: fmt::Display>(coverage_json: &str, err: E) -> napi::Error {
    let hint = if looks_like_single_file_coverage(coverage_json) {
        " (hint: input parses as a single FileCoverage; the remap API \
         expects an Istanbul CoverageMap shape `{[path]: FileCoverage}`. \
         Wrap with `{ [fc.path]: fc }` before calling.)"
    } else {
        ""
    };
    napi::Error::new(napi::Status::InvalidArg, format!("invalid coverage JSON: {err}{hint}"))
}

/// Whether `coverage_json` looks like a single `FileCoverage` rather than a
/// `CoverageMap`.
///
/// Checks the shape of the parsed [`serde_json::Value`] instead of deserializing
/// into `FileCoverage`: full `FileCoverage` deserialization diverges between the
/// native and `wasm32-wasi` bindings, so a deserialization-based check silently
/// drops the hint on wasi.
fn looks_like_single_file_coverage(coverage_json: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(coverage_json) else {
        return false;
    };
    let Some(obj) = value.as_object() else {
        return false;
    };
    obj.get("path").is_some_and(serde_json::Value::is_string)
        && obj.contains_key("statementMap")
        && obj.contains_key("fnMap")
        && obj.contains_key("branchMap")
}

/// Map the two JS-facing decorator booleans onto [`DecoratorMode`].
///
/// `(experimental = false, metadata = true)` is rejected: the upstream
/// `oxc_transformer` decorator pass is gated on legacy mode, so metadata without
/// decorator lowering has no effect.
fn decorator_mode_from_flags(experimental: bool, metadata: bool) -> napi::Result<DecoratorMode> {
    match (experimental, metadata) {
        (false, false) => Ok(DecoratorMode::PassThrough),
        (true, false) => Ok(DecoratorMode::Experimental),
        (true, true) => Ok(DecoratorMode::ExperimentalWithMetadata),
        (false, true) => Err(napi::Error::new(
            napi::Status::InvalidArg,
            "emitDecoratorMetadata: true requires experimentalDecorators: true",
        )),
    }
}

fn core_remap_options_from(options: Option<RemapOptions>) -> CoreRemapOptions {
    CoreRemapOptions {
        drop_unmapped: options.and_then(|options| options.drop_unmapped).unwrap_or(false),
    }
}

fn core_instrument_options_from(
    options: Option<InstrumentOptions>,
) -> napi::Result<oxc_coverage_instrument::InstrumentOptions> {
    let Some(options) = options else {
        return Ok(oxc_coverage_instrument::InstrumentOptions::default());
    };

    let experimental = options.experimental_decorators.unwrap_or(false);
    let metadata = options.emit_decorator_metadata.unwrap_or(false);
    let decorator_mode = decorator_mode_from_flags(experimental, metadata)?;
    Ok(oxc_coverage_instrument::InstrumentOptions {
        compat: options.compat.map(Into::into),
        source_type: options.source_type.map(Into::into),
        coverage_variable: options.coverage_variable.unwrap_or_else(|| "__coverage__".to_string()),
        source_map: options.source_map.unwrap_or(false),
        input_source_map: options.input_source_map,
        compose_input_source_map: options.compose_input_source_map.unwrap_or(false),
        report_logic: options.report_logic.unwrap_or(false),
        track_optional_chain: options.track_optional_chain_branches.unwrap_or(true),
        ignore_class_methods: options.ignore_class_methods.unwrap_or_default(),
        strip_typescript: options.strip_typescript.unwrap_or(false),
        decorator_mode,
        strict_null_checks: options.strict_null_checks.unwrap_or(true),
        function_identity_overlay: options.function_identity_overlay.unwrap_or(false),
        name_callback_arguments: options.name_callback_arguments.unwrap_or(false),
        if_consequent_location: options.if_consequent_location.unwrap_or(false),
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use oxc_coverage_instrument::{CompatProfile as CoreCompatProfile, DecoratorMode};

    use super::{
        InstrumentOptions, RemapOptions, core_instrument_options_from, core_remap_options_from,
        decorator_mode_from_flags, invalid_coverage_json_error, looks_like_single_file_coverage,
        remap_coverage_map, remap_coverage_map_with_loader,
    };

    const SINGLE_FC: &str =
        r#"{"path":"app.js","statementMap":{},"fnMap":{},"branchMap":{},"s":{},"f":{},"b":{}}"#;
    const COVERAGE_MAP: &str = r#"{"app.js":{"path":"app.js","statementMap":{},"fnMap":{},"branchMap":{},"s":{},"f":{},"b":{}}}"#;
    const ONE_LINE_COVERAGE_MAP: &str = r#"{
        "intermediate.js": {
            "path": "intermediate.js",
            "statementMap": {
                "0": {
                    "start": {"line": 1, "column": 0},
                    "end": {"line": 1, "column": 1}
                }
            },
            "fnMap": {},
            "branchMap": {},
            "s": {"0": 1},
            "f": {},
            "b": {}
        }
    }"#;
    const ONE_LINE_SOURCE_MAP: &str = r#"{"version":3,"sources":["src/app.ts"],"sourcesContent":["x"],"mappings":"AAAA","names":[]}"#;

    fn empty_options() -> InstrumentOptions {
        InstrumentOptions {
            compat: None,
            source_type: None,
            coverage_variable: None,
            source_map: None,
            input_source_map: None,
            compose_input_source_map: None,
            report_logic: None,
            track_optional_chain_branches: None,
            ignore_class_methods: None,
            strip_typescript: None,
            experimental_decorators: None,
            emit_decorator_metadata: None,
            strict_null_checks: None,
            function_identity_overlay: None,
            name_callback_arguments: None,
            if_consequent_location: None,
        }
    }

    #[test]
    fn detects_single_file_coverage() {
        assert!(looks_like_single_file_coverage(SINGLE_FC));
    }

    #[test]
    fn rejects_coverage_map_outer_container() {
        // The outer object's `"app.js"` value is a `FileCoverage`, not a string,
        // so the `path` string check fails on the outer object.
        assert!(!looks_like_single_file_coverage(COVERAGE_MAP));
    }

    #[test]
    fn rejects_non_object_root() {
        assert!(!looks_like_single_file_coverage("[]"));
        assert!(!looks_like_single_file_coverage("\"hello\""));
        assert!(!looks_like_single_file_coverage("42"));
        assert!(!looks_like_single_file_coverage("null"));
    }

    #[test]
    fn rejects_invalid_json() {
        assert!(!looks_like_single_file_coverage("{ not json"));
        assert!(!looks_like_single_file_coverage(""));
    }

    #[test]
    fn rejects_object_missing_required_keys() {
        // A bare `path` is ambiguous: every CoverageMap key is a path too.
        assert!(!looks_like_single_file_coverage(r#"{"path":"app.js"}"#));
        assert!(!looks_like_single_file_coverage(
            r#"{"statementMap":{},"fnMap":{},"branchMap":{}}"#
        ));
        assert!(!looks_like_single_file_coverage(
            r#"{"path":42,"statementMap":{},"fnMap":{},"branchMap":{}}"#,
        ));
    }

    #[test]
    fn invalid_coverage_json_error_omits_hint_for_coverage_map_shape() {
        let err = invalid_coverage_json_error(COVERAGE_MAP, "bad inner entry");
        let message = err.to_string();
        assert!(message.contains("invalid coverage JSON: bad inner entry"));
        assert!(!message.contains("input parses as a single FileCoverage"));
    }

    #[test]
    fn remap_coverage_map_reports_single_file_hint() {
        let err = remap_coverage_map(SINGLE_FC.to_string(), None)
            .expect_err("single FileCoverage is not a CoverageMap");
        let message = err.to_string();
        assert!(message.contains("input parses as a single FileCoverage"));
        assert!(message.contains("Wrap with `{ [fc.path]: fc }` before calling"));
    }

    #[test]
    fn loader_remap_reports_a_function_neutral_single_file_hint() {
        let err = remap_coverage_map_with_loader(SINGLE_FC.to_string(), HashMap::new(), None)
            .expect_err("single FileCoverage is not a CoverageMap");
        let message = err.to_string();

        assert!(message.contains("the remap API expects an Istanbul CoverageMap shape"));
        assert!(!message.contains("remapCoverageMap expects"));
    }

    #[test]
    fn loader_remap_rekeys_entries_by_original_source() {
        let mut source_maps = HashMap::new();
        source_maps.insert("intermediate.js".to_string(), ONE_LINE_SOURCE_MAP.to_string());

        let remapped = remap_coverage_map_with_loader(
            ONE_LINE_COVERAGE_MAP.to_string(),
            source_maps,
            Some(RemapOptions { drop_unmapped: Some(false) }),
        )
        .expect("loader-backed remap succeeds");
        let value: serde_json::Value =
            serde_json::from_str(&remapped).expect("remapped coverage is valid JSON");

        assert!(value.get("src/app.ts").is_some(), "entry is rekeyed by source path");
        assert!(
            value["src/app.ts"].get("inputSourceMap").is_none(),
            "consumed source map is not serialized back",
        );
    }

    #[test]
    fn decorator_flags_map_to_core_modes() {
        assert_eq!(
            decorator_mode_from_flags(false, false).expect("pass-through mode"),
            DecoratorMode::PassThrough,
        );
        assert_eq!(
            decorator_mode_from_flags(true, false).expect("experimental mode"),
            DecoratorMode::Experimental,
        );
        assert_eq!(
            decorator_mode_from_flags(true, true).expect("metadata mode"),
            DecoratorMode::ExperimentalWithMetadata,
        );
    }

    #[test]
    fn remap_options_default_to_keep_unmapped() {
        assert!(!core_remap_options_from(None).drop_unmapped);
        assert!(!core_remap_options_from(Some(RemapOptions { drop_unmapped: None })).drop_unmapped);
        assert!(
            !core_remap_options_from(Some(RemapOptions { drop_unmapped: Some(false) }))
                .drop_unmapped
        );
        assert!(
            core_remap_options_from(Some(RemapOptions { drop_unmapped: Some(true) })).drop_unmapped
        );
    }

    #[test]
    fn core_options_fill_defaults() {
        let opts = core_instrument_options_from(None).expect("default options");
        assert_eq!(opts.compat, None);
        assert_eq!(opts.coverage_variable, "__coverage__");
        assert!(!opts.source_map);
        assert!(!opts.compose_input_source_map);
        assert!(!opts.report_logic);
        assert!(opts.track_optional_chain);
        assert_eq!(opts.ignore_class_methods, Vec::<String>::new());
        assert!(!opts.strip_typescript);
        assert_eq!(opts.decorator_mode, DecoratorMode::PassThrough);
        assert!(!opts.function_identity_overlay);
        assert!(!opts.name_callback_arguments);
        assert!(!opts.if_consequent_location);
    }

    #[test]
    fn core_options_map_js_flags() {
        let mut input = empty_options();
        input.compat = Some(super::CompatProfile::Istanbul);
        input.coverage_variable = Some("__cov".to_string());
        input.source_map = Some(true);
        input.input_source_map = Some(r#"{"version":3}"#.to_string());
        input.compose_input_source_map = Some(true);
        input.report_logic = Some(true);
        input.track_optional_chain_branches = Some(false);
        input.ignore_class_methods = Some(vec!["render".to_string(), "toJSON".to_string()]);
        input.strip_typescript = Some(true);
        input.experimental_decorators = Some(true);
        input.emit_decorator_metadata = Some(true);
        input.function_identity_overlay = Some(true);
        input.name_callback_arguments = Some(true);
        input.if_consequent_location = Some(true);

        let opts = core_instrument_options_from(Some(input)).expect("mapped options");
        assert_eq!(opts.compat, Some(CoreCompatProfile::Istanbul));
        assert_eq!(opts.coverage_variable, "__cov");
        assert!(opts.source_map);
        assert_eq!(opts.input_source_map.as_deref(), Some(r#"{"version":3}"#));
        assert!(opts.compose_input_source_map);
        assert!(opts.report_logic);
        assert!(!opts.track_optional_chain);
        assert_eq!(opts.ignore_class_methods, ["render", "toJSON"]);
        assert!(opts.strip_typescript);
        assert_eq!(opts.decorator_mode, DecoratorMode::ExperimentalWithMetadata);
        assert!(opts.function_identity_overlay);
        assert!(opts.name_callback_arguments);
        assert!(opts.if_consequent_location);
    }

    #[test]
    fn core_options_reject_metadata_without_experimental_decorators() {
        let mut input = empty_options();
        input.emit_decorator_metadata = Some(true);

        let err = core_instrument_options_from(Some(input)).expect_err("invalid decorator flags");
        assert_eq!(
            err.to_string(),
            "InvalidArg, emitDecoratorMetadata: true requires experimentalDecorators: true",
        );
    }
}
