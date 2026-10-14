//! Per-metric coverage aggregates.
//!
//! Mirrors istanbul-lib-coverage's `CoverageSummary` shape so consumers can
//! round-trip through json-summary without field translation.

use std::collections::BTreeSet;

use oxc_coverage_types::FileCoverage;

/// Coverage counts for one of the four Istanbul metrics.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Metric {
    /// Number of entries the instrumenter recorded for this metric.
    pub total: u32,
    /// Number of those entries with at least one hit.
    pub covered: u32,
    /// Number of entries excluded by an `/* istanbul ignore */` pragma.
    ///
    /// Always `0` here: [`oxc_coverage_types`] coerces `null` hit counts to `0`
    /// on ingestion, and [`Metric::new`] sets this field to `0`. It exists so
    /// json-summary output matches the istanbul-reports schema.
    pub skipped: u32,
    /// `covered / total` as a percentage, rounded to two decimal places, or
    /// `100.0` when `total` is `0`.
    pub pct: f64,
}

impl Default for Metric {
    /// Zero counts with `pct = 100.0`, matching istanbul-lib-coverage's
    /// zero-total convention (a file with no statements is "fully covered").
    fn default() -> Self {
        Self { total: 0, covered: 0, skipped: 0, pct: 100.0 }
    }
}

impl Metric {
    /// Build a metric from a total and covered count.
    ///
    /// `pct` is rounded to two decimal places. A zero-total metric reports
    /// `pct = 100.0`, matching istanbul-lib-coverage.
    pub fn new(total: u32, covered: u32) -> Self {
        Self { total, covered, skipped: 0, pct: pct(covered, total) }
    }

    /// Add another metric into this one, recomputing `pct` from the new totals.
    pub fn merge(&mut self, other: &Self) {
        self.total += other.total;
        self.covered += other.covered;
        self.skipped += other.skipped;
        self.pct = pct(self.covered, self.total);
    }
}

/// The four-metric per-file or per-folder coverage summary.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CoverageSummary {
    /// Distinct source lines that carry at least one statement.
    pub lines: Metric,
    /// Entries of `statementMap`.
    pub statements: Metric,
    /// Entries of `fnMap`.
    pub functions: Metric,
    /// Branch arms, summed over every entry of `branchMap`.
    pub branches: Metric,
}

impl CoverageSummary {
    /// Compute the summary for a single [`FileCoverage`].
    pub fn from_file(file: &FileCoverage) -> Self {
        Self {
            lines: lines_metric(file),
            statements: statements_metric(file),
            functions: functions_metric(file),
            branches: branches_metric(file),
        }
    }

    /// Merge another summary into this one (used to roll folder summaries up).
    pub fn merge(&mut self, other: &Self) {
        self.lines.merge(&other.lines);
        self.statements.merge(&other.statements);
        self.functions.merge(&other.functions);
        self.branches.merge(&other.branches);
    }
}

fn pct(covered: u32, total: u32) -> f64 {
    if total == 0 {
        return 100.0;
    }
    let raw = f64::from(covered) * 100.0 / f64::from(total);
    (raw * 100.0).round() / 100.0
}

/// Narrow a container length to the `u32` width Istanbul counters use.
#[expect(
    clippy::cast_possible_truncation,
    reason = "a coverage map with u32::MAX entries cannot be held by the parser that produced it"
)]
fn count(len: usize) -> u32 {
    len as u32
}

// istanbul-lib-coverage derives the statement, function and branch totals from
// the counter maps (`computeSimpleTotals` / `computeBranchTotals`,
// istanbul-lib-coverage 3.2.2 `lib/file-coverage.js`), so a counter map that is
// missing entries, or longer than its metadata declares, moves the denominator.
// The totals below come from the location maps instead, which are the
// instrumenter's authoritative inventory, so a malformed counter map cannot
// change them.
fn statements_metric(file: &FileCoverage) -> Metric {
    let total = count(file.statement_map.len());
    let covered = count(
        file.statement_map.keys().filter(|id| file.s.get(*id).copied().unwrap_or(0) > 0).count(),
    );
    Metric::new(total, covered)
}

