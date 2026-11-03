//! Counter slot identity, and the AST builders that emit counter increments.

use std::mem;

use oxc_allocator::Vec as ArenaVec;
use oxc_ast::ast::*;
use oxc_span::SPAN;
use oxc_syntax::operator::UpdateOperator;
use oxc_traverse::TraverseCtx;

use super::CoverageState;

/// Which counter map a [`PendingInsertion`] belongs to.
#[derive(Clone, Copy)]
pub(super) enum CounterType {
    /// `cov_fn().s[counter_id]`.
    Statement,
    /// Left arm of a logical assignment, `cov_fn().b[counter_id][0]`.
    BranchLeft,
}

/// A counter that has to be emitted as a sibling statement rather than wrapped
/// around its own expression slot.
pub(super) struct PendingInsertion {
    /// `span.start` of the statement the counter is emitted in front of.
    pub(super) target_start: u32,
    /// Index into the map named by `counter_type`.
    pub(super) counter_id: usize,
    pub(super) counter_type: CounterType,
}

/// A counter slot in the coverage map. Bundles `cov_fn_name` with the per-slot
/// indices so the AST builders take one value instead of threading three or
/// four primitives through every call site.
#[derive(Clone, Copy)]
pub(super) enum CounterKind<'a> {
    /// `cov_fn().s[id]` or `cov_fn().f[id]` slot, `kind` being `"s"` or `"f"`.
    Slot { cov_fn_name: &'a str, kind: &'static str, id: usize },
    /// `cov_fn().b[branch_id][path_idx]` slot.
    Branch { cov_fn_name: &'a str, branch_id: usize, path_idx: usize },
}

impl<'a> CounterKind<'a> {
    pub(super) const fn stmt(cov_fn_name: &'a str, id: usize) -> Self {
        Self::Slot { cov_fn_name, kind: "s", id }
    }
    pub(super) const fn func(cov_fn_name: &'a str, id: usize) -> Self {
        Self::Slot { cov_fn_name, kind: "f", id }
    }
    pub(super) const fn branch(cov_fn_name: &'a str, branch_id: usize, path_idx: usize) -> Self {
        Self::Branch { cov_fn_name, branch_id, path_idx }
    }

    pub(super) fn from_pending(cov_fn_name: &'a str, pending: &PendingInsertion) -> Self {
        match pending.counter_type {
            CounterType::Statement => Self::stmt(cov_fn_name, pending.counter_id),
            CounterType::BranchLeft => Self::branch(cov_fn_name, pending.counter_id, 0),
        }
    }
}

/// Build a `base.field` static member access.
fn static_field<'a>(
    base: Expression<'a>,
    field: &'a str,
    ctx: &TraverseCtx<'a, CoverageState>,
) -> MemberExpression<'a> {
    MemberExpression::new_static_member_expression(
        SPAN,
        base,
        IdentifierName::new(SPAN, field, ctx),
        false,
        ctx,
    )
}

/// Build a `base[idx]` computed (numeric-index) member access.
fn computed_index<'a>(
    base: MemberExpression<'a>,
    idx: usize,
    ctx: &TraverseCtx<'a, CoverageState>,
) -> MemberExpression<'a> {
    MemberExpression::new_computed_member_expression(
        SPAN,
        Expression::from(base),
        index_literal(ctx, idx),
        false,
        ctx,
    )
}

/// Build the `cov_fn()` call that returns the coverage object. A call, not a
/// plain identifier, keeps a counter safe in a hoisted function that runs
/// before the preamble initializes the coverage object.
fn coverage_call<'a>(cov_fn_name: &'a str, ctx: &TraverseCtx<'a, CoverageState>) -> Expression<'a> {
    let callee = Expression::new_identifier(SPAN, cov_fn_name, ctx);
    Expression::new_call_expression(SPAN, callee, None, ArenaVec::new_in(ctx), false, ctx)
}

