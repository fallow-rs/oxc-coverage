//! Byte-span to istanbul position conversion, and registration of statement,
//! function and branch entries under the eager-compose remap gate.

use oxc_span::Span;

use oxc_coverage_types::{BranchEntry, FnEntry, Location, Position};

use super::CoverageTransform;

/// Identity of a coverage point after eager-compose resolution: the resolved
/// source index plus the remapped original endpoints. Two generated spans with
/// the same key fold into one entry when the canonicalizing remap merges by
/// location, so the eager gate hands them one shared counter id up front.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct EagerMergeKey {
    source: u32,
    start_line: u32,
    start_column: u32,
    end_line: u32,
    end_column: u32,
}

impl From<(u32, Location)> for EagerMergeKey {
    fn from((source, location): (u32, Location)) -> Self {
        Self {
            source,
            start_line: location.start.line,
            start_column: location.start.column,
            end_line: location.end.line,
            end_column: location.end.column,
        }
    }
}

/// Fold key for a branch after eager-compose resolution: the resolved source
/// plus the remapped surviving-arm location vector. Mirrors `merge_branches`,
/// which keys a fold on the arm `locations` vector alone: the umbrella `loc`
/// and the `branch_type` are excluded so two branches that widen to one
/// original arm vector share a counter even when their umbrella or type
/// differ, exactly as the canonicalizing merge folds them.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct BranchKey {
    source: u32,
    arms: Vec<(u32, u32, u32, u32)>,
}

/// One branch arm collected before the umbrella id is chosen. A missing
/// `location_span` becomes Istanbul's empty synthetic location. `body_span` is
/// the v8 side-table range and can differ from the reported location.
pub(super) struct PendingArm {
    pub(super) location_span: Option<Span>,
    pub(super) body_span: Span,
}

impl PendingArm {
    /// Arm whose reported location and v8 body span coincide, the common case.
    pub(super) const fn new(span: Span) -> Self {
        Self { location_span: Some(span), body_span: span }
    }

    /// Arm whose istanbul-reported location and v8 body span differ (if-arm 0
    /// and the else-arm).
    pub(super) const fn with_body(location_span: Span, body_span: Span) -> Self {
        Self { location_span: Some(location_span), body_span }
    }

    /// Synthetic arm whose Istanbul-compatible reported location is empty.
    pub(super) const fn empty_with_body(body_span: Span) -> Self {
        Self { location_span: None, body_span }
    }
}

/// A branch whose arms are collected but whose id is not yet assigned.
pub(super) struct PendingBranch {
    pub(super) branch_type: &'static str,
    pub(super) umbrella_span: Span,
    /// `false` for logical binary-expr, optional-chain and logical-assignment,
    /// whose arm vector must match the wrapped leaves / fixed helper indices
    /// one for one, so an arm is never individually dropped. `true` elsewhere.
    pub(super) gate_arms: bool,
    pub(super) arms: Vec<PendingArm>,
}

/// The umbrella id and per-input-arm surviving slot returned by
/// [`CoverageTransform::register_branch`].
pub(super) struct BranchRegistration {
    pub(super) branch_id: usize,
    /// `path_indices[k]` is the slot for input arm `k`, or `None` when the
    /// per-arm gate dropped it. Contiguous over surviving arms; on a dedup hit
    /// these index the shared entry's arms, which coincide one for one by
    /// construction of the fold key.
    path_indices: Vec<Option<usize>>,
}

impl BranchRegistration {
    /// Slot for input arm `k`, if it survived.
    pub(super) fn slot(&self, arm: usize) -> Option<usize> {
        self.path_indices[arm]
    }
}

const fn location_parts(loc: &Location) -> (u32, u32, u32, u32) {
    (loc.start.line, loc.start.column, loc.end.line, loc.end.column)
}

impl CoverageTransform<'_, '_> {
    /// Whether an istanbul `Location` survives `getMapping` resolution through
    /// the eager-compose input source map, which is the same decision the
    /// deferred `drop_unmapped` prune makes. Resolution is per-span through
    /// `getMapping` rather than per-endpoint through a greatest-lower-bound
    /// lookup, so the two paths agree by construction. `true` when no remapper
    /// is set, so gating is a strict no-op outside eager mode.
    fn location_maps(&self, loc: &Location) -> bool {
        self.eager_remapper.as_ref().is_none_or(|r| r.location_maps(loc))
    }

    /// The eager merge key for `loc`: `None` outside eager mode, where every
    /// registration keeps its own id.
    fn eager_merge_key(&self, loc: &Location) -> Option<EagerMergeKey> {
        self.eager_remapper.as_ref().and_then(|r| r.remap_location(loc)).map(EagerMergeKey::from)
    }

