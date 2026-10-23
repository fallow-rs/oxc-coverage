//! Istanbul-compatible coverage data types.
//!
//! First-party serde types derived from Istanbul's JSON schema
//! (`@istanbuljs/schema`). Produces `coverage-final.json` compatible
//! output that Jest, Vitest, c8, nyc, and Codecov all consume.
//!
//! This crate is the data-model layer of the `oxc-coverage` suite. The
//! instrumenter, source-map remapper, V8-to-Istanbul converter, and reporters
//! all share these types.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize, ser::SerializeStruct};

/// A coverage pragma comment that was found but not handled.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnhandledPragma {
    /// The full comment text.
    pub comment: String,
    /// 1-based line number.
    pub line: u32,
    /// 0-based column.
    pub column: u32,
}

/// Coverage data for a single file. Serializes to Istanbul's `coverage-final.json` format.
///
/// The root `coverage-final.json` is a map of file paths to `FileCoverage` objects.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileCoverage {
    /// Absolute file path.
    #[serde(default, deserialize_with = "deserialize_null_as_empty")]
    pub path: String,
    /// Statement locations, keyed by sequential string IDs ("0", "1", ...).
    #[serde(rename = "statementMap")]
    pub statement_map: BTreeMap<String, Location>,
    /// Function metadata, keyed by sequential string IDs.
    #[serde(rename = "fnMap")]
    pub fn_map: BTreeMap<String, FnEntry>,
    /// Branch metadata, keyed by sequential string IDs.
    #[serde(rename = "branchMap")]
    pub branch_map: BTreeMap<String, BranchEntry>,
    /// Statement hit counts, keyed by the same IDs as `statement_map`.
    /// Istanbul allows `null` values (e.g., uninstrumented statements); these are coerced to `0`.
    #[serde(deserialize_with = "deserialize_null_as_zero_map")]
    pub s: BTreeMap<String, u32>,
    /// Function hit counts, keyed by the same IDs as `fn_map`.
    /// Istanbul allows `null` values; these are coerced to `0`.
    #[serde(deserialize_with = "deserialize_null_as_zero_map")]
    pub f: BTreeMap<String, u32>,
    /// Branch hit counts, keyed by the same IDs as `branch_map`.
    /// Each value is a Vec with one count per branch arm.
    /// Istanbul allows `null` values in both the Vec and individual elements; these are coerced to `0`.
    #[serde(deserialize_with = "deserialize_null_as_zero_vec_map")]
    pub b: BTreeMap<String, Vec<u32>>,
    /// Branch truthy tracking hit counts. Only present when `report_logic` is enabled.
    /// Tracks whether each operand in a logical expression evaluated to a truthy value.
    /// Keyed by the same IDs as `binary-expr` entries in `branch_map`.
    #[serde(
        rename = "bT",
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "deserialize_optional_null_as_zero_vec_map"
    )]
    pub b_t: Option<BTreeMap<String, Vec<u32>>>,
    /// Input source map from a prior transformation (e.g., TypeScript to JS).
    /// Stored so downstream tools can chain back to the original source.
    /// Only present when the instrumenter received an `inputSourceMap`.
    #[serde(rename = "inputSourceMap", skip_serializing_if = "Option::is_none")]
    pub input_source_map: Option<serde_json::Value>,
    /// Optional non-Istanbul extension: stable function identity overlay for
    /// downstream code-quality tools (Fallow et al.). Keyed by the same IDs
    /// as `fn_map`; ignored by standard Istanbul consumers because the field
    /// name is prefixed with `x_`. Present only when the instrumenter was
    /// invoked with `InstrumentOptions::function_identity_overlay = true`.
    /// See the "Function identity overlay" section of the workspace README.
    #[serde(rename = "x_fallow_functionMap", default, skip_serializing_if = "Option::is_none")]
    pub x_fallow_function_map: Option<BTreeMap<String, FunctionIdentity>>,
}