/// Build a counter increment expression: `cov_fn().s[id]++`, `cov_fn().f[id]++`,
/// or `cov_fn().b[branch_id][path_idx]++` depending on the kind.
fn build_counter_expr<'a>(
    kind: CounterKind<'a>,
    ctx: &TraverseCtx<'a, CoverageState>,
) -> Expression<'a> {
    let target = match kind {
        CounterKind::Slot { cov_fn_name, kind, id } => {
            let coverage = coverage_call(cov_fn_name, ctx);
            let field = static_field(coverage, kind, ctx);
            computed_index(field, id, ctx)
        }
        CounterKind::Branch { cov_fn_name, branch_id, path_idx } => {
            let coverage = coverage_call(cov_fn_name, ctx);
            let b = static_field(coverage, "b", ctx);
            let outer = computed_index(b, branch_id, ctx);
            computed_index(outer, path_idx, ctx)
        }
    };
    Expression::new_update_expression(
        SPAN,
        UpdateOperator::Increment,
        true,
        SimpleAssignmentTarget::from(target),
        ctx,
    )
}

/// Build a counter increment statement wrapping the matching expression.
pub(super) fn build_counter_stmt<'a>(
    kind: CounterKind<'a>,
    ctx: &TraverseCtx<'a, CoverageState>,
) -> Statement<'a> {
    let expr = build_counter_expr(kind, ctx);
    Statement::new_expression_statement(SPAN, expr, ctx)
}

/// Replace `target` with the sequence expression `(counter, target)`, where
/// `counter` increments the `kind` slot. This is the shape istanbul uses to
/// attach a counter to an expression slot: statement init, ternary arm,
/// logical-assignment right-hand side, branch leaf.
pub(super) fn prepend_counter<'a>(
    target: &mut Expression<'a>,
    kind: CounterKind<'a>,
    ctx: &TraverseCtx<'a, CoverageState>,
) {
    let counter = build_counter_expr(kind, ctx);
    let orig = mem::replace(target, dummy_expr(ctx));
    let mut items = ArenaVec::new_in(ctx);
    items.push(counter);
    items.push(orig);
    *target = Expression::new_sequence_expression(SPAN, items, ctx);
}

/// Numeric literal for a coverage-map index. Ids are bounded by the number of
/// AST nodes in one file, so the saturating narrowing is unreachable.
pub(super) fn index_literal<'a>(
    ctx: &TraverseCtx<'a, CoverageState>,
    index: usize,
) -> Expression<'a> {
    let index = f64::from(u32::try_from(index).unwrap_or(u32::MAX));
    Expression::new_numeric_literal(SPAN, index, None, oxc_syntax::number::NumberBase::Decimal, ctx)
}

/// Placeholder expression for `mem::replace`, never reachable in the output.
pub(super) fn dummy_expr<'a>(ctx: &TraverseCtx<'a, CoverageState>) -> Expression<'a> {
    Expression::new_numeric_literal(SPAN, 0.0, None, oxc_syntax::number::NumberBase::Decimal, ctx)
}

/// Inject a branch counter into a statement, wrapping in a block if necessary.
pub(super) fn inject_branch_counter_into_statement<'a>(
    stmt: &mut Statement<'a>,
    kind: CounterKind<'a>,
    ctx: &mut TraverseCtx<'a, CoverageState>,
) {
    let counter_stmt = build_counter_stmt(kind, ctx);

    if let Statement::BlockStatement(block) = stmt {
        block.body.insert(0, counter_stmt);
        return;
    }

    // The synthesized block needs its own scope, otherwise traverse panics on
    // the unregistered scope id.
    let scope_id = ctx.create_child_scope_of_current(oxc_syntax::scope::ScopeFlags::empty());
    let original = mem::replace(stmt, Statement::new_empty_statement(SPAN, ctx));
    let mut stmts = ArenaVec::new_in(ctx);
    stmts.push(counter_stmt);
    stmts.push(original);
    *stmt = Statement::new_block_statement_with_scope_id(SPAN, stmts, scope_id, ctx);
}