fn functions_metric(file: &FileCoverage) -> Metric {
    let total = count(file.fn_map.len());
    let covered =
        count(file.fn_map.keys().filter(|id| file.f.get(*id).copied().unwrap_or(0) > 0).count());
    Metric::new(total, covered)
}

fn branches_metric(file: &FileCoverage) -> Metric {
    let mut total = 0;
    let mut covered = 0;
    for (id, entry) in &file.branch_map {
        total += count(entry.locations.len());
        for arm_index in 0..entry.locations.len() {
            let hits = file.b.get(id).and_then(|arms| arms.get(arm_index)).copied().unwrap_or(0);
            covered += u32::from(hits > 0);
        }
    }
    Metric::new(total, covered)
}

fn lines_metric(file: &FileCoverage) -> Metric {
    let mut all_lines: BTreeSet<u32> = BTreeSet::new();
    let mut hit_lines: BTreeSet<u32> = BTreeSet::new();
    // Istanbul treats the counter map as authoritative for line coverage.
    // Metadata without a matching counter contributes no source line.
    for (id, hits) in &file.s {
        let Some(loc) = file.statement_map.get(id) else {
            continue;
        };
        let line = loc.start.line;
        // Istanbul lines are 1-based, so 0 is the unknown-position sentinel
        // rather than a source line.
        if line == 0 {
            continue;
        }
        all_lines.insert(line);
        if *hits > 0 {
            hit_lines.insert(line);
        }
    }
    Metric::new(count(all_lines.len()), count(hit_lines.len()))
}

#[cfg(test)]
mod tests {
    use oxc_coverage_types::parse_coverage_map;

    use super::*;

    fn parse_single(json: &str) -> FileCoverage {
        let map = parse_coverage_map(json).unwrap();
        map.into_values().next().unwrap()
    }

    /// One two-arm `if` branch on line 1, with `b` set to `hits`.
    fn two_arm_branch(hits: &str) -> FileCoverage {
        let json = format!(
            r#"{{"a.js":{{"path":"a.js","statementMap":{{}},"fnMap":{{}},"branchMap":{{"0":{{"loc":{{"start":{{"line":1,"column":0}},"end":{{"line":1,"column":3}}}},"line":1,"type":"if","locations":[{{"start":{{"line":1,"column":0}},"end":{{"line":1,"column":1}}}},{{"start":{{"line":1,"column":2}},"end":{{"line":1,"column":3}}}}]}}}},"s":{{}},"f":{{}},"b":{hits}}}}}"#
        );
        parse_single(&json)
    }

    #[test]
    fn pct_is_one_hundred_for_zero_total() {
        let m = Metric::new(0, 0);
        assert_eq!(m.pct, 100.0);
    }

    #[test]
    fn pct_rounds_to_two_decimals() {
        // 1/3 = 33.333... -> 33.33
        let m = Metric::new(3, 1);
        assert_eq!(m.pct, 33.33);
        // 2/3 = 66.666... -> 66.67
        let m = Metric::new(3, 2);
        assert_eq!(m.pct, 66.67);
    }

    #[test]
    fn statements_count_nonzero_hits() {
        let json = r#"{"a.js":{"path":"a.js","statementMap":{"0":{"start":{"line":1,"column":0},"end":{"line":1,"column":5}},"1":{"start":{"line":2,"column":0},"end":{"line":2,"column":5}}},"fnMap":{},"branchMap":{},"s":{"0":1,"1":0},"f":{},"b":{}}}"#;
        let f = parse_single(json);
        let s = CoverageSummary::from_file(&f);
        assert_eq!(s.statements, Metric { total: 2, covered: 1, skipped: 0, pct: 50.0 });
    }

    #[test]
    fn branches_sum_arms() {
        let json = r#"{"a.js":{"path":"a.js","statementMap":{},"fnMap":{},"branchMap":{"0":{"loc":{"start":{"line":1,"column":0},"end":{"line":1,"column":1}},"line":1,"type":"if","locations":[{"start":{"line":1,"column":0},"end":{"line":1,"column":1}},{"start":{"line":1,"column":2},"end":{"line":1,"column":3}}]}},"s":{},"f":{},"b":{"0":[1,0]}}}"#;
        let f = parse_single(json);
        let s = CoverageSummary::from_file(&f);
        assert_eq!(s.branches, Metric { total: 2, covered: 1, skipped: 0, pct: 50.0 });
    }