impl FileCoverage {
    /// Deserialize a `FileCoverage` from a JSON string.
    ///
    /// The input is a single entry of Istanbul's `coverage-final.json`, not the
    /// root map; use [`parse_coverage_map`] for the root map.
    ///
    /// # Errors
    ///
    /// Returns [`serde_json::Error`] if `json` is not valid JSON or is not an
    /// Istanbul `FileCoverage` object. A missing `statementMap`, `fnMap`,
    /// `branchMap`, `s`, `f` or `b` key is an error; `null` values inside them
    /// are not.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// Drop counter slots that have no matching location-map entry, enforcing
    /// the Istanbul invariant that every counter key is also a key of its
    /// location map: `keys(s) ⊆ keys(statementMap)`, `keys(f) ⊆ keys(fnMap)`,
    /// and `keys(b)` / `keys(bT) ⊆ keys(branchMap)`. The `x_fallow_functionMap`
    /// overlay is kept 1:1 with `fnMap` under the same rule. Returns the total
    /// number of orphan entries removed across every map (counter slots plus
    /// any orphaned overlay entries), so a non-zero result reliably means the
    /// input violated the invariant.
    ///
    /// An orphan counter (an `s`/`f`/`b` key whose location-map entry is
    /// missing) is fatal to `istanbul-lib-coverage`'s `CoverageMap.merge`,
    /// which `nyc` calls to merge sharded coverage: `mergeProp` looks up
    /// `statementMap[key]` for every `s` key and passes it to `keyFromLoc`,
    /// which destructures `{ start }` of the `undefined` entry and throws
    /// `TypeError: Cannot destructure property 'start' of 'undefined'`. A
    /// single orphan anywhere aborts the whole `nyc report` step
    /// ([#107](https://github.com/fallow-rs/oxc-coverage/issues/107)).
    ///
    /// Orphans cannot arise from this crate's own instrumenter output (the
    /// counter maps are built from the location-map keys), but a coverage
    /// object collected at runtime can pick one up when an upstream
    /// instrumenter emitted a counter increment for a location that was later
    /// pruned from the map: `++cov.s[id]` against an absent slot evaluates
    /// `undefined + 1 = NaN`, which serializes back as `null` and reappears as
    /// an orphan key. Removing the orphan counter is the only valid
    /// reconciliation: its location cannot be reconstructed, and Istanbul
    /// consumers cannot merge it.
    pub fn prune_orphan_counters(&mut self) -> usize {
        let Self { statement_map, fn_map, branch_map, s, f, b, b_t, x_fallow_function_map, .. } =
            self;
        let mut removed = 0;
        removed += retain_known_keys(s, |k| statement_map.contains_key(k));
        removed += retain_known_keys(f, |k| fn_map.contains_key(k));
        removed += retain_known_keys(b, |k| branch_map.contains_key(k));
        if let Some(b_t) = b_t.as_mut() {
            removed += retain_known_keys(b_t, |k| branch_map.contains_key(k));
        }
        // A dangling overlay entry would leave downstream tools joining on
        // `fnMap` keys with an identity that has no function.
        if let Some(overlay) = x_fallow_function_map.as_mut() {
            removed += retain_known_keys(overlay, |k| fn_map.contains_key(k));
        }
        removed
    }
}

/// Stable function identity record.
///
/// Non-Istanbul extension consumed by downstream code-quality tools to join AST
/// inventories, runtime coverage, and source-mapped positions across runs
/// without reconstructing identity from `(path, name, line, column)` after the
/// fact.
///
/// `id` is `fallow:fn:<8 hex>`, bit-equal to
/// `fallow_cov_protocol::function_identity_id`, so the overlay serves as a
/// cross-surface join key against any other producer in the fallow ecosystem.
/// The derivation lives with the instrumenter that computes it.
///
/// Positions in `decl` / `loc` follow the same source as `FnEntry` in
/// `fn_map` (original TS when `strip_typescript` is on; instrumented JS
/// otherwise). The remap pipeline carries overlay entries through unchanged,
/// so consumers that remap through `inputSourceMap` must recompute identity
/// against the post-remap positions themselves.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionIdentity {
    /// Stable identity string: `fallow:fn:<8 hex>`.
    pub id: String,
    /// Function name. Mirrors `FnEntry::name` (anonymous functions use
    /// `"(anonymous_N)"`).
    pub name: String,
    /// File path as passed to `instrument()`. Mirrors `FileCoverage::path`.
    pub path: String,
    /// Declaration span. Mirrors `FnEntry::decl`.
    pub decl: Location,
    /// Body span (including braces). Mirrors `FnEntry::loc`.
    pub loc: Location,
}

/// A source location span with start and end positions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Location {
    /// First position of the span.
    pub start: Position,
    /// One position past the last character of the span.
    pub end: Position,
}

/// A 1-based line, 0-based column position.
///
/// Istanbul allows `null` or missing fields for line/column when the position
/// is unknown (e.g., empty `{}` objects for branch locations); these are
/// coerced to `0` on deserialization.
#[derive(Debug, Clone, Deserialize)]
pub struct Position {
    /// 1-based line number.
    #[serde(default, deserialize_with = "deserialize_null_as_zero")]
    pub line: u32,
    /// 0-based column number.
    #[serde(default, deserialize_with = "deserialize_null_as_zero")]
    pub column: u32,
}

