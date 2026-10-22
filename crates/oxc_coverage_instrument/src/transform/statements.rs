//! Statement-counter placement: which statement kinds carry a counter, and
//! where a counter is emitted when it cannot wrap its own expression slot.

use std::collections::BTreeMap;
use std::mem;

use oxc_allocator::Vec as ArenaVec;
use oxc_ast::ast::*;
use oxc_span::{GetSpan, SPAN};
use oxc_traverse::TraverseCtx;

use crate::pragma::IgnoreType;

use super::counters::{
    CounterKind, CounterType, PendingInsertion, build_counter_stmt, prepend_counter,
};
use super::coverage_map::is_synthetic_span;
use super::names::declarator_function_name;
use super::{CoverageState, CoverageTransform};

impl<'arena> CoverageTransform<'_, 'arena> {
    /// Register the statement counter for `stmt`, or record that its subtree is
    /// suppressed by a pragma.
    pub(super) fn register_statement_counter(
        &mut self,
        stmt: &Statement<'arena>,
        ctx: &TraverseCtx<'arena, CoverageState>,
    ) {
        let span = stmt.span();
        let parent_ignored = self.in_ignored_subtree();
        let is_injected = is_synthetic_span(span);
        // The statement that wraps a concise arrow body starts inside the
        // body's parentheses, so a pragma there belongs to the expression,
        // not to the statement. Look it up where the source text starts.
        let pragma_start = match stmt {
            Statement::ExpressionStatement(expr) => span.start.min(expr.expression.span().start),
            _ => span.start,
        };
        let has_ignore_next = !is_injected
            && (ctx.state.pragmas.get(pragma_start) == Some(IgnoreType::Next)
                || self.is_in_ignored_if_arm(span));
        self.ignored_stmt_stack.push(has_ignore_next);
        if is_injected || is_container_statement(stmt) || parent_ignored {
            return;
        }
        // `skip_next` lets nested functions and arrows in the subtree skip
        // their own counters; `finish_statement` clears it so it cannot reach
        // the next sibling statement.
        if has_ignore_next {
            self.skip_next = true;
            return;
        }
        if self.skip_next {
            self.skip_next = false;
            return;
        }
        if let Some(stmt_id) = self.add_statement(span) {
            self.pending_insertions.push(PendingInsertion {
                target_start: span.start,
                counter_id: stmt_id,
                counter_type: CounterType::Statement,
            });
        }
    }

    pub(super) fn finish_statement(&mut self) {
        // Nested enter hooks consume `skip_next` when they fire. When none
        // fires (`/* istanbul ignore next */ return 1;`) it would otherwise
        // reach the next sibling statement.
        self.skip_next = false;
        self.ignored_stmt_stack.pop();
    }

    /// Rebuild `stmts` with every pending counter whose target statement is in
    /// this list emitted immediately before it. Counters whose target is not
    /// here stay pending for an enclosing list.
    pub(super) fn insert_pending_statement_counters(
        &mut self,
        stmts: &mut ArenaVec<'arena, Statement<'arena>>,
        ctx: &TraverseCtx<'arena, CoverageState>,
    ) {
        if self.pending_insertions.is_empty() {
            return;
        }

        let cov_fn = self.cov_fn_name;
        let mut insertions_by_target = BTreeMap::new();
        for stmt in stmts.iter() {
            let span = stmt.span();
            if is_synthetic_span(span) {
                continue;
            }
            insertions_by_target.entry(span.start).or_insert_with(Vec::new);
        }

        let mut unmatched = Vec::with_capacity(self.pending_insertions.len());
        let mut matched_count = 0;
        for pending in mem::take(&mut self.pending_insertions) {
            if let Some(target) = insertions_by_target.get_mut(&pending.target_start) {
                target.push(pending);
                matched_count += 1;
            } else {
                unmatched.push(pending);
            }
        }
        self.pending_insertions = unmatched;

        if matched_count == 0 {
            return;
        }

        let original = mem::replace(stmts, ArenaVec::new_in(ctx));
        let mut rebuilt = ArenaVec::with_capacity_in(original.len() + matched_count, ctx);
        for stmt in original {
            let span = stmt.span();
            if let Some(insertions) = insertions_by_target.remove(&span.start) {
                for pending in insertions {
                    rebuilt
                        .push(build_counter_stmt(CounterKind::from_pending(cov_fn, &pending), ctx));
                }
            }
            rebuilt.push(stmt);
        }
        *stmts = rebuilt;
    }

    /// Attach the per-declarator statement counter to a `VariableDeclarator`
    /// and carry the binding name into a function-valued initializer.
    pub(super) fn instrument_variable_declarator(
        &mut self,
        decl: &mut VariableDeclarator<'arena>,
        ctx: &TraverseCtx<'arena, CoverageState>,
    ) {
        // If the enclosing declaration is ignored, skip both the statement
        // counter wrap and any inner function counter. Set `skip_next` so the
        // inner arrow/function hook consumes it.
        if self.skip_current_var_decl {
            if matches!(
                decl.init.as_ref().map(Expression::without_parentheses),
                Some(Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_))
            ) {
                self.skip_next = true;
            }
            return;
        }

        if let Some(name) = declarator_function_name(decl) {
            self.pending_name = Some(name);
        }

        // istanbul-lib-instrument's `coverVariableDeclarator` calls
        // `insertStatementCounter` on `path.get('init')`, so the counter wraps
        // the init as `(++cov.s[N], init)` and a declarator without an init
        // (`let x;`) gets no statement counter.
        let Some(init) = decl.init.as_mut() else { return };
        if self.in_ignored_subtree() {
            return;
        }
        // Babel drops parentheses from the AST, so istanbul records `x = (a, b)`
        // as `a, b`. Oxc keeps them as a node; every location strips them.
        let init_span = init.without_parentheses().span();
        if is_synthetic_span(init_span) {
            return;
        }

        // The sequence-expression wrap `(++s, fn)` defeats NamedEvaluation, so
        // `const foo = function () {}` would produce an unnamed function. For
        // those initializers the counter is hoisted to a sibling statement
        // before the enclosing `VariableDeclaration` and the right-hand side
        // stays the direct init. A `for (var x = fn;;)` or for-in/for-of head
        // has no sibling slot, so it falls back to the wrap.
        let is_named_initializer = matches!(
            init.without_parentheses(),
            Expression::FunctionExpression(_)
                | Expression::ArrowFunctionExpression(_)
                | Expression::ClassExpression(_)
        );
        if is_named_initializer
            && let Some(hoist_target_start) = enclosing_var_decl_hoist_target(ctx)
        {
            if let Some(stmt_id) = self.add_statement(init_span) {
                self.pending_insertions.push(PendingInsertion {
                    target_start: hoist_target_start,
                    counter_id: stmt_id,
                    counter_type: CounterType::Statement,
                });
            }
            return;
        }

        if let Some(stmt_id) = self.add_statement(init_span) {
            prepend_counter(init, CounterKind::stmt(self.cov_fn_name, stmt_id), ctx);
        }
    }

    pub(super) fn retarget_pending_insertions(&mut self, from_start: u32, to_start: u32) {
        for pending in &mut self.pending_insertions {
            if pending.target_start == from_start {
                pending.target_start = to_start;
            }
        }
    }

    pub(super) fn inject_pending_counters_into_statement_child(
        &mut self,
        body: &mut Statement<'arena>,
        ctx: &mut TraverseCtx<'arena, CoverageState>,
    ) {
        if matches!(body, Statement::BlockStatement(_)) {
            return;
        }

        let span = body.span();
        if is_synthetic_span(span) {
            return;
        }

        let pending: Vec<_> =
            self.pending_insertions.extract_if(.., |p| p.target_start == span.start).collect();
        if pending.is_empty() {
            return;
        }

        let cov_fn = self.cov_fn_name;
        let scope_id = ctx.create_child_scope_of_current(oxc_syntax::scope::ScopeFlags::empty());
        let original = mem::replace(body, Statement::new_empty_statement(SPAN, ctx));
        let mut stmts = ArenaVec::new_in(ctx);
        for insertion in pending {
            stmts.push(build_counter_stmt(CounterKind::from_pending(cov_fn, &insertion), ctx));
        }
        stmts.push(original);
        *body = Statement::new_block_statement_with_scope_id(SPAN, stmts, scope_id, ctx);
    }
}