    #[test]
    fn orphan_statement_and_function_hits_are_ignored() {
        let json = r#"{"a.js":{"path":"a.js","statementMap":{"0":{"start":{"line":1,"column":0},"end":{"line":1,"column":1}}},"fnMap":{"0":{"name":"f","decl":{"start":{"line":1,"column":0},"end":{"line":1,"column":1}},"loc":{"start":{"line":1,"column":0},"end":{"line":1,"column":1}},"line":1}},"branchMap":{},"s":{"0":0,"99":7},"f":{"0":0,"99":7},"b":{}}}"#;
        let summary = CoverageSummary::from_file(&parse_single(json));
        assert_eq!(summary.statements, Metric::new(1, 0));
        assert_eq!(summary.functions, Metric::new(1, 0));
    }

    #[test]
    fn missing_statement_and_function_counters_are_uncovered() {
        let json = r#"{"a.js":{"path":"a.js","statementMap":{"0":{"start":{"line":1,"column":0},"end":{"line":1,"column":1}}},"fnMap":{"0":{"name":"f","decl":{"start":{"line":1,"column":0},"end":{"line":1,"column":1}},"loc":{"start":{"line":1,"column":0},"end":{"line":1,"column":1}},"line":1}},"branchMap":{},"s":{},"f":{},"b":{}}}"#;
        let summary = CoverageSummary::from_file(&parse_single(json));
        assert_eq!(summary.statements, Metric::new(1, 0));
        assert_eq!(summary.functions, Metric::new(1, 0));
    }

    #[test]
    fn lines_ignore_statement_map_entries_without_counters() {
        let json = r#"{"a.js":{"path":"a.js","statementMap":{"0":{"start":{"line":1,"column":0},"end":{"line":1,"column":5}},"1":{"start":{"line":7,"column":0},"end":{"line":7,"column":5}}},"fnMap":{},"branchMap":{},"s":{"0":3},"f":{},"b":{}}}"#;
        let summary = CoverageSummary::from_file(&parse_single(json));
        assert_eq!(summary.lines, Metric::new(1, 1));
    }

    #[test]
    fn absent_branch_counters_count_all_arms_uncovered() {
        assert_eq!(CoverageSummary::from_file(&two_arm_branch("{}")).branches, Metric::new(2, 0));
    }

    #[test]
    fn short_branch_counter_array_leaves_missing_arms_uncovered() {
        assert_eq!(
            CoverageSummary::from_file(&two_arm_branch(r#"{"0":[4]}"#)).branches,
            Metric::new(2, 1)
        );
    }

    #[test]
    fn branch_counters_beyond_the_declared_arms_are_ignored() {
        assert_eq!(
            CoverageSummary::from_file(&two_arm_branch(r#"{"0":[4,0,9]}"#)).branches,
            Metric::new(2, 1)
        );
    }

    #[test]
    fn lines_dedupe_by_start_line() {
        let json = r#"{"a.js":{"path":"a.js","statementMap":{"0":{"start":{"line":1,"column":0},"end":{"line":1,"column":5}},"1":{"start":{"line":1,"column":6},"end":{"line":1,"column":10}},"2":{"start":{"line":2,"column":0},"end":{"line":2,"column":5}}},"fnMap":{},"branchMap":{},"s":{"0":1,"1":0,"2":0},"f":{},"b":{}}}"#;
        let f = parse_single(json);
        let s = CoverageSummary::from_file(&f);
        // Statements 0 and 1 both start on line 1, and statement 0 was hit.
        assert_eq!(s.lines, Metric { total: 2, covered: 1, skipped: 0, pct: 50.0 });
    }

    #[test]
    fn merge_sums_counts_and_recomputes_pct() {
        let mut a = Metric::new(10, 5);
        let b = Metric::new(10, 8);
        a.merge(&b);
        assert_eq!(a, Metric { total: 20, covered: 13, skipped: 0, pct: 65.0 });
    }
}
