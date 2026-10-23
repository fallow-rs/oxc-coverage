//! Istanbul/v8 coverage pragma comment handling.
//!
//! Scans AST comments for `istanbul ignore` and `v8 ignore` directives,
//! building a lookup table that the coverage transform uses to skip
//! instrumentation for specific nodes.

use std::collections::{BTreeMap, BTreeSet};

use oxc_ast::{
    AstKind,
    ast::{Comment, Expression, ParenthesizedExpression, Program},
};
use oxc_ast_visit::{Visit, walk};
use oxc_span::GetSpan;

use oxc_coverage_types::UnhandledPragma;

use crate::source_text;

/// Narrow a byte index to the `u32` offset width `oxc_span::Span` uses.
/// Saturating rather than wrapping, so a pathological source cannot produce
/// an offset that points at unrelated text.
fn to_offset(index: usize) -> u32 {
    u32::try_from(index).unwrap_or(u32::MAX)
}

/// Type of coverage ignore directive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IgnoreType {
    /// `/* istanbul ignore next */` or `/* v8 ignore next */`
    /// Skip the next node (statement, function, class, etc.)
    Next,
    /// `/* istanbul ignore if */`
    /// Skip the if branch of an if statement.
    If,
    /// `/* istanbul ignore else */`
    /// Skip the else branch of an if statement.
    Else,
}

/// Lookup table of coverage ignore directives, keyed by the start offset
/// of the token the comment is attached to.
pub struct PragmaMap {
    /// Maps token start offset -> ignore type.
    ignores: BTreeMap<u32, IgnoreType>,
    /// Ranges ignored by `ignore start` / `ignore stop` block pragmas.
    ignored_ranges: Vec<(u32, u32)>,
    /// `ignore next` keys whose outermost node is a type that istanbul does
    /// not visit, such as the call in `/* istanbul ignore next */ f().g()`.
    /// See [`PragmaMap::get_for_node`].
    wrapper_owned: BTreeSet<u32>,
    /// Whether the entire file should be ignored.
    pub ignore_file: bool,
}

impl PragmaMap {
    /// Build a pragma map from the program's comments and source text.
    /// `istanbul_compat` limits the map to the pragmas that istanbul reads.
    pub fn from_program(
        program: &Program,
        source: &str,
        istanbul_compat: bool,
    ) -> (Self, Vec<UnhandledPragma>) {
        let mut state = PragmaCollect { hops: !istanbul_compat, ..PragmaCollect::default() };
        let mut comments: Vec<_> = program.comments.iter().collect();
        comments.sort_by_key(|comment| comment.span.start);

        for comment in comments {
            let text = Self::comment_text(comment, source);
            if let Some(result) = Self::parse_pragma(&text, istanbul_compat) {
                state.apply(result, comment, source);
            }
        }

        if let Some(start) = state.active_ignore_start {
            state.ignored_ranges.push((start, to_offset(source.len())));
        }
        let mut wrapper_owned = BTreeSet::new();
        if state.ignores.values().any(|&kind| kind == IgnoreType::Next) {
            let mut owners = PragmaOwners {
                ignores: &mut state.ignores,
                parents: Vec::new(),
                seen: BTreeSet::new(),
                wrapper_owned: &mut wrapper_owned,
            };
            owners.visit_program(program);
        }

        (
            Self {
                ignores: state.ignores,
                ignored_ranges: state.ignored_ranges,
                wrapper_owned,
                ignore_file: state.ignore_file,
            },
            state.unhandled,
        )
    }

    /// Get the ignore type for a given token start offset.
    pub fn get(&self, token_start: u32) -> Option<IgnoreType> {
        self.ignores
            .get(&token_start)
            .copied()
            .or_else(|| self.in_ignored_range(token_start).then_some(IgnoreType::Next))
    }