    fn span_to_location(&self, span: Span) -> Location {
        Location {
            start: self.offset_to_position(span.start),
            end: self.offset_to_position(span.end),
        }
    }

    fn offset_to_position(&self, offset: u32) -> Position {
        let line = self.line_offsets.partition_point(|&o| o <= offset).saturating_sub(1);
        let line_start = self.line_offsets[line] as usize;
        let end = (offset as usize).min(self.source.len());
        // Istanbul and Babel report columns as UTF-16 code units (JavaScript
        // string indices), not UTF-8 bytes. For an ASCII source the byte
        // distance equals the UTF-16 distance; otherwise the chars have to be
        // walked and their UTF-16 widths summed.
        let column = if self.source_is_ascii {
            end - line_start
        } else {
            self.source[line_start..end].chars().map(char::len_utf16).sum::<usize>()
        };
        Position {
            line: u32::try_from(line + 1).unwrap_or(u32::MAX),
            column: u32::try_from(column).unwrap_or(u32::MAX),
        }
    }

    /// Register a function entry. In eager mode returns `None` when any of the
    /// four endpoints (`decl` start/end, `loc` start/end) fails to remap,
    /// mirroring the function prune in `prune_single_source_unmapped`: the
    /// entry is not pushed and the caller must skip the function counter. A function whose remapped `decl` collides
    /// with an earlier one gets that entry's id instead of a new one, matching
    /// the decl-keyed fold `merge_file_coverage` applies; the shared counter
    /// then sums the hits the deferred path would merge. Outside eager mode
    /// this always returns `Some` with a fresh id.
    pub(super) fn add_function(
        &mut self,
        name: String,
        decl_span: Span,
        body_span: Span,
    ) -> Option<usize> {
        let decl = self.span_to_location(decl_span);
        let loc = self.span_to_location(body_span);
        if !self.location_maps(&decl) || !self.location_maps(&loc) {
            return None;
        }
        let key = self.eager_merge_key(&decl);
        if let Some(key) = &key
            && let Some(&id) = self.eager_function_ids.get(key)
        {
            // The deferred merge drops the whole `x_fallow_functionMap` overlay
            // when two functions fold onto one decl with differing identities
            // (`merge_functions` via `function_identities_equal`). Two distinct
            // AST functions always sit at distinct generated decl positions, so
            // any real fold is a conflict; record it field-for-field on the
            // same inputs the deferred comparison uses so `finalize` drops the
            // overlay exactly when the deferred path would.
            if let Some(existing) = self.fn_map.get(id)
                && (existing.name != name
                    || location_parts(&existing.decl) != location_parts(&decl)
                    || location_parts(&existing.loc) != location_parts(&loc))
            {
                self.eager_function_overlay_conflict = true;
            }
            return Some(id);
        }
        let id_num = self.fn_map.len();
        if let Some(key) = key {
            self.eager_function_ids.insert(key, id_num);
        }
        // istanbul-lib-instrument takes the deprecated `line` from the body, not
        // the declaration, so a multi-line parameter list moves it down.
        let line = loc.start.line;
        self.fn_map.push(FnEntry { name, line, decl, loc });
        Some(id_num)
    }

    /// Register a statement location. In eager mode returns `None` when either
    /// endpoint fails to remap, mirroring the statement prune in
    /// `prune_single_source_unmapped`: the location is not pushed and the
    /// caller must skip the statement counter. A statement
    /// whose remapped location collides with an earlier one gets that entry's
    /// id instead of a new one, matching the location-keyed fold
    /// `merge_file_coverage` applies; the shared counter then sums the hits the
    /// deferred path would merge. Outside eager mode this always returns `Some`
    /// with a fresh id.
    pub(super) fn add_statement(&mut self, span: Span) -> Option<usize> {
        let loc = self.span_to_location(span);
        if !self.location_maps(&loc) {
            return None;
        }
        let key = self.eager_merge_key(&loc);
        if let Some(key) = &key
            && let Some(&id) = self.eager_statement_ids.get(key)
        {
            return Some(id);
        }
        let id_num = self.statement_map.len();
        if let Some(key) = key {
            self.eager_statement_ids.insert(key, id_num);
        }
        self.statement_map.push(loc);
        Some(id_num)
    }