impl Serialize for Position {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        // Istanbul writes `{}` for an unknown branch-arm position. `line` is
        // 1-based, so no real position lands here, and emitting
        // `{"line":0,"column":0}` instead would lose byte-parity with
        // `istanbul-lib-instrument` output.
        if self.line == 0 && self.column == 0 {
            return serializer.serialize_struct("Position", 0)?.end();
        }

        let mut state = serializer.serialize_struct("Position", 2)?;
        state.serialize_field("line", &self.line)?;
        state.serialize_field("column", &self.column)?;
        state.end()
    }
}

/// Function entry in the coverage map.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FnEntry {
    /// Function name. Anonymous functions use `"(anonymous_N)"`.
    #[serde(default, deserialize_with = "deserialize_null_as_empty")]
    pub name: String,
    /// 1-based line of the function declaration.
    #[serde(default, deserialize_with = "deserialize_null_as_zero")]
    pub line: u32,
    /// Span of the function declaration (keyword to name/params).
    pub decl: Location,
    /// Span of the function body.
    pub loc: Location,
}

/// Branch entry in the coverage map.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchEntry {
    /// Overall location of the branch construct.
    pub loc: Location,
    /// 1-based line where the branch starts.
    #[serde(default, deserialize_with = "deserialize_null_as_zero")]
    pub line: u32,
    /// Branch type: `"if"`, `"switch"`, `"cond-expr"`, `"binary-expr"`, `"default-arg"`.
    #[serde(rename = "type", default, deserialize_with = "deserialize_null_as_empty")]
    pub branch_type: String,
    /// One location per branch arm.
    pub locations: Vec<Location>,
}

/// The deserialized `coverage-final.json` shape: file path to [`FileCoverage`].
///
/// `BTreeMap` keeps the iteration order deterministic, so re-serializing a
/// coverage map yields byte-stable JSON.
pub type CoverageMap = BTreeMap<String, FileCoverage>;

/// Error returned by [`parse_coverage_map_validated`].
#[derive(Debug)]
#[non_exhaustive]
pub enum CoverageMapValidationError {
    /// The input is not valid Istanbul coverage JSON.
    Json(serde_json::Error),
    /// The outer map key has no non-empty `/`-separated path segment.
    PathlessOuterKey {
        /// The rejected outer map key, preserved verbatim.
        outer_path: String,
    },
    /// A non-empty `FileCoverage.path` differs from its outer map key.
    PathMismatch {
        /// The authoritative outer map key.
        outer_path: String,
        /// The conflicting path stored inside `FileCoverage`.
        inner_path: String,
    },
}

impl std::fmt::Display for CoverageMapValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Json(error) => write!(f, "invalid coverage JSON: {error}"),
            Self::PathlessOuterKey { outer_path } => {
                write!(f, "invalid coverage path {outer_path:?}: expected a non-empty path segment")
            }
            Self::PathMismatch { outer_path, inner_path } => write!(
                f,
                "coverage path mismatch: outer key {outer_path:?} differs from FileCoverage.path {inner_path:?}"
            ),
        }
    }
}

impl std::error::Error for CoverageMapValidationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Json(error) => Some(error),
            Self::PathlessOuterKey { .. } | Self::PathMismatch { .. } => None,
        }
    }
}

impl From<serde_json::Error> for CoverageMapValidationError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

/// Parse a `coverage-final.json` string into a map of file paths to coverage data.
///
/// This is the inverse of instrumentation: it reads existing coverage data
/// produced by any Istanbul-compatible tool (Jest, Vitest, c8, nyc, etc.).
///
/// # Errors
///
/// Returns [`serde_json::Error`] if `json` is not valid JSON or is not a map of
/// file path to Istanbul `FileCoverage` object. A missing `statementMap`,
/// `fnMap`, `branchMap`, `s`, `f` or `b` key is an error; `null` values inside
/// them are not.
///
/// # Example
///
/// ```
/// use oxc_coverage_types::parse_coverage_map;
///
/// let json = r#"{"file.js": {"path": "file.js", "statementMap": {}, "fnMap": {}, "branchMap": {}, "s": {}, "f": {}, "b": {}}}"#;
/// let map = parse_coverage_map(json).unwrap();
/// assert!(map.contains_key("file.js"));
/// ```
pub fn parse_coverage_map(json: &str) -> Result<CoverageMap, serde_json::Error> {
    serde_json::from_str(json)
}