/// Whether istanbul-lib-instrument's `visitor.js` treats the statement as a
/// container rather than a counted statement:
///
/// - `FunctionDeclaration` / `ClassDeclaration`: covered by function counters
/// - `VariableDeclaration`: covered per declarator, see
///   `enter_variable_declarator`
/// - imports, exports and type-only declarations: skipped entirely
/// - `BlockStatement` / `EmptyStatement`: never counted
pub(super) fn is_container_statement(stmt: &Statement<'_>) -> bool {
    matches!(
        stmt,
        Statement::BlockStatement(_)
            | Statement::EmptyStatement(_)
            | Statement::FunctionDeclaration(_)
            | Statement::ClassDeclaration(_)
            | Statement::VariableDeclaration(_)
            | Statement::ImportDeclaration(_)
            // Oxc 0.143 split the old combined `ExportNamedDeclaration` into
            // `ExportDeclaration` (`export const x = 1`),
            // `ExportNamedDeclaration` (`export { x }`) and
            // `ExportFromDeclaration` (`export { x } from 'm'`). All three stay
            // containers, as the single node was.
            | Statement::ExportDeclaration(_)
            | Statement::ExportNamedDeclaration(_)
            | Statement::ExportFromDeclaration(_)
            | Statement::ExportDefaultDeclaration(_)
            | Statement::ExportAllDeclaration(_)
            | Statement::TSTypeAliasDeclaration(_)
            | Statement::TSInterfaceDeclaration(_)
            | Statement::TSEnumDeclaration(_)
            // Oxc 0.146 split the old combined `TSModuleDeclaration` into
            // `TSExternalModuleDeclaration` (`declare module 'foo' {}`) and
            // `TSNamespaceDeclaration` (`namespace Foo {}` / `module Foo {}`).
            // Both stay containers, as the single node was.
            | Statement::TSExternalModuleDeclaration(_)
            | Statement::TSNamespaceDeclaration(_)
            | Statement::TSImportEqualsDeclaration(_)
            | Statement::TSExportAssignment(_)
            | Statement::TSNamespaceExportDeclaration(_)
    )
}

