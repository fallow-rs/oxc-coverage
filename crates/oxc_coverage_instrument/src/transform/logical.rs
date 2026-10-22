//! Logical-expression chain flattening, and per-leaf branch counter wrapping.

use std::mem;

use oxc_allocator::Vec as ArenaVec;
use oxc_ast::ast::*;
use oxc_span::{GetSpan, SPAN, Span};
use oxc_syntax::operator::LogicalOperator;
use oxc_traverse::TraverseCtx;

use crate::pragma::{IgnoreType, PragmaMap};

use super::counters::{CounterKind, dummy_expr, index_literal, prepend_counter};
use super::coverage_map::{PendingArm, PendingBranch, is_synthetic_span};
use super::{CoverageState, CoverageTransform};

impl<'arena> CoverageTransform<'_, 'arena> {
    /// Register the `binary-expr` branch for a logical chain and wrap each
    /// surviving leaf operand in its branch counter.
    pub(super) fn instrument_logical_expression(
        &mut self,
        expr: &mut LogicalExpression<'arena>,
        ctx: &mut TraverseCtx<'arena, CoverageState>,
    ) {
        if logical_expression_ignored(expr, ctx, self.in_ignored_subtree()) {
            return;
        }
        match expr.operator {
            LogicalOperator::And | LogicalOperator::Or | LogicalOperator::Coalesce => {
                // Istanbul flattens a chain of logical expressions into a single
                // branch with one location per leaf operand, so only the
                // outermost expression creates the branch entry.
                if is_parent_logical(ctx) {
                    return;
                }

                // A `/* istanbul ignore next */` on one operand only drops
                // that arm from the branch map; the surviving operand still
                // contributes a single-arm branch entry. A real
                // `LogicalExpression` always has at least one surviving
                // leaf because dropping both arms would require pragmas on
                // every leaf in the chain (handled by the empty-list bail
                // below).
                let leaf_spans = collect_logical_leaf_spans(expr, &ctx.state.pragmas);
                if leaf_spans.is_empty() {
                    return;
                }

                // `gate_arms: false`: leaf wrapping advances `path_idx` per
                // leaf and requires `b[id].len()` to equal the number of
                // wrapped leaves, so no leaf is ever individually dropped.
                // Either the branch is kept and every leaf is wrapped and
                // registered, or the umbrella is skipped and none are.
                let arms = leaf_spans.into_iter().map(PendingArm::new).collect();
                let Some(reg) = self.register_branch(PendingBranch {
                    branch_type: "binary-expr",
                    umbrella_span: expr.span,
                    gate_arms: false,
                    arms,
                }) else {
                    return;
                };

                if self.report_logic {
                    self.logical_branch_ids.push(reg.branch_id);
                }

                let mut state = LogicalWrapState::new(
                    self.cov_fn_name,
                    self.cov_fn_bt_name,
                    reg.branch_id,
                    self.report_logic,
                );
                wrap_logical_leaves(expr, &mut state, ctx);
            }
        }
    }
}

/// Whether the nearest non-parenthesized ancestor is a logical expression.
/// Oxc keeps `ParenthesizedExpression` nodes that Babel strips, so matching
/// istanbul-lib-instrument's chain flattening means looking through any
/// wrapping parens before deciding an operand is an inner one.
pub(super) fn is_parent_logical(ctx: &TraverseCtx<'_, CoverageState>) -> bool {
    use oxc_traverse::Ancestor;
    for a in ctx.ancestors() {
        match a {
            Ancestor::ParenthesizedExpressionExpression(_) => {}
            Ancestor::LogicalExpressionLeft(_) | Ancestor::LogicalExpressionRight(_) => {
                return true;
            }
            _ => return false,
        }
    }
    false
}

pub(super) fn logical_expression_ignored(
    expr: &LogicalExpression,
    ctx: &TraverseCtx<'_, CoverageState>,
    parent_ignored: bool,
) -> bool {
    parent_ignored
        || ctx.state.pragmas.get_for_node(expr.span.start) == Some(IgnoreType::Next)
        || is_synthetic_span(expr.span)
}

/// Collect all leaf operand spans from a chained logical expression.
/// For `a && b || c`, returns spans of [a, b, c]. Also flattens through
/// `ParenthesizedExpression` nodes so `a && (b || c)` is treated as one
/// three-leaf chain, matching istanbul-lib-instrument.
pub(super) fn collect_logical_leaf_spans(
    expr: &LogicalExpression,
    pragmas: &PragmaMap,
) -> Vec<Span> {
    let mut spans = Vec::new();
    collect_logical_leaves_inner(&expr.left, pragmas, &mut spans);
    collect_logical_leaves_inner(&expr.right, pragmas, &mut spans);
    spans
}

