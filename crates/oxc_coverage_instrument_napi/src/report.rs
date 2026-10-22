//! Report and summary bindings over `oxc_coverage_report` and
//! `oxc_coverage_reports`.
//!
//! Every function takes a `coverage-final.json`-shaped string, the same input
//! as `remapCoverageMap`. None of them remap: a map whose entries carry an
//! `inputSourceMap` should go through `remapCoverageMap` first, except for the
//! html report, which remaps internally like `oxc-coverage-instrument report`.

use std::{collections::HashMap, path::PathBuf};

use napi_derive::napi;
use oxc_coverage_report::{
    CoverageMap, CoverageSummary as CoreCoverageSummary, Metric as CoreMetric, summarize,
};
use oxc_coverage_reports::Format;

use crate::{generic_failure, invalid_coverage_json_error};

/// Options for `createReport`.
#[napi(object)]
pub struct ReportOptions {
    /// Directory that `lcov` and `cobertura` source paths are made relative
    /// to. A path outside it stays verbatim.
    ///
    /// Defaults to no relativization, so paths appear as the coverage map
    /// records them. Pass `process.cwd()` to match `istanbul-reports`.
    pub root_dir: Option<String>,
}

/// Options for `writeHtmlReport`.
#[napi(object)]
pub struct HtmlReportOptions {
    /// Directory that relative coverage paths are resolved against to read
    /// the source files. Defaults to no prefix.
    pub root_dir: Option<String>,
    /// Percentage at or above which a metric renders green. Must be in
    /// `[0, 100]`. Defaults to `80`.
    pub green_threshold: Option<f64>,
}

/// Counts for one Istanbul metric, the `istanbul-lib-coverage` shape.
#[napi(object)]
pub struct CoverageMetric {
    /// Number of entries the instrumenter recorded.
    pub total: u32,
    /// Number of those entries with at least one hit.
    pub covered: u32,
    /// Number of entries excluded by an ignore pragma.
    pub skipped: u32,
    /// `covered / total` as a percentage, rounded to two decimals, or `100`
    /// when `total` is `0`.
    pub pct: f64,
}

/// The four-metric summary of one file or of the whole map.
#[napi(object)]
pub struct CoverageSummary {
    /// Distinct source lines that carry at least one statement.
    pub lines: CoverageMetric,
    /// Entries of `statementMap`.
    pub statements: CoverageMetric,
    /// Entries of `fnMap`.
    pub functions: CoverageMetric,
    /// Branch arms, summed over every entry of `branchMap`.
    pub branches: CoverageMetric,
}

/// Result of `summarizeCoverage`.
#[napi(object)]
pub struct CoverageSummaryResult {
    /// Summary of every file together.
    pub total: CoverageSummary,
    /// Summary per file, keyed like the input coverage map.
    pub files: HashMap<String, CoverageSummary>,
}

impl From<&CoreMetric> for CoverageMetric {
    fn from(metric: &CoreMetric) -> Self {
        Self {
            total: metric.total,
            covered: metric.covered,
            skipped: metric.skipped,
            pct: metric.pct,
        }
    }
}

impl From<&CoreCoverageSummary> for CoverageSummary {
    fn from(summary: &CoreCoverageSummary) -> Self {
        Self {
            lines: (&summary.lines).into(),
            statements: (&summary.statements).into(),
            functions: (&summary.functions).into(),
            branches: (&summary.branches).into(),
        }
    }
}

/// Summarize a coverage map into total and per-file metrics.
///
/// The numbers match `istanbul-lib-coverage`'s
/// `coverageMap.getCoverageSummary()` and `fileCoverage.toSummary()`.
///
/// # Errors
///
/// Returns an error if `coverage_json` is not an Istanbul `CoverageMap`.
#[napi]
pub fn summarize_coverage(coverage_json: String) -> napi::Result<CoverageSummaryResult> {
    let map = parse_map(&coverage_json)?;
    let total = (&summarize(&map).summary).into();
    let files = map
        .iter()
        .map(|(path, file)| (path.clone(), (&CoreCoverageSummary::from_file(file)).into()))
        .collect();
    Ok(CoverageSummaryResult { total, files })
}