    /// Get the ignore type for a function, arrow, conditional or logical
    /// expression that starts at `node_start`.
    ///
    /// Babel attaches a comment to the outermost node after it. When that node
    /// is a type istanbul does not visit, such as a call, a member or a
    /// sequence expression, the pragma has no effect on a node that only
    /// shares its start: `function () {}` in `function () {}.call(this)`.
    pub fn get_for_node(&self, node_start: u32) -> Option<IgnoreType> {
        if self.wrapper_owned.contains(&node_start) {
            return self.in_ignored_range(node_start).then_some(IgnoreType::Next);
        }
        self.get(node_start)
    }

    fn in_ignored_range(&self, offset: u32) -> bool {
        self.ignored_ranges.iter().any(|&(start, end)| offset >= start && offset < end)
    }

    /// Remove the `ignore next` pragma keyed at `start`, if there is one.
    /// Istanbul wraps some values in a counter with Babel's `replaceWith`,
    /// which moves the leading comments of the value to the wrapper. The value
    /// itself then has no pragma. `ignore start` ranges stay in effect.
    pub(crate) fn consume_next(&mut self, start: u32) {
        if self.ignores.get(&start) == Some(&IgnoreType::Next) {
            self.ignores.remove(&start);
        }
    }

    /// Extract comment text from source.
    fn comment_text(comment: &Comment, source: &str) -> String {
        let content_span = comment.content_span();
        source[content_span.start as usize..content_span.end as usize].to_string()
    }

    fn next_token_start(source: &str, offset: u32) -> Option<u32> {
        let mut cursor = offset as usize;
        while cursor < source.len() {
            let ch = source[cursor..].chars().next()?;
            if ch.is_whitespace() {
                cursor += ch.len_utf8();
                continue;
            }

            let rest = &source[cursor..];
            if rest.starts_with("//") {
                if let Some((line_end, width)) = source_text::find_line_terminator(rest) {
                    cursor += line_end + width;
                } else {
                    return None;
                }
                continue;
            }
            if rest.starts_with("/*") {
                if let Some(end) = rest.find("*/") {
                    cursor += end + 2;
                    continue;
                }
                return None;
            }

            return Some(to_offset(cursor));
        }
        None
    }

    /// The start of the token a pragma comment targets. With `hops`, a
    /// pragma alone in an empty block moves to the next block or JSX element,
    /// and a pragma before `else if` moves to the chained `if`. Istanbul does
    /// neither, so the strict profile passes `hops: false`.
    fn pragma_target_start(source: &str, comment: &Comment, hops: bool) -> Option<u32> {
        let token_start = Self::next_token_start(source, comment.span.end)?;
        if !hops {
            return Some(token_start);
        }
        if source[token_start as usize..].starts_with('}')
            && Self::previous_non_whitespace(source, comment.span.start) == Some('{')
            && let Some(after_close) = Self::next_token_start(source, token_start + 1)
            && source[after_close as usize..].starts_with(['{', '<'])
        {
            return Some(after_close);
        }
        // `/* istanbul ignore <kind> */` placed between `else` and a chained
        // `else if` anchors to the inner `if`, not to the `else` keyword
        // itself. `IfStatement::span.start` is the `if` keyword; without
        // this hop the pragma never reaches the visitor. `word_at` matches
        // ASCII letters only, so `Some("else")` guarantees that the four
        // bytes starting at `token_start` are `b'e'`, `b'l'`, `b's'`, `b'e'`
        // and `token_start + 4` is a valid UTF-8 boundary.
        if Self::word_at(source, token_start) == Some("else")
            && let Some(after_else) = Self::next_token_start(source, token_start + 4)
            && Self::word_at(source, after_else) == Some("if")
        {
            return Some(after_else);
        }
        Some(token_start)
    }

    fn word_at(source: &str, offset: u32) -> Option<&str> {
        let rest = source.get(offset as usize..)?;
        let end = rest.find(|c: char| !c.is_ascii_alphabetic()).unwrap_or(rest.len());
        if end == 0 { None } else { Some(&rest[..end]) }
    }

    fn previous_non_whitespace(source: &str, offset: u32) -> Option<char> {
        source[..offset as usize].chars().rev().find(|ch| !ch.is_whitespace())
    }