fn collect_logical_leaves_inner(expr: &Expression, pragmas: &PragmaMap, spans: &mut Vec<Span>) {
    if let Expression::ParenthesizedExpression(paren) = expr {
        collect_logical_leaves_inner(&paren.expression, pragmas, spans);
        return;
    }
    if pragmas.get(expr.span().start) == Some(IgnoreType::Next) {
        return;
    }
    if let Expression::LogicalExpression(logical) = expr {
        collect_logical_leaves_inner(&logical.left, pragmas, spans);
        collect_logical_leaves_inner(&logical.right, pragmas, spans);
    } else {
        spans.push(expr.span());
    }
}

/// Walk state for wrapping the leaves of one logical-expression chain: the
/// slot identity every leaf shares, plus the arm index the walk is up to.
pub(super) struct LogicalWrapState<'b> {
    cov_fn_name: &'b str,
    cov_fn_bt_name: Option<&'b str>,
    branch_id: usize,
    report_logic: bool,
    path_idx: usize,
}

impl<'b> LogicalWrapState<'b> {
    pub(super) const fn new(
        cov_fn_name: &'b str,
        cov_fn_bt_name: Option<&'b str>,
        branch_id: usize,
        report_logic: bool,
    ) -> Self {
        Self { cov_fn_name, cov_fn_bt_name, branch_id, report_logic, path_idx: 0 }
    }
}

/// `inner` -> `cov_fn_bt(inner, branch_id, path_idx)`, the truthy-tracking
/// helper call emitted when `report_logic` is enabled.
fn build_bt_call<'a>(
    inner: Expression<'a>,
    state: &LogicalWrapState<'a>,
    ctx: &TraverseCtx<'a, CoverageState>,
) -> Expression<'a> {
    let bt_name = state.cov_fn_bt_name.expect("report_logic requires cov_fn_bt_name");
    let callee = Expression::new_identifier(SPAN, bt_name, ctx);
    let mut args = ArenaVec::new_in(ctx);
    args.push(Argument::from(inner));
    args.push(Argument::from(index_literal(ctx, state.branch_id)));
    args.push(Argument::from(index_literal(ctx, state.path_idx)));
    Expression::new_call_expression(SPAN, callee, None, args, false, ctx)
}

fn wrap_logical_leaf<'a>(
    operand: &mut Expression<'a>,
    state: &mut LogicalWrapState<'a>,
    ctx: &TraverseCtx<'a, CoverageState>,
) {
    prepend_counter(
        operand,
        CounterKind::branch(state.cov_fn_name, state.branch_id, state.path_idx),
        ctx,
    );
    if state.report_logic {
        let branch_wrapped = mem::replace(operand, dummy_expr(ctx));
        *operand = build_bt_call(branch_wrapped, state, ctx);
    }
    state.path_idx += 1;
}

/// Recursively wrap each leaf operand in a chained logical expression with
/// its branch counter: `(cov.b[id][pathIdx]++, operand)`. Looks through
/// `ParenthesizedExpression` so `a && (b || c)` wraps all three leaves.
pub(super) fn wrap_logical_leaves<'a>(
    expr: &mut LogicalExpression<'a>,
    state: &mut LogicalWrapState<'a>,
    ctx: &mut TraverseCtx<'a, CoverageState>,
) {
    wrap_logical_operand(&mut expr.left, state, ctx);
    wrap_logical_operand(&mut expr.right, state, ctx);
}

fn wrap_logical_operand<'a>(
    operand: &mut Expression<'a>,
    state: &mut LogicalWrapState<'a>,
    ctx: &mut TraverseCtx<'a, CoverageState>,
) {
    // Parens are transparent here, which is the AST shape Babel produces.
    if let Expression::ParenthesizedExpression(paren) = operand {
        return wrap_logical_operand(&mut paren.expression, state, ctx);
    }
    if ctx.state.pragmas.get(operand.span().start) == Some(IgnoreType::Next) {
        return;
    }
    if let Expression::LogicalExpression(inner) = operand {
        wrap_logical_leaves(inner, state, ctx);
    } else {
        wrap_logical_leaf(operand, state, ctx);
    }
}