    /// The eager fold key for a branch: `Some` only in eager mode when every
    /// surviving arm remaps to one shared source, mirroring the single-source
    /// arm vector `merge_branches` folds on. `None` (no fold, fresh id) outside
    /// eager mode, when any arm fails to remap, or when arms span more than one
    /// source. A `None` key never over-folds relative to the deferred path,
    /// which within one output file only ever merges a single source.
    fn eager_branch_key(&self, surviving_locs: &[Location]) -> Option<BranchKey> {
        let remapper = self.eager_remapper.as_ref()?;
        let mut source = None;
        let mut arms = Vec::with_capacity(surviving_locs.len());
        for loc in surviving_locs {
            let (arm_source, mapped) = remapper.remap_location(loc)?;
            if *source.get_or_insert(arm_source) != arm_source {
                return None;
            }
            arms.push(location_parts(&mapped));
        }
        Some(BranchKey { source: source?, arms })
    }

    /// Register a branch whose arms are already collected, returning the
    /// umbrella id and the surviving slot per input arm. `None` when the branch
    /// is not instrumented at all, which only happens in eager mode: no arm
    /// survives the per-arm gate, or the umbrella `loc` fails to remap and no
    /// arm remaps either. An umbrella that fails while an arm still remaps
    /// keeps the branch and takes that arm's location as its `loc`, the
    /// fallback `prune_single_source_unmapped` and `fan_out_branches` apply on
    /// the deferred path. The single source of the gate, the fold and the
    /// dedup for every branch type.
    ///
    /// Outside eager mode this always allocates a fresh id, keeps every arm and
    /// pushes one entry, so it is byte-identical to the former `add_branch` plus
    /// per-arm push it replaces, including the empty entry a zero-arm switch
    /// pushes to preserve the id-burn `build_file_coverage` relies on. In eager
    /// mode a branch whose remapped surviving-arm vector collides with an
    /// earlier one gets that entry's id and pushes nothing, so both branches'
    /// arm counters sum onto the shared slot the way `merge_branches` sums them.
    pub(super) fn register_branch(&mut self, pending: PendingBranch) -> Option<BranchRegistration> {
        let PendingBranch { branch_type, umbrella_span, gate_arms, arms } = pending;
        let umbrella = self.span_to_location(umbrella_span);
        let mut path_indices = Vec::with_capacity(arms.len());
        let mut surviving_locs = Vec::new();
        let mut body_spans = Vec::new();
        for arm in &arms {
            let loc = arm.location_span.map_or_else(
                || Location {
                    start: Position { line: 0, column: 0 },
                    end: Position { line: 0, column: 0 },
                },
                |span| self.span_to_location(span),
            );
            if gate_arms && arm.location_span.is_some() && !self.location_maps(&loc) {
                path_indices.push(None);
                continue;
            }
            path_indices.push(Some(surviving_locs.len()));
            surviving_locs.push(loc);
            body_spans.push((arm.body_span.start, arm.body_span.end));
        }
        // A branch whose arms all fail to remap is dropped on the deferred path
        // (`prune_single_source_unmapped` / `fan_out_branches`), even when its
        // umbrella maps; returning `None` instruments nothing and consumes no
        // id, matching that drop and keeping the eager `branchMap` ids
        // contiguous. Only reachable in eager mode: outside it `location_maps`
        // never rejects an arm, so an all-cases-ignored switch still falls
        // through to the empty push below.
        if self.eager_remapper.is_some() && surviving_locs.is_empty() {
            return None;
        }
        // The deferred path rejects on the umbrella only when no arm remaps
        // either; otherwise it keeps the branch and reassigns `loc` from the
        // first surviving arm (`prune_single_source_unmapped` /
        // `fan_out_branches`). Storing that arm's generated location here makes
        // the finalize remap resolve it to the same original position.
        let entry_loc = if self.location_maps(&umbrella) {
            umbrella
        } else {
            surviving_locs.iter().find(|loc| self.location_maps(loc)).cloned()?
        };
        let key = self.eager_branch_key(&surviving_locs);
        if let Some(key) = &key
            && let Some(&branch_id) = self.eager_branch_ids.get(key)
        {
            return Some(BranchRegistration { branch_id, path_indices });
        }
        let branch_id = self.branch_map.len();
        if let Some(key) = key {
            self.eager_branch_ids.insert(key, branch_id);
        }
        let line = entry_loc.start.line;
        self.branch_map.push(BranchEntry {
            loc: entry_loc,
            line,
            branch_type: branch_type.to_string(),
            locations: surviving_locs,
        });
        self.branch_arm_body_byte_spans.push(body_spans);
        Some(BranchRegistration { branch_id, path_indices })
    }
}

/// True for nodes whose byte span is `(0, 0)`: nodes synthesized during the
/// transform that have no anchor in the original source, such as the
/// `typeof X === "function" ? X : Object` guards the legacy decorator
/// metadata pass inserts. Registering a branch for one would inflate the
/// branch denominator with a location that maps back to L1:C0 and that the
/// user cannot act on.
pub(super) fn is_synthetic_span(span: Span) -> bool {
    span.start == 0 && span.end == 0
}
