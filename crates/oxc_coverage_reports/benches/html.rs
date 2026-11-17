//! Whole-directory HTML emit, including syntect tokenization of real
//! source files on disk, at two project sizes.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use oxc_coverage_report::CoverageMap;
use oxc_coverage_reports::html::{self, HtmlOptions};
use oxc_coverage_types::{BranchEntry, FileCoverage, FnEntry, Location, Position};
use tempfile::TempDir;

const CASES: &[(&str, usize, u32)] = &[("small", 4, 40), ("medium", 16, 80)];

fn loc(line: u32, start_col: u32, end_col: u32) -> Location {
    Location {
        start: Position { line, column: start_col },
        end: Position { line, column: end_col },
    }
}

fn source_for(lines: u32, file_id: usize) -> String {
    let mut source = String::new();
    let _ = writeln!(source, "export function compute{file_id}(input: number): number {{");
    source.push_str("  let total = input;\n");
    for line in 2..lines {
        if line % 9 == 0 {
            let _ = writeln!(source, "  total += input > {line} ? {line} : -{line};");
        } else if line % 5 == 0 {
            let _ = writeln!(source, "  total += input && {line};");
        } else {
            let _ = writeln!(source, "  total += {line};");
        }
    }
    source.push_str("  return total;\n}\n");
    source
}

fn coverage_for(path: String, lines: u32) -> FileCoverage {
    let mut statement_map = BTreeMap::new();
    let mut fn_map = BTreeMap::new();
    let mut branch_map = BTreeMap::new();
    let mut s = BTreeMap::new();
    let mut f = BTreeMap::new();
    let mut b = BTreeMap::new();

    for line in 1..=lines {
        let key = (line - 1).to_string();
        statement_map.insert(key.clone(), loc(line, 2, 18));
        s.insert(key, u32::from(line % 7 != 0));
    }

    fn_map.insert(
        "0".to_string(),
        FnEntry { name: "compute".to_string(), line: 1, decl: loc(1, 16, 23), loc: loc(1, 0, 42) },
    );
    f.insert("0".to_string(), 1);

    for (branch_id, line) in (9..=lines).step_by(9).enumerate() {
        let key = branch_id.to_string();
        branch_map.insert(
            key.clone(),
            BranchEntry {
                loc: loc(line, 11, 34),
                line,
                branch_type: "cond-expr".to_string(),
                locations: vec![loc(line, 25, 27), loc(line, 31, 34)],
            },
        );
        b.insert(key, vec![1, u32::from(line % 2 == 0)]);
    }

    FileCoverage {
        path,
        statement_map,
        fn_map,
        branch_map,
        s,
        f,
        b,
        b_t: None,
        input_source_map: None,
        x_fallow_function_map: None,
    }
}

/// Writes `file_count` TypeScript sources under a temporary root and
/// returns them alongside a coverage map keyed by their relative paths.
/// Both directories are returned so they outlive the benchmark.
fn fixture(file_count: usize, lines: u32) -> (TempDir, TempDir, CoverageMap) {
    let root = tempfile::tempdir().expect("fixture root tempdir");
    let output = tempfile::tempdir().expect("fixture output tempdir");
    let mut map = BTreeMap::new();

    for file_id in 0..file_count {
        let relative = format!("src/pkg{}/file{file_id}.ts", file_id % 8);
        let absolute = root.path().join(&relative);
        fs::create_dir_all(absolute.parent().expect("source file parent")).expect("mkdir source");
        fs::write(&absolute, source_for(lines, file_id)).expect("write source");
        map.insert(relative.clone(), coverage_for(relative, lines));
    }

    (root, output, map)
}

/// Make the bench thread the only thread of the global rayon pool.
///
/// The HTML writer fans detail pages out over rayon. With a separate worker,
/// the bench thread blocks on a futex until the work is done, and CodSpeed
/// counts that wait as system call time. The benchmark then reads as
/// "dominated by system calls", and a change of the wait moves the value.
/// When the bench thread is the pool, it runs the work inline and never waits.
#[cfg(codspeed)]
fn run_rayon_on_bench_thread() {
    rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .use_current_thread()
        .build_global()
        .expect("the benchmark builds the global rayon pool first");
}

fn bench_html(c: &mut Criterion) {
    #[cfg(codspeed)]
    run_rayon_on_bench_thread();
    let options = HtmlOptions::default();
    let mut group = c.benchmark_group("html_report");

    for (label, files, lines) in CASES {
        let (root, output, map) = fixture(*files, *lines);
        group.bench_with_input(BenchmarkId::new("write", label), &map, |b, map| {
            b.iter(|| {
                html::write(map, root.path(), output.path(), &options).expect("html report writes");
            });
        });
    }

    group.finish();
}

criterion_group!(benches, bench_html);
criterion_main!(benches);