/// Parse and validate a `coverage-final.json` string at an ingestion boundary.
///
/// Unlike [`parse_coverage_map`], this rejects outer keys without a reportable
/// `/`-separated path segment and enforces one path identity per entry. An
/// empty or `null` inner [`FileCoverage::path`] is normalized to the outer key;
/// a conflicting non-empty inner path is rejected.
///
/// Path equality is byte-for-byte. This function does not resolve relative
/// segments, convert platform separators, or access the filesystem.
///
/// # Errors
///
/// Returns [`CoverageMapValidationError::Json`] for JSON or schema errors,
/// [`CoverageMapValidationError::PathlessOuterKey`] for a key such as `"///"`,
/// and [`CoverageMapValidationError::PathMismatch`] for conflicting identities.
pub fn parse_coverage_map_validated(json: &str) -> Result<CoverageMap, CoverageMapValidationError> {
    let mut map = parse_coverage_map(json)?;
    for (outer_path, coverage) in &mut map {
        if !outer_path.split('/').any(|segment| !segment.is_empty()) {
            return Err(CoverageMapValidationError::PathlessOuterKey {
                outer_path: outer_path.clone(),
            });
        }
        if coverage.path.is_empty() {
            coverage.path.clone_from(outer_path);
        } else if coverage.path != *outer_path {
            return Err(CoverageMapValidationError::PathMismatch {
                outer_path: outer_path.clone(),
                inner_path: coverage.path.clone(),
            });
        }
    }
    Ok(map)
}

/// Coerce a `null` string to `""`.
///
/// Applied to the fields Istanbul may emit as `null`: `path`,
/// [`FnEntry::name`] and [`BranchEntry::branch_type`].
fn deserialize_null_as_empty<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<String>::deserialize(deserializer)?.unwrap_or_default())
}

/// Deserialize a `u32` where `null` is coerced to `0`.
///
/// Istanbul allows `null` for position fields (line, column) when the exact
/// location is unknown, and for individual hit counts in some edge cases.
fn deserialize_null_as_zero<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<u32>::deserialize(deserializer)?.unwrap_or(0))
}

/// Deserialize a `BTreeMap<String, u32>` where `null` values are coerced to `0`.
///
/// Istanbul coverage tools may emit `null` for hit counts when a statement or
/// function was never instrumented or the count is indeterminate.
fn deserialize_null_as_zero_map<'de, D>(deserializer: D) -> Result<BTreeMap<String, u32>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw: BTreeMap<String, Option<u32>> = Deserialize::deserialize(deserializer)?;
    Ok(raw.into_iter().map(|(k, v)| (k, v.unwrap_or(0))).collect())
}

/// Deserialize a `BTreeMap<String, Vec<u32>>` with null-tolerance at both
/// nesting levels.
///
/// A `null` Vec is coerced to an empty Vec and a `null` element to `0`.
fn deserialize_null_as_zero_vec_map<'de, D>(
    deserializer: D,
) -> Result<BTreeMap<String, Vec<u32>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw: BTreeMap<String, Option<Vec<Option<u32>>>> = Deserialize::deserialize(deserializer)?;
    Ok(zero_vec_map(raw))
}

/// Deserialize an `Option<BTreeMap<String, Vec<u32>>>` with null-tolerance.
///
/// An absent or `null` `bT` deserializes to `None`; a present map applies the
/// same Vec-level and element-level coercions as
/// [`deserialize_null_as_zero_vec_map`].
fn deserialize_optional_null_as_zero_vec_map<'de, D>(
    deserializer: D,
) -> Result<Option<BTreeMap<String, Vec<u32>>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt: Option<BTreeMap<String, Option<Vec<Option<u32>>>>> =
        Deserialize::deserialize(deserializer)?;
    Ok(opt.map(zero_vec_map))
}

fn zero_vec_map(raw: BTreeMap<String, Option<Vec<Option<u32>>>>) -> BTreeMap<String, Vec<u32>> {
    raw.into_iter()
        .map(|(k, v)| (k, v.unwrap_or_default().into_iter().map(|x| x.unwrap_or(0)).collect()))
        .collect()
}

/// Retain only the map entries whose key passes `keep`, returning how many were
/// removed.
fn retain_known_keys<V>(map: &mut BTreeMap<String, V>, keep: impl Fn(&str) -> bool) -> usize {
    let before = map.len();
    map.retain(|key, _| keep(key));
    before - map.len()
}
