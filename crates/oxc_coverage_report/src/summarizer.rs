//! Build a [`ReportNode`] tree from a coverage map.
//!
//! The tree is rooted at the longest path prefix shared by every file. Folders
//! nest below it, each leaf is a file, and every folder's summary is the merge
//! of its children's.
//!
//! Only `/` is treated as a separator. A Windows-style `\` path stays a single
//! segment, producing a flat tree rather than an incorrect one.

use std::collections::BTreeMap;

use oxc_coverage_types::FileCoverage;

use crate::summary::CoverageSummary;
use crate::tree::{NodeKind, ReportNode};

/// Alias for the deserialized `coverage-final.json` shape.
///
/// `BTreeMap<String, FileCoverage>` keyed by file path is what
/// [`oxc_coverage_types::parse_coverage_map`] returns.
pub type CoverageMap = BTreeMap<String, FileCoverage>;

/// Build a folder/file tree from a flat [`CoverageMap`].
///
/// The returned root has `name = ""` and `relative_path = ""`, and each
/// [`FileCoverage`] is cloned into its leaf node. An empty input yields a
/// childless root whose metrics are all zero-total, so `pct = 100.0` per the
/// istanbul convention.
pub fn summarize(map: &CoverageMap) -> ReportNode {
    if map.is_empty() {
        return ReportNode {
            name: String::new(),
            relative_path: String::new(),
            summary: CoverageSummary::default(),
            kind: NodeKind::Folder { children: Vec::new() },
        };
    }

    let paths: Vec<Vec<&str>> = map.keys().map(|key| split_path(key)).collect();
    let prefix_len = common_prefix_len(&paths);

    let mut root = FolderBuilder::default();
    for (segments, file) in paths.iter().zip(map.values()) {
        insert(&mut root, &segments[prefix_len..], file.clone());
    }

    build_node(String::new(), String::new(), root)
}

fn split_path(path: &str) -> Vec<&str> {
    path.split('/').filter(|s| !s.is_empty()).collect()
}

fn common_prefix_len(paths: &[Vec<&str>]) -> usize {
    if paths.is_empty() {
        return 0;
    }
    let first = &paths[0];
    // Leave at least the leaf segment so a single-file input still has a name.
    let max_prefix = first.len().saturating_sub(1);
    let mut len = 0;
    while len < max_prefix {
        let seg = first[len];
        if !paths.iter().all(|p| p.get(len).copied() == Some(seg)) {
            break;
        }
        len += 1;
    }
    len
}

#[derive(Default)]
struct FolderBuilder {
    subfolders: BTreeMap<String, FolderBuilder>,
    files: BTreeMap<String, FileCoverage>,
}

fn insert(folder: &mut FolderBuilder, segments: &[&str], file: FileCoverage) {
    match segments {
        // Unreachable: `prefix_len` is bounded by `paths[0].len() - 1`, and a
        // strict segment-prefix of `paths[0]` would sort before it in the
        // `BTreeMap`, so no path is shorter than the prefix.
        [] => {}
        [leaf] => {
            folder.files.insert((*leaf).to_owned(), file);
        }
        [head, rest @ ..] => {
            let child = folder.subfolders.entry((*head).to_owned()).or_default();
            insert(child, rest, file);
        }
    }
}

fn join(parent: &str, segment: &str) -> String {
    if parent.is_empty() { segment.to_owned() } else { format!("{parent}/{segment}") }
}

fn build_node(name: String, rel: String, folder: FolderBuilder) -> ReportNode {
    let mut children: Vec<ReportNode> =
        Vec::with_capacity(folder.subfolders.len() + folder.files.len());
    let mut summary = CoverageSummary::default();

    for (sub_name, sub_folder) in folder.subfolders {
        let sub_rel = join(&rel, &sub_name);
        let child = build_node(sub_name, sub_rel, sub_folder);
        summary.merge(&child.summary);
        children.push(child);
    }
    for (file_name, coverage) in folder.files {
        let file_rel = join(&rel, &file_name);
        let file_summary = CoverageSummary::from_file(&coverage);
        summary.merge(&file_summary);
        children.push(ReportNode {
            name: file_name,
            relative_path: file_rel,
            summary: file_summary,
            kind: NodeKind::File { coverage: Box::new(coverage) },
        });
    }

    ReportNode { name, relative_path: rel, summary, kind: NodeKind::Folder { children } }
}

#[cfg(test)]
mod tests {
    use oxc_coverage_types::parse_coverage_map;

    use super::*;

    fn empty_file(path: &str, hit: bool) -> String {
        let s_val = u32::from(hit);
        format!(
            r#"{{"path":"{path}","statementMap":{{"0":{{"start":{{"line":1,"column":0}},"end":{{"line":1,"column":1}}}}}},"fnMap":{{}},"branchMap":{{}},"s":{{"0":{s_val}}},"f":{{}},"b":{{}}}}"#
        )
    }

    #[test]
    fn empty_map_returns_empty_root() {
        let map: CoverageMap = BTreeMap::new();
        let root = summarize(&map);
        assert!(root.is_folder());
        assert!(root.children().is_empty());
        assert_eq!(root.summary.statements.total, 0);
        assert_eq!(root.summary.statements.pct, 100.0);
    }

    #[test]
    fn single_file_has_no_extra_folder_wrapper() {
        let json = format!("{{\"a.js\":{}}}", empty_file("a.js", true));
        let map = parse_coverage_map(&json).unwrap();
        let root = summarize(&map);
        assert_eq!(root.children().len(), 1);
        let leaf = &root.children()[0];
        assert_eq!(leaf.name, "a.js");
        assert_eq!(leaf.relative_path, "a.js");
        assert!(leaf.is_file());
    }

    #[test]
    fn shared_prefix_is_stripped() {
        let json = format!(
            "{{\"src/a.js\":{},\"src/b.js\":{}}}",
            empty_file("src/a.js", true),
            empty_file("src/b.js", false)
        );
        let map = parse_coverage_map(&json).unwrap();
        let root = summarize(&map);
        let names: Vec<_> = root.children().iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["a.js", "b.js"]);
        assert_eq!(root.summary.statements.total, 2);
        assert_eq!(root.summary.statements.covered, 1);
        assert_eq!(root.summary.statements.pct, 50.0);
    }

    #[test]
    fn divergent_paths_build_subfolders() {
        let json = format!(
            "{{\"src/a/x.js\":{},\"src/b/y.js\":{}}}",
            empty_file("src/a/x.js", true),
            empty_file("src/b/y.js", true)
        );
        let map = parse_coverage_map(&json).unwrap();
        let root = summarize(&map);
        let names: Vec<_> = root.children().iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["a", "b"]);
        assert!(root.children().iter().all(ReportNode::is_folder));
        for child in root.children() {
            assert_eq!(child.children().len(), 1);
            assert!(child.children()[0].is_file());
        }
    }

    #[test]
    fn rollup_sums_across_subfolders() {
        let json = format!(
            "{{\"src/a/x.js\":{},\"src/b/y.js\":{}}}",
            empty_file("src/a/x.js", true),
            empty_file("src/b/y.js", false)
        );
        let map = parse_coverage_map(&json).unwrap();
        let root = summarize(&map);
        assert_eq!(root.summary.statements, crate::Metric::new(2, 1));
    }
}