    /// Parse a pragma comment text into an ignore type.
    ///
    /// Matches `<tool> ignore <kind>` where `<tool>` is one of `istanbul`, `v8`, `c8`,
    /// and `<kind>` is one of `next`, `if`, `else`, `file`, `start`, or `stop`. Any ASCII whitespace run
    /// (spaces, tabs, newlines) between tokens is accepted, matching Istanbul's behavior.
    /// As in Istanbul, `<kind>` ends at the first character that is not an
    /// ASCII letter, digit or `_`, so `next: reason` and `next-reason` are `next`.
    ///
    /// A single leading `!` (the legal-comment marker preserved by esbuild,
    /// terser, swc, and most other minifiers) is skipped before parsing so
    /// `/*! istanbul ignore next */` and `//! istanbul ignore next` are
    /// honored identically to their plain forms.
    ///
    /// With `istanbul_compat`, only the `istanbul ignore next|if|else|file`
    /// pragmas that `istanbul-lib-instrument` reads have an effect. The other
    /// forms (`v8`, `c8`, `start`, `stop` and the legal-comment marker) are
    /// reported as unhandled.
    fn parse_pragma(text: &str, istanbul_compat: bool) -> Option<PragmaResult> {
        let trimmed = text.trim();
        let legal_marker = trimmed.starts_with('!');
        let after_legal_marker = trimmed.strip_prefix('!').unwrap_or(trimmed);
        let mut tokens = after_legal_marker.split_whitespace();
        let tool = tokens.next()?;
        if !matches!(tool, "istanbul" | "v8" | "c8") {
            return None;
        }
        if tokens.next()? != "ignore" {
            return None;
        }
        let kind_token = tokens.next().unwrap_or("");
        let kind_end = kind_token
            .find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
            .unwrap_or(kind_token.len());
        let result = match &kind_token[..kind_end] {
            "next" => PragmaResult::Ignore(IgnoreType::Next),
            "if" => PragmaResult::Ignore(IgnoreType::If),
            "else" => PragmaResult::Ignore(IgnoreType::Else),
            "file" => PragmaResult::File,
            "start" => PragmaResult::Start,
            "stop" => PragmaResult::Stop,
            _ => return Some(PragmaResult::Unknown(trimmed.to_string())),
        };
        let istanbul_reads = !legal_marker
            && tool == "istanbul"
            && !matches!(result, PragmaResult::Start | PragmaResult::Stop);
        if istanbul_compat && !istanbul_reads {
            return Some(PragmaResult::Unknown(trimmed.to_string()));
        }
        Some(result)
    }
}

/// Finds the node that Babel attaches each `ignore next` pragma to.
///
/// Babel has no node for parentheses. It attaches a comment before `(` to the
/// outermost node that starts after the comment, and that is the expression in
/// the parentheses. Oxc keys the pragma at `(` instead, so the pass copies the
/// key to the expression. The exception is a parent that starts at the same
/// `(`, such as the call in `(f)()` or the logical expression in `(a) || b`:
/// Babel attaches the comment to that parent.
///
/// The pass also records each key whose outermost node is a type that istanbul
/// does not visit.
struct PragmaOwners<'m> {
    ignores: &'m mut BTreeMap<u32, IgnoreType>,
    /// The start offset of each node on the current path, and whether that
    /// node is a parenthesized expression.
    parents: Vec<(u32, bool)>,
    /// The `ignore next` keys whose outermost node the pass has found.
    seen: BTreeSet<u32>,
    wrapper_owned: &'m mut BTreeSet<u32>,
}

