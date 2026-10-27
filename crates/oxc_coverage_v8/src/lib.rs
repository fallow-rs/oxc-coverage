//! V8 inspector coverage to Istanbul `FileCoverage` conversion.
//!
//! V8's inspector protocol reports coverage as `[startOffset, endOffset, count]`
//! ranges grouped by function. Istanbul reporters consume per-statement,
//! per-function, and per-branch hit counts keyed by (line, column). This crate
//! takes a pre-built `FileCoverage` (typically produced by an AST-traversal
//! pass in an instrumenter) and fills in its hit-count vectors from V8 ranges.
//!
//! ## Position semantics
//!
//! Istanbul's `Position` is 1-based line + 0-based UTF-16 column. V8 ranges are
//! absolute UTF-16 code-unit offsets into the V8-visible source. Oxc branch
//! body spans use UTF-8 byte offsets, so this crate converts source-side
//! coordinates to absolute UTF-16 offsets before intersecting V8 ranges.
//!
//! ## Wrapper base
//!
//! Node wraps every CommonJS module in `(function(exports,require,module,...){`.
//! Some coverage producers report offsets relative to a wrapped source. Pass
//! their wrapper prefix length in UTF-16 code units so source-side coordinates
//! use the same base. Node inspector coverage is normally source-relative, so
//! its wrapper length is zero.
//!
//! ## Companion helpers for inline / external source maps
//!
//! V8 coverage data does not carry source-map information; the source itself
//! does, via a `//# sourceMappingURL=` trailer. [`extract_inline_source_map`]
//! decodes the inline `data:application/json` form and
//! [`extract_external_source_mapping_url`] reports the trailer URL when it is
//! not a data URL. Callers can attach the result as `FileCoverage.input_source_map`
//! and chain through `oxc_coverage_source_maps::remap_coverage` to resolve
//! positions back to the original source.

mod source_index;
mod source_map_url;

use std::collections::BTreeMap;

use oxc_coverage_types::{FileCoverage, Location};
use serde::{Deserialize, Serialize};

use crate::source_index::SourceIndex;

pub use crate::source_map_url::{extract_external_source_mapping_url, extract_inline_source_map};

/// A function's coverage data as reported by the V8 inspector.
///
/// Serializes to / from the same JSON shape as the Chrome DevTools Protocol's
/// [`Profiler.FunctionCoverage`](https://chromedevtools.github.io/devtools-protocol/tot/Profiler/#type-FunctionCoverage)
/// so callers can hand the V8 inspector's output straight through.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V8FunctionCoverage {
    /// Function name as reported by V8 (may be empty for anonymous functions
    /// or the implicit top-level module function).
    #[serde(rename = "functionName")]
    pub function_name: String,
    /// One or more UTF-16 code-unit ranges. With `is_block_coverage = false` there is
    /// exactly one range (the whole function); with `is_block_coverage = true`
    /// the outermost range covers the function and inner ranges cover blocks.
    pub ranges: Vec<V8CoverageRange>,
    /// When true, `ranges` includes block-level subdivisions. When false, the
    /// only count is at function granularity.
    #[serde(rename = "isBlockCoverage")]
    pub is_block_coverage: bool,
}

/// A single V8 coverage range.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct V8CoverageRange {
    /// UTF-16 code-unit offset of the range start (inclusive).
    #[serde(rename = "startOffset")]
    pub start_offset: u32,
    /// UTF-16 code-unit offset of the range end (exclusive).
    #[serde(rename = "endOffset")]
    pub end_offset: u32,
    /// Hit count. Zero means the range was reachable but never executed.
    pub count: u32,
}

/// The distance in UTF-16 code units within which a V8 range end can still
/// match a branch arm end.
const ARM_TOLERANCE: u32 = 4;

/// How a branch arm finds its own V8 range.
#[derive(Clone, Copy)]
enum ArmMatch {
    /// Both range ends sit within [`ARM_TOLERANCE`] of the arm body span.
    Near,
    /// As `Near`, then against the arm [`Location`]. An `if` arm location is
    /// the whole statement, so its body span comes first.
    NearOrLocation,
    /// A `cond-expr` arm. V8 starts the arm range at the `?` or `:` token, and
    /// a comment or a line break can come before or after that token. The
    /// body span runs from the end of the previous operand to the end of the
    /// arm, so the range must start in the gap before the arm.
    Window,
}

#[derive(Clone, Copy)]
struct CoverageRangeWithMode {
    range: V8CoverageRange,
    is_block_coverage: bool,
}

