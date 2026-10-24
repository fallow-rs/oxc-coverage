//! Lookup state threaded through a remap pass: the borrowed source map, the
//! caches that keep repeated `getMapping` lookups cheap, and the key and result
//! types those lookups are stated in.

use std::collections::{BTreeMap, BTreeSet};

use oxc_coverage_types::Location;
use srcmap_sourcemap::SourceMap;

/// getMapping lookup caches.
///
/// Kept separate from the `&SourceMap` borrow so they can outlive a single
/// [`RemapContext`]: [`PositionRemapper`] owns one set and reuses it across
/// every eager-gate call, while a file remap uses a fresh set per coverage map.
///
/// [`PositionRemapper`]: crate::PositionRemapper
#[derive(Default)]
pub struct RemapCaches {
    pub mapping_cache: BTreeMap<LocationKey, Option<MappedLocation>>,
    pub original_line_columns: BTreeMap<OriginalLineKey, Vec<u32>>,
    /// Sources whose lines are all in `original_line_columns`.
    pub indexed_sources: BTreeSet<u32>,
    // Each cache belongs to one SourceMap, so source indices are stable keys.
    original_line_ends: BTreeMap<u32, OriginalLineEndCache>,
}

impl RemapCaches {
    /// UTF-16 column that ends `line` of the original source, used to clamp
    /// istanbul's `column: Infinity` end. Resolved from `sourcesContent` when
    /// present, otherwise from the greatest original column mapped on that line.
    pub fn original_line_end_column(&mut self, sm: &SourceMap, source_idx: u32, line: u32) -> u32 {
        let content = usize::try_from(source_idx)
            .ok()
            .and_then(|source_idx| sm.sources_content.get(source_idx))
            .and_then(Option::as_deref);
        let cache = self.original_line_ends.entry(source_idx).or_default();
        if let Some(content) = content
            && let Some(column) = cache.content.line_end(content, line)
        {
            return column;
        }
        cache.mapped_line_end(sm, source_idx, line)
    }
}

#[derive(Default)]
struct OriginalLineEndCache {
    content: ContentLineEnds,
    mapped: Option<MappedLineEnds>,
}

impl OriginalLineEndCache {
    fn mapped_line_end(&mut self, sm: &SourceMap, source_idx: u32, line: u32) -> u32 {
        self.mapped.get_or_insert_with(|| MappedLineEnds::from_source_map(sm, source_idx)).get(line)
    }
}

// Bound dense storage so an untrusted original line cannot force a large allocation.
const MAX_DENSE_LINE_ENDS: usize = 65_536;
// Dense storage is worthwhile only while the highest line index stays within
// this multiple of the mapping count.
const MAX_DENSE_LINE_RATIO: usize = 4;

enum MappedLineEnds {
    Dense(Vec<u32>),
    Sparse(BTreeMap<u32, u32>),
}

impl MappedLineEnds {
    fn from_source_map(sm: &SourceMap, source_idx: u32) -> Self {
        let mappings: Vec<(u32, u32)> = sm
            .all_mappings()
            .iter()
            .filter(|mapping| mapping.source == source_idx)
            .map(|mapping| (mapping.original_line, mapping.original_column))
            .collect();
        let dense_len = mappings
            .iter()
            .map(|(line, _)| *line)
            .max()
            .and_then(|line| usize::try_from(line).ok())
            .and_then(|line| line.checked_add(1));
        let use_dense = dense_len.is_some_and(|len| {
            len <= MAX_DENSE_LINE_ENDS && len <= mappings.len().saturating_mul(MAX_DENSE_LINE_RATIO)
        });

        if use_dense {
            let mut line_ends = vec![0; dense_len.unwrap_or(0)];
            for (line, column) in mappings {
                if let Ok(line) = usize::try_from(line) {
                    line_ends[line] = line_ends[line].max(column);
                }
            }
            Self::Dense(line_ends)
        } else {
            let mut line_ends: BTreeMap<u32, u32> = BTreeMap::new();
            for (line, column) in mappings {
                line_ends.entry(line).and_modify(|end| *end = (*end).max(column)).or_insert(column);
            }
            Self::Sparse(line_ends)
        }
    }

    fn get(&self, line: u32) -> u32 {
        match self {
            Self::Dense(line_ends) => usize::try_from(line)
                .ok()
                .and_then(|line| line_ends.get(line))
                .copied()
                .unwrap_or(0),
            Self::Sparse(line_ends) => line_ends.get(&line).copied().unwrap_or(0),
        }
    }
}

#[derive(Default)]
struct ContentLineEnds {
    // Columns are populated only through the highest requested content line.
    columns: Vec<u32>,
    next_byte: usize,
    complete: bool,
}

impl ContentLineEnds {
    fn line_end(&mut self, content: &str, line: u32) -> Option<u32> {
        let line = usize::try_from(line).ok()?;
        while self.columns.len() <= line && !self.complete {
            let remaining = &content[self.next_byte..];
            if let Some(newline) = remaining.find('\n') {
                self.columns.push(utf16_line_len(&remaining[..newline]));
                self.next_byte += newline + 1;
            } else {
                self.columns.push(utf16_line_len(remaining));
                self.next_byte = content.len();
                self.complete = true;
            }
        }
        self.columns.get(line).copied()
    }
}

fn utf16_line_len(text: &str) -> u32 {
    let text = text.strip_suffix('\r').unwrap_or(text);
    u32::try_from(text.encode_utf16().count()).unwrap_or(u32::MAX)
}

/// A remapped location together with the index of the original source it
/// resolved to.
#[derive(Clone)]
pub struct MappedLocation {
    pub source: u32,
    pub location: Location,
}

/// The source map and caches a single remap pass looks positions up in.
pub struct RemapContext<'a> {
    pub sm: &'a SourceMap,
    pub caches: &'a mut RemapCaches,
}

impl<'a> RemapContext<'a> {
    pub fn new(sm: &'a SourceMap, caches: &'a mut RemapCaches) -> Self {
        Self { sm, caches }
    }
}

/// Cache key for one `getMapping` lookup: the generated span it resolves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct LocationKey {
    start_line: u32,
    start_column: u32,
    end_line: u32,
    end_column: u32,
}

impl From<&Location> for LocationKey {
    fn from(loc: &Location) -> Self {
        Self {
            start_line: loc.start.line,
            start_column: loc.start.column,
            end_line: loc.end.line,
            end_column: loc.end.column,
        }
    }
}

/// Cache key for the sorted original-column index of one `(source, line)` pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct OriginalLineKey {
    pub source: u32,
    pub line: u32,
}