/// Render a coverage map as one of the single-stream Istanbul reports and
/// return the report text.
///
/// `format` is `text`, `text-summary`, `json-summary`, `lcov`, or
/// `cobertura`. Use `writeHtmlReport` for `html`, which writes a directory.
///
/// # Errors
///
/// Returns an error if `format` is not a single-stream format, or if
/// `coverage_json` is not an Istanbul `CoverageMap`.
#[napi(
    ts_args_type = "coverageJson: string, format: 'text' | 'text-summary' | 'json-summary' | 'lcov' | 'cobertura', options?: ReportOptions | undefined | null"
)]
pub fn create_report(
    coverage_json: String,
    format: String,
    options: Option<ReportOptions>,
) -> napi::Result<String> {
    let format = stream_format(&format)?;
    let map = parse_map(&coverage_json)?;
    let root_dir = PathBuf::from(options.and_then(|o| o.root_dir).unwrap_or_default());
    let mut out = Vec::new();
    format.write(&summarize(&map), &root_dir, &mut out).map_err(generic_failure)?;
    String::from_utf8(out).map_err(generic_failure)
}

/// Write the html report for a coverage map into `output_dir`, creating it
/// when needed.
///
/// Entries with an `inputSourceMap` are remapped first, so the pages show the
/// original source. Only the native bindings carry the html reporter; the
/// WebAssembly binding returns an error.
///
/// # Errors
///
/// Returns an error if `coverage_json` is not an Istanbul `CoverageMap`, if
/// `greenThreshold` is out of range, if a coverage path escapes `output_dir`,
/// or if writing a page fails.
#[napi]
pub fn write_html_report(
    coverage_json: String,
    output_dir: String,
    options: Option<HtmlReportOptions>,
) -> napi::Result<()> {
    let map = parse_map(&coverage_json)?;
    let (root_dir, green_threshold) =
        options.map_or((None, None), |o| (o.root_dir, o.green_threshold));
    let html_options = match green_threshold {
        Some(value) => oxc_coverage_reports::html::HtmlOptions::new(value)
            .map_err(|e| napi::Error::new(napi::Status::InvalidArg, e.to_string()))?,
        None => oxc_coverage_reports::html::HtmlOptions::default(),
    };
    write_html(
        &map,
        &PathBuf::from(root_dir.unwrap_or_default()),
        &PathBuf::from(output_dir),
        &html_options,
    )
}

#[cfg(not(target_family = "wasm"))]
fn write_html(
    map: &CoverageMap,
    root_dir: &std::path::Path,
    output_dir: &std::path::Path,
    options: &oxc_coverage_reports::html::HtmlOptions,
) -> napi::Result<()> {
    Format::Html.write_to_dir(map, root_dir, output_dir, options).map_err(generic_failure)
}

#[cfg(target_family = "wasm")]
fn write_html(
    _map: &CoverageMap,
    _root_dir: &std::path::Path,
    _output_dir: &std::path::Path,
    _options: &oxc_coverage_reports::html::HtmlOptions,
) -> napi::Result<()> {
    Err(napi::Error::new(
        napi::Status::GenericFailure,
        "the html report needs the native binding; the WebAssembly binding does not include it",
    ))
}

fn stream_format(name: &str) -> napi::Result<Format> {
    if name == "html" {
        return Err(napi::Error::new(
            napi::Status::InvalidArg,
            "html writes a directory; use writeHtmlReport instead of createReport",
        ));
    }
    Format::parse(name).filter(|format| !format.is_multi_file()).ok_or_else(|| {
        napi::Error::new(
            napi::Status::InvalidArg,
            format!(
                "unknown report format `{name}`; expected text, text-summary, json-summary, lcov, or cobertura"
            ),
        )
    })
}

fn parse_map(coverage_json: &str) -> napi::Result<CoverageMap> {
    oxc_coverage_instrument::parse_coverage_map(coverage_json)
        .map_err(|e| invalid_coverage_json_error(coverage_json, e))
}