struct CoverageContext<'a> {
    source_index: &'a SourceIndex,
    ranges: &'a [V8CoverageRange],
    arm_ranges: &'a [V8CoverageRange],
    inheritance_ranges: &'a [CoverageRangeWithMode],
    wrapper_length_utf16: u32,
}

impl CoverageContext<'_> {
    fn count_for_location(&self, loc: &Location) -> u32 {
        let (start, end) = self.location_utf16_span(loc);
        smallest_containing_range_count(start, end, self.ranges)
    }

    /// The count of the V8 range that matches one branch arm, if any.
    ///
    /// An empty body span (a synthetic `else`) never matches. See [`ArmMatch`]
    /// for the match rules.
    fn tight_arm_count(
        &self,
        arm_loc: &Location,
        body_byte_span: Option<(u32, u32)>,
        mode: ArmMatch,
    ) -> Option<u32> {
        if body_byte_span.is_some_and(|(start, end)| start == end) {
            return None;
        }
        let (body_start, body_end) = self.arm_utf16_span(arm_loc, body_byte_span);
        let (location_start, location_end) = self.location_utf16_span(arm_loc);
        match mode {
            ArmMatch::Near => self.best_arm_range_count(body_start, body_end),
            ArmMatch::NearOrLocation => self
                .best_arm_range_count(body_start, body_end)
                .or_else(|| self.best_arm_range_count(location_start, location_end)),
            ArmMatch::Window if body_byte_span.is_some() => {
                self.window_arm_range_count(body_start, location_start, location_end, body_end)
            }
            ArmMatch::Window => self.best_arm_range_count(location_start, location_end),
        }
    }

    /// The count of the V8 range that starts in `[gap_start, arm_start]` and
    /// ends in `[arm_end, body_end + ARM_TOLERANCE]`.
    ///
    /// The range of the closest end wins. A range that starts before
    /// `gap_start` belongs to the test or to the previous arm, and a range that
    /// starts after `arm_start` lies inside the arm.
    fn window_arm_range_count(
        &self,
        gap_start: u32,
        arm_start: u32,
        arm_end: u32,
        body_end: u32,
    ) -> Option<u32> {
        let first = self.arm_ranges.partition_point(|r| r.start_offset < gap_start);
        self.arm_ranges[first..]
            .iter()
            .take_while(|r| r.start_offset <= arm_start)
            .filter(|r| {
                r.end_offset >= arm_end && r.end_offset <= body_end.saturating_add(ARM_TOLERANCE)
            })
            .min_by_key(|r| (r.end_offset - arm_end, r.start_offset))
            .map(|r| r.count)
    }

    /// The count of an arm with no matching V8 range, from the smallest range
    /// that strictly contains the arm.
    ///
    /// V8 removes a child range whose count equals the count of its parent
    /// range, so an arm with no range of its own runs as often as its parent.
    fn inherited_arm_count(&self, arm_loc: &Location, body_byte_span: Option<(u32, u32)>) -> u32 {
        if body_byte_span.is_some_and(|(start, end)| start >= end) {
            return 0;
        }
        let (arm_start, arm_end) = self.arm_utf16_span(arm_loc, body_byte_span);
        self.smallest_block_range_count(arm_start, arm_end, true).unwrap_or(0)
    }

    /// The count of the smallest range that contains `[start, end)`, when that
    /// range comes from a block-coverage function.
    ///
    /// With `strict`, a range equal to `[start, end)` does not count as a
    /// container. A function-only record gives no block counts, so a smallest
    /// container from such a record gives `None`.
    fn smallest_block_range_count(&self, start: u32, end: u32, strict: bool) -> Option<u32> {
        self.inheritance_ranges
            .iter()
            .find(|entry| {
                entry.range.start_offset <= start
                    && entry.range.end_offset >= end
                    && (!strict || entry.range.start_offset < start || entry.range.end_offset > end)
            })
            .filter(|entry| entry.is_block_coverage)
            .map(|entry| entry.range.count)
    }

    fn arm_utf16_span(&self, arm_loc: &Location, body_byte_span: Option<(u32, u32)>) -> (u32, u32) {
        match body_byte_span {
            Some((start, end)) => (
                self.source_index.byte_to_utf16(start).saturating_add(self.wrapper_length_utf16),
                self.source_index.byte_to_utf16(end).saturating_add(self.wrapper_length_utf16),
            ),
            None => self.location_utf16_span(arm_loc),
        }
    }

    fn location_utf16_span(&self, loc: &Location) -> (u32, u32) {
        (
            self.source_index
                .position_to_utf16(loc.start.line, loc.start.column)
                .saturating_add(self.wrapper_length_utf16),
            self.source_index
                .position_to_utf16(loc.end.line, loc.end.column)
                .saturating_add(self.wrapper_length_utf16),
        )
    }

    /// The count of the nearest V8 range whose start and end both sit within
    /// [`ARM_TOLERANCE`] code units of `[arm_start, arm_end)`, breaking ties towards
    /// the narrower range.
    fn best_arm_range_count(&self, arm_start: u32, arm_end: u32) -> Option<u32> {
        let mut best: Option<(V8CoverageRange, u32)> = None;
        let lower = arm_start.saturating_sub(ARM_TOLERANCE);
        let upper = arm_start.saturating_add(ARM_TOLERANCE);
        let start = self.arm_ranges.partition_point(|r| r.start_offset < lower);
        for r in &self.arm_ranges[start..] {
            if r.start_offset > upper {
                break;
            }
            let dist_start = r.start_offset.abs_diff(arm_start);
            let dist_end = r.end_offset.abs_diff(arm_end);
            if dist_end > ARM_TOLERANCE {
                continue;
            }
            let distance = dist_start + dist_end;
            match best {
                None => best = Some((*r, distance)),
                Some((prev, prev_distance)) => {
                    let prev_width = prev.end_offset.saturating_sub(prev.start_offset);
                    let this_width = r.end_offset.saturating_sub(r.start_offset);
                    if distance < prev_distance
                        || (distance == prev_distance && this_width < prev_width)
                    {
                        best = Some((*r, distance));
                    }
                }
            }
        }
        best.map(|(r, _)| r.count)
    }
}