/// Node types that istanbul does not visit and that can start at the same
/// offset as a child function, conditional or logical expression.
const fn is_unvisited_wrapper(kind: &AstKind) -> bool {
    matches!(
        kind,
        AstKind::CallExpression(_)
            | AstKind::StaticMemberExpression(_)
            | AstKind::ComputedMemberExpression(_)
            | AstKind::PrivateFieldExpression(_)
            | AstKind::ChainExpression(_)
            | AstKind::TaggedTemplateExpression(_)
            | AstKind::SequenceExpression(_)
            | AstKind::BinaryExpression(_)
            | AstKind::AssignmentExpression(_)
            | AstKind::UpdateExpression(_)
            | AstKind::TSAsExpression(_)
            | AstKind::TSSatisfiesExpression(_)
            | AstKind::TSNonNullExpression(_)
            | AstKind::TSInstantiationExpression(_)
    )
}

impl PragmaOwners<'_> {
    fn alias_chain(&mut self, outer: &ParenthesizedExpression) {
        let inner_start = outer.expression.without_parentheses().span().start;
        let parent_start = self.parents.last().map(|&(start, _)| start);
        let mut paren_starts = Vec::new();
        if parent_start != Some(outer.span.start) {
            paren_starts.push(outer.span.start);
        }
        let mut expr = &outer.expression;
        while let Expression::ParenthesizedExpression(inner) = expr {
            paren_starts.push(inner.span.start);
            expr = &inner.expression;
        }
        if paren_starts.iter().any(|start| self.ignores.get(start) == Some(&IgnoreType::Next)) {
            self.ignores.entry(inner_start).or_insert(IgnoreType::Next);
        }
    }
}

impl<'a> Visit<'a> for PragmaOwners<'_> {
    fn enter_node(&mut self, kind: AstKind<'a>) {
        let start = kind.span().start;
        let is_paren = matches!(kind, AstKind::ParenthesizedExpression(_));
        // The walk enters an outer node before the nodes in it, so the first
        // node entered at a key is the outermost one.
        if !is_paren
            && self.ignores.get(&start) == Some(&IgnoreType::Next)
            && self.seen.insert(start)
            && is_unvisited_wrapper(&kind)
        {
            self.wrapper_owned.insert(start);
        }
        self.parents.push((start, is_paren));
    }

    fn leave_node(&mut self, _kind: AstKind<'a>) {
        self.parents.pop();
    }

    fn visit_parenthesized_expression(&mut self, it: &ParenthesizedExpression<'a>) {
        // Only the outermost parenthesis of a chain such as `((a))` starts
        // the alias. It reads the inner parentheses itself.
        if !self.parents.last().is_some_and(|&(_, is_paren)| is_paren) {
            self.alias_chain(it);
        }
        walk::walk_parenthesized_expression(self, it);
    }
}

enum PragmaResult {
    Ignore(IgnoreType),
    File,
    Start,
    Stop,
    Unknown(String),
}

#[derive(Default)]
struct PragmaCollect {
    ignores: BTreeMap<u32, IgnoreType>,
    ignored_ranges: Vec<(u32, u32)>,
    active_ignore_start: Option<u32>,
    /// Whether pragma targets may hop, see [`PragmaMap::pragma_target_start`].
    hops: bool,
    ignore_file: bool,
    unhandled: Vec<UnhandledPragma>,
}

