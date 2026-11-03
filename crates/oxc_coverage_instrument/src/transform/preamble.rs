//! The per-file coverage preamble: the hoisted function that installs the
//! coverage object on the global, the runtime helpers it declares, and the
//! path-derived identifier and hash it is keyed by.

use std::fmt::Write;

use oxc_coverage_types::FileCoverage;

/// Inputs to [`generate_preamble_source`], grouped so the generator stays
/// at a single parameter even as new options accrete.
pub struct PreambleInputs<'a> {
    /// The full coverage map for the file (used for the `path` field only).
    pub coverage: &'a FileCoverage,
    /// Pre-serialized JSON of `coverage`, embedded as the `coverageData` literal.
    pub coverage_json: &'a str,
    /// Stable hex hash of `coverage_json` used by Istanbul's stale-cache guard.
    pub coverage_hash: &'a str,
    /// Name of the global coverage variable (default `__coverage__`).
    pub coverage_var: &'a str,
    /// Per-file coverage function name (e.g. `cov_<hash>`).
    pub cov_fn_name: &'a str,
    /// Whether to emit the truthy-value tracking helper (`_bt`).
    pub report_logic: bool,
    /// Whether to emit the optional-chain link observer (`_oc`).
    pub needs_optional_chain_helper: bool,
}

/// Generate the preamble as source text for insertion after the hashbang and
/// directive prologue in the emitted code.
///
/// The preamble is built as a string rather than as AST nodes, which is what
/// istanbul-lib-instrument does: the function is fixed apart from the
/// interpolated names and the embedded coverage literal.
///
/// The coverage object comes from a hoisted function declaration, not from a
/// `var` initializer, and every counter calls it (`cov_fn().s[0]++`). A hoisted
/// function of the module can run before the module body, for example through
/// an ES module import cycle. The first call installs the coverage object and
/// then replaces the function with a getter for that object.
pub fn generate_preamble_source(inputs: &PreambleInputs<'_>) -> String {
    let PreambleInputs {
        coverage,
        coverage_json,
        coverage_hash,
        coverage_var,
        cov_fn_name,
        report_logic,
        needs_optional_chain_helper,
    } = *inputs;
    // Both `serde_json::to_string` calls below serialize a plain string, and
    // the full coverage map arrives already serialized as `coverage_json`, so
    // no JSON error is reachable from here.
    let mut buf = String::with_capacity(256 + coverage_json.len());
    let _ = write!(buf, "function {cov_fn_name}() {{ var path = ");
    buf.push_str(
        &serde_json::to_string(&coverage.path).expect("serializing a String to JSON is infallible"),
    );
    let _ = write!(buf, "; var hash = ");
    buf.push_str(
        &serde_json::to_string(coverage_hash).expect("serializing a &str to JSON is infallible"),
    );
    let _ = write!(buf, "; var gcv = '{coverage_var}'; var coverageData = ");
    buf.push_str(coverage_json);
    buf.push_str(
        "; coverageData.hash = hash; var coverage = typeof globalThis !== 'undefined' ? globalThis : typeof global !== 'undefined' ? global : typeof self !== 'undefined' ? self : this; if (!coverage[gcv]) { coverage[gcv] = {}; } "
    );
    // A path that is literally `__proto__` cannot be installed with
    // `coverage[gcv][path] = ...`: that write would reach `Object.prototype`
    // instead of adding a key, so it goes through `Object.defineProperty`.
    if coverage.path == "__proto__" {
        buf.push_str(
            "if (!Object.prototype.hasOwnProperty.call(coverage[gcv], path) || !coverage[gcv][path] || coverage[gcv][path].hash !== hash) { Object.defineProperty(coverage[gcv], path, { value: coverageData, enumerable: true, writable: true, configurable: true }); }"
        );
    } else {
        buf.push_str("if (!coverage[gcv][path] || coverage[gcv][path].hash !== hash) { coverage[gcv][path] = coverageData; }");
    }
    let _ = writeln!(
        buf,
        " var actualCoverage = coverage[gcv][path]; {cov_fn_name} = function () {{ return actualCoverage; }}; return actualCoverage; }}\n{cov_fn_name}();"
    );
    if report_logic {
        append_logic_helper(&mut buf, cov_fn_name);
    }
    if needs_optional_chain_helper {
        append_optional_chain_helper(&mut buf, cov_fn_name);
    }
    buf
}

/// Append the truthy-value tracker (`cov_fn_bt`). It counts values that are
/// truthy and, per istanbul's check, non-trivial: not an empty array and not an
/// empty plain object. A non-plain object such as a class instance always
/// counts.
fn append_logic_helper(buf: &mut String, cov_fn_name: &str) {
    let _ = writeln!(buf, "var {cov_fn_name}_temp;");
    let _ = writeln!(
        buf,
        "function {cov_fn_name}_bt(val, id, idx) {{ {cov_fn_name}_temp = val; if ({cov_fn_name}_temp && (!Array.isArray({cov_fn_name}_temp) || {cov_fn_name}_temp.length) && (Object.getPrototypeOf({cov_fn_name}_temp) !== Object.prototype || Object.values({cov_fn_name}_temp).length)) {{ ++{cov_fn_name}().bT[id][idx]; }} return {cov_fn_name}_temp; }}"
    );
}

/// Append the optional-chain link observer (`cov_fn_oc`). It bumps arm 0 when
/// the observed value is `null` or `undefined` (the link short-circuits) and
/// arm 1 otherwise, returning the input unchanged so native `?.` semantics are
/// preserved.
fn append_optional_chain_helper(buf: &mut String, cov_fn_name: &str) {
    let _ = writeln!(
        buf,
        "function {cov_fn_name}_oc(val, id) {{ ++{cov_fn_name}().b[id][val == null ? 0 : 1]; return val; }}"
    );
}

/// Stable DJB31 hex hash. Used for both the per-file coverage function name
/// and Istanbul's stale-cache guard hash on the embedded coverage object.
pub fn djb31_hex(input: &str) -> String {
    let mut hash: u64 = 0;
    for byte in input.bytes() {
        hash = hash.wrapping_mul(31).wrapping_add(u64::from(byte));
    }
    format!("{hash:x}")
}

/// Generate a deterministic coverage function name from the file path.
pub fn generate_cov_fn_name(file_path: &str) -> String {
    format!("cov_{}", djb31_hex(file_path))
}