/// Fill in the statement, function and branch hit counts of a pre-built
/// [`FileCoverage`] from V8's coverage ranges.
///
/// The caller builds `file_coverage`, typically from an instrumenter's
/// AST-traversal pass, and supplies `arm_body_byte_spans`:
/// `arm_body_byte_spans["<branch_id>"][<arm_idx>]` is the `(start, end)` UTF-8
/// byte range of that arm's body. An empty entry marks a synthetic body, such
/// as the implicit `else` of an `if`. A non-empty body span is matched against
/// V8's block ranges before falling back to the arm's [`Location`].
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
/// `wrapper_length_utf16` is the UTF-16 code-unit base for producers that report
/// wrapper-shifted ranges. Pass 0 for source-relative inspector output.
pub fn apply_v8_coverage(
    file_coverage: &mut FileCoverage,
    source: &str,
    functions: &[V8FunctionCoverage],
    wrapper_length_utf16: u32,
    arm_body_byte_spans: &BTreeMap<String, Vec<(u32, u32)>>,
) {
    let source_index = SourceIndex::new(source);
    let mut ranges: Vec<V8CoverageRange> =
        functions.iter().flat_map(|f| f.ranges.iter().copied()).collect();
    ranges.sort_by_key(|r| r.end_offset.saturating_sub(r.start_offset));
    // The first range in every FunctionCoverage record is that function's outer
    // range, not a branch range. A nested function declaration can sit inside an
    // if arm, so only child ranges from block-coverage records are eligible for
    // tight arm matching.
    let mut arm_ranges: Vec<V8CoverageRange> = functions
        .iter()
        .filter(|function| function.is_block_coverage)
        .flat_map(|function| function.ranges.iter().skip(1).copied())
        .collect();
    arm_ranges.sort_by_key(|r| r.start_offset);
    let mut inheritance_ranges: Vec<CoverageRangeWithMode> = functions
        .iter()
        .flat_map(|function| {
            function.ranges.iter().copied().map(|range| CoverageRangeWithMode {
                range,
                is_block_coverage: function.is_block_coverage,
            })
        })
        .collect();
    inheritance_ranges
        .sort_by_key(|entry| entry.range.end_offset.saturating_sub(entry.range.start_offset));
    let context = CoverageContext {
        source_index: &source_index,
        ranges: &ranges,
        arm_ranges: &arm_ranges,
        inheritance_ranges: &inheritance_ranges,
        wrapper_length_utf16,
    };

    apply_statement_counts(file_coverage, &context);
    apply_function_counts(file_coverage, &context);
    apply_branch_counts(file_coverage, arm_body_byte_spans, &context);
}

fn apply_statement_counts(file_coverage: &mut FileCoverage, context: &CoverageContext<'_>) {
    for (id, loc) in &file_coverage.statement_map {
        let count = context.count_for_location(loc);
        if let Some(slot) = file_coverage.s.get_mut(id) {
            *slot = count;
        }
    }
}

fn apply_function_counts(file_coverage: &mut FileCoverage, context: &CoverageContext<'_>) {
    for (id, fn_entry) in &file_coverage.fn_map {
        let count = context.count_for_location(&fn_entry.loc);
        if let Some(slot) = file_coverage.f.get_mut(id) {
            *slot = count;
        }
    }
}

