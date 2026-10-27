//! Convert V8 UTF-16 range coverage into Istanbul `FileCoverage`.
//!
//! Thin orchestrator that combines this crate's AST-traversal pass
//! ([`crate::instrument::collect_for_v8_to_istanbul`]) with
//! `oxc_coverage_v8::apply_v8_coverage_with_static_members` for the V8-range intersection and
//! `oxc_coverage_v8`'s source-map extraction helpers for the inline /
//! external `//# sourceMappingURL=` trailer.
//!
//! For the user-facing description of the V8-to-Istanbul mapping, the CJS
//! wrapper offset semantics, and the per-arm `body_byte_span` resolution, see
//! the `oxc_coverage_v8` crate documentation.

pub use oxc_coverage_v8::{V8CoverageRange, V8FunctionCoverage};

use oxc_coverage_types::FileCoverage;
use oxc_coverage_v8::{
    apply_v8_coverage_with_static_members, extract_external_source_mapping_url,
    extract_inline_source_map,
};

use crate::instrument::collect_for_v8_to_istanbul;

/// Errors produced by the V8-to-Istanbul conversion.
#[derive(Debug)]
pub enum V8ToIstanbulError {
    /// The source could not be parsed.
    Parse(String),
}

impl std::fmt::Display for V8ToIstanbulError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse(msg) => write!(f, "parse error: {msg}"),
        }
    }
}

impl std::error::Error for V8ToIstanbulError {}

/// Convert V8 function coverage into Istanbul `FileCoverage`.
///
/// `wrapper_length_utf16` is an explicit UTF-16 code-unit base for coverage producers
/// that report wrapper-shifted ranges. Pass 0 for source-relative Node inspector
/// coverage.
///
/// Statement and function counts come from the smallest V8 range that contains
/// the location. A branch arm gets the count of its own V8 range. An arm with
/// no range gets a count from its parent range, as `apply_v8_coverage` in
/// `oxc_coverage_v8` describes. When the source carries an
/// inline `//# sourceMappingURL=data:application/json;base64,...` comment, the
/// embedded map is decoded and attached as `inputSourceMap` so the result
/// chains cleanly into [`crate::remap_coverage`].
///
/// A private class method (`#name() {}`) gets its own `fnMap` entry named
/// `#name`, because V8 reports a function range for it. Istanbul
/// instrumentation gives a private method no `fnMap` entry, so
/// [`crate::instrument()`] does not record one.
///
/// To resolve an external `//# sourceMappingURL=foo.js.map` reference rather
/// than the inline data-URL form, use [`v8_to_istanbul_with_loader`] and
/// supply a loader that reads the map JSON from disk (or fetches it).
///
/// # Errors
///
/// Returns [`V8ToIstanbulError::Parse`] if `source` cannot be parsed.
pub fn v8_to_istanbul(
    source: &str,
    filename: &str,
    functions: &[V8FunctionCoverage],
    wrapper_length_utf16: u32,
) -> Result<FileCoverage, V8ToIstanbulError> {
    v8_to_istanbul_with_loader(source, filename, functions, wrapper_length_utf16, |_| None)
}

/// Like [`v8_to_istanbul`], but with a loader for external `sourceMappingURL`
/// references.
///
/// When the source carries a `//# sourceMappingURL=` trailer that is not an
/// inline `data:application/json` URL, the loader is called with the URL as
/// reported by the trailer (e.g. `foo.js.map`, `https://cdn.example/x.js.map`).
/// Returning `Some(json)` attaches the parsed map as `inputSourceMap` on the
/// result so a subsequent [`crate::remap_coverage`] resolves
/// positions back to the original source in one chained call. Returning
/// `None` leaves `inputSourceMap` unset.
///
/// The loader is sync and infallible: side channels (disk I/O errors, HTTP
/// failures) collapse to `None`. Caller-side URL resolution (relative paths,
/// http schemes, file:// URIs) is intentionally not handled here.
///
/// # Example
///
/// ```
/// use oxc_coverage_instrument::{V8CoverageRange, V8FunctionCoverage, v8_to_istanbul_with_loader};
///
/// let source = "const x = 1;\n//# sourceMappingURL=app.js.map\n";
/// let functions = vec![V8FunctionCoverage {
///     function_name: String::new(),
///     ranges: vec![V8CoverageRange { start_offset: 0, end_offset: source.len() as u32, count: 1 }],
///     is_block_coverage: false,
/// }];
///
/// // Loader keyed on the trailer URL; here we just keep one map in-memory,
/// // but a real caller would read from disk relative to `filename`.
/// let map_json = r#"{"version":3,"sources":["src/app.ts"],"mappings":"AAAA","names":[]}"#;
/// let fc = v8_to_istanbul_with_loader(source, "app.js", &functions, 0, |url| {
///     if url == "app.js.map" { Some(map_json.to_string()) } else { None }
/// })
/// .unwrap();
///
/// // The loader-supplied map is attached and can be chained into remap_coverage.
/// assert!(fc.input_source_map.is_some());
/// ```
///
/// # Errors
///
/// Returns [`V8ToIstanbulError::Parse`] if `source` cannot be parsed.
pub fn v8_to_istanbul_with_loader<L>(
    source: &str,
    filename: &str,
    functions: &[V8FunctionCoverage],
    wrapper_length_utf16: u32,
    loader: L,
) -> Result<FileCoverage, V8ToIstanbulError>
where
    L: Fn(&str) -> Option<String>,
{
    let collected = collect_for_v8_to_istanbul(source, filename)
        .map_err(|e| V8ToIstanbulError::Parse(e.to_string()))?;
    let mut file_coverage = collected.coverage_map;
    apply_v8_coverage_with_static_members(
        &mut file_coverage,
        source,
        functions,
        wrapper_length_utf16,
        &collected.arm_body_byte_spans,
        &collected.static_member_byte_spans,
    );

    if file_coverage.input_source_map.is_none() {
        if let Some(inline_map) = extract_inline_source_map(source) {
            file_coverage.input_source_map = Some(inline_map);
        } else if let Some(url) = extract_external_source_mapping_url(source)
            && let Some(json) = loader(url)
        {
            file_coverage.input_source_map = serde_json::from_str::<serde_json::Value>(&json).ok();
        }
    }

    Ok(file_coverage)
}