/// Return the start offset of the enclosing `VariableDeclaration` if it
/// occupies a statement slot from which the per-declarator statement
/// counter can be hoisted out as a preceding sibling. Returns `None` when
/// the declaration is in a position with no sibling slot (`for (var x = ..;)`,
/// `for (var x of ..)`, `for (var x in ..)`), in which case callers must
/// keep the sequence-expression wrap.
pub(super) fn enclosing_var_decl_hoist_target(ctx: &TraverseCtx<'_, CoverageState>) -> Option<u32> {
    use oxc_traverse::Ancestor;
    let mut iter = ctx.ancestors();
    let var_decl_span = match iter.next()? {
        Ancestor::VariableDeclarationDeclarations(a) => *a.span(),
        _ => return None,
    };
    match iter.next()? {
        Ancestor::ForStatementInit(_)
        | Ancestor::ForInStatementLeft(_)
        | Ancestor::ForOfStatementLeft(_) => None,
        // `export const fn = () => {}` wraps the `VariableDeclaration` in an
        // `ExportDeclaration`, and the export node is what occupies the
        // statement slot. The counter has to target the export statement:
        // targeting the inner declaration's start never matches in
        // `exit_statements`, which would leave the counter at zero even though
        // the initializer runs at module evaluation.
        Ancestor::ExportDeclarationDeclaration(a) => Some(a.span().start),
        _ => Some(var_decl_span.start),
    }
}