/// How a branch type resolves an arm that has no V8 range of its own.
#[derive(Clone, Copy, PartialEq, Eq)]
enum MissingArmRule {
    /// The arm gets the count of the smallest range that contains it.
    Inherit,
    /// The arm gets the count of the branch parent minus the other arms.
    Remainder,
    /// No V8 range gives the arm a count.
    Zero,
}

impl MissingArmRule {
    /// `complete` is false when a pragma removed an arm of an `if` or a
    /// `cond-expr`. The count of a removed arm is unknown, so the other arm
    /// cannot take the remainder.
    fn for_arm(branch_type: &str, body_byte_span: Option<(u32, u32)>, complete: bool) -> Self {
        let empty_body = body_byte_span.is_some_and(|(start, end)| start >= end);
        match branch_type {
            // A synthetic `else` has no source text, so it cannot inherit.
            "if" if empty_body && complete => Self::Remainder,
            // The `if` arm location is the whole statement, so only a body
            // span gives a range to inherit from.
            "if" if body_byte_span.is_some() && !empty_body => Self::Inherit,
            "cond-expr" if complete => Self::Remainder,
            "cond-expr" | "binary-expr" | "default-arg" | "switch" if !empty_body => Self::Inherit,
            _ => Self::Zero,
        }
    }
}

fn apply_branch_counts(
    file_coverage: &mut FileCoverage,
    arm_body_byte_spans: &BTreeMap<String, Vec<(u32, u32)>>,
    context: &CoverageContext<'_>,
) {
    for (id, branch_entry) in &file_coverage.branch_map {
        let Some(slot) = file_coverage.b.get_mut(id) else {
            continue;
        };
        let body_spans = arm_body_byte_spans.get(id);
        let body_span = |arm_idx: usize| body_spans.and_then(|spans| spans.get(arm_idx).copied());
        let complete = branch_entry.locations.len() == 2;
        let mode = match branch_entry.branch_type.as_str() {
            "if" => ArmMatch::NearOrLocation,
            "cond-expr" => ArmMatch::Window,
            _ => ArmMatch::Near,
        };

        // First pass: each arm with its own V8 range, or with an enclosing
        // range to inherit from. A `Remainder` arm stays `None`.
        let mut counts: Vec<Option<u32>> = Vec::with_capacity(branch_entry.locations.len());
        let mut has_remainder_arm = false;
        for (arm_idx, loc) in branch_entry.locations.iter().enumerate() {
            let body = body_span(arm_idx);
            let count =
                context.tight_arm_count(loc, body, mode).or_else(|| match MissingArmRule::for_arm(
                    &branch_entry.branch_type,
                    body,
                    complete,
                ) {
                    MissingArmRule::Inherit => Some(context.inherited_arm_count(loc, body)),
                    MissingArmRule::Remainder => {
                        has_remainder_arm = true;
                        None
                    }
                    MissingArmRule::Zero => Some(0),
                });
            counts.push(count);
        }

        // Second pass: the first `Remainder` arm gets the parent count minus
        // the other arms, as each evaluation of the branch takes one arm.
        // Later `Remainder` arms get 0.
        let mut remainder = if has_remainder_arm {
            // The parent is the range that is active when the branch starts.
            // After an early `return`, `break` or `continue`, V8 can split the
            // range around the branch, so the whole branch span can sit in a
            // larger range than its test does.
            let (start, _) = context.location_utf16_span(&branch_entry.loc);
            let known = counts.iter().flatten().fold(0u32, |sum, count| sum.saturating_add(*count));
            context
                .smallest_block_range_count(start, start.saturating_add(1), false)
                .map_or(0, |parent| parent.saturating_sub(known))
        } else {
            0
        };
        slot.clear();
        slot.extend(
            counts.into_iter().map(|count| count.unwrap_or_else(|| std::mem::take(&mut remainder))),
        );
    }
}

/// Pick the count of the smallest V8 range that fully contains `[start, end)`.
/// Smaller ranges represent inner blocks (with their own counts under
/// `isBlockCoverage`) and override the outer function-level count.
///
/// Both V8 ranges and the statement UTF-16 span use the half-open convention
/// (`endOffset` / `end` are exclusive). The containment predicate is therefore
/// `r.start <= start && r.end >= end`: a range whose exclusive end is equal
/// to the statement's exclusive end is the smallest possible exact container.
fn smallest_containing_range_count(start: u32, end: u32, ranges: &[V8CoverageRange]) -> u32 {
    for r in ranges {
        if r.start_offset <= start && r.end_offset >= end {
            return r.count;
        }
    }
    0
}