impl PragmaCollect {
    fn apply(&mut self, result: PragmaResult, comment: &Comment, source: &str) {
        match result {
            PragmaResult::Ignore(it) => {
                let token_start = PragmaMap::pragma_target_start(source, comment, self.hops)
                    .unwrap_or(comment.attached_to);
                self.ignores.insert(token_start, it);
            }
            PragmaResult::File => self.ignore_file = true,
            PragmaResult::Start => {
                self.active_ignore_start.get_or_insert(comment.span.end);
            }
            PragmaResult::Stop => {
                if let Some(start) = self.active_ignore_start.take()
                    && start <= comment.span.start
                {
                    self.ignored_ranges.push((start, comment.span.start));
                }
            }
            PragmaResult::Unknown(comment_text) => {
                let (line, column) = source_text::line_column(source, comment.span.start);
                self.unhandled.push(UnhandledPragma { comment: comment_text, line, column });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{IgnoreType, PragmaMap, PragmaResult};
    use oxc_ast::ast::Comment;
    use oxc_span::Span;

    fn offset_of(source: &str, needle: &str) -> u32 {
        u32::try_from(source.find(needle).expect("needle occurs in source")).unwrap()
    }

    fn pragma_comment(source: &str) -> Comment {
        let start = offset_of(source, "/* istanbul");
        let end = offset_of(source, "*/") + 2;
        Comment { span: Span::new(start, end), ..Default::default() }
    }

    #[test]
    fn parses_plain_block_pragma() {
        assert!(matches!(
            PragmaMap::parse_pragma(" istanbul ignore next ", false),
            Some(PragmaResult::Ignore(IgnoreType::Next))
        ));
    }

    #[test]
    fn parses_legal_block_pragma() {
        assert!(matches!(
            PragmaMap::parse_pragma("! istanbul ignore next ", false),
            Some(PragmaResult::Ignore(IgnoreType::Next))
        ));
        assert!(matches!(
            PragmaMap::parse_pragma("!istanbul ignore next", false),
            Some(PragmaResult::Ignore(IgnoreType::Next))
        ));
    }

    #[test]
    fn parses_legal_line_pragma() {
        assert!(matches!(
            PragmaMap::parse_pragma("! v8 ignore next", false),
            Some(PragmaResult::Ignore(IgnoreType::Next))
        ));
        assert!(matches!(
            PragmaMap::parse_pragma("! istanbul ignore else", false),
            Some(PragmaResult::Ignore(IgnoreType::Else))
        ));
    }

    #[test]
    fn rejects_bang_only() {
        assert!(PragmaMap::parse_pragma("!", false).is_none());
        assert!(PragmaMap::parse_pragma("!!!", false).is_none());
    }

    #[test]
    fn rejects_non_pragma_legal_comment() {
        assert!(PragmaMap::parse_pragma("! Copyright (c) 2026", false).is_none());
    }

    #[test]
    fn next_token_start_skips_line_comments_for_every_line_terminator() {
        for (name, terminator) in
            [("LF", "\n"), ("CRLF", "\r\n"), ("CR", "\r"), ("LS", "\u{2028}"), ("PS", "\u{2029}")]
        {
            let source = format!("// bridge{terminator}next();");
            let expected = offset_of(&source, "next");
            assert_eq!(PragmaMap::next_token_start(&source, 0), Some(expected), "{name}");
        }
    }

    #[test]
    fn word_at_extracts_keyword() {
        assert_eq!(PragmaMap::word_at("else if (x)", 0), Some("else"));
        assert_eq!(PragmaMap::word_at("if (x)", 0), Some("if"));
        assert_eq!(PragmaMap::word_at("123abc", 0), None);
        assert_eq!(PragmaMap::word_at("", 0), None);
    }

    #[test]
    fn pragma_target_start_hops_past_else_to_chained_if() {
        let source = "if (a) {} /* istanbul ignore else */ else if (b) {}";
        let target = PragmaMap::pragma_target_start(source, &pragma_comment(source), true).unwrap();
        assert_eq!(target, offset_of(source, "if (b)"), "pragma must anchor on the chained `if`");
    }

    #[test]
    fn pragma_target_start_hops_past_else_through_inline_comment() {
        let source = "if (a) {} /* istanbul ignore else */ else /*c*/ if (b) {}";
        let target = PragmaMap::pragma_target_start(source, &pragma_comment(source), true).unwrap();
        assert_eq!(
            target,
            offset_of(source, "if (b)"),
            "pragma must hop past `else` and the inline comment to `if`"
        );
    }

    #[test]
    fn pragma_target_start_keeps_else_when_followed_by_block() {
        let source = "if (a) {} /* istanbul ignore else */ else { x }";
        let target = PragmaMap::pragma_target_start(source, &pragma_comment(source), true).unwrap();
        assert_eq!(
            target,
            offset_of(source, "else {"),
            "pragma stays on `else` when no chained `if` follows"
        );
    }
}
