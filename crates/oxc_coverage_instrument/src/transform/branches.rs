//! Branch instrumentation for `if` arms, ternary arms, logical assignment
//! and optional-chain links.

use std::mem;

use oxc_allocator::Vec as ArenaVec;
use oxc_ast::ast::*;
use oxc_span::{GetSpan, SPAN, Span};
use oxc_traverse::TraverseCtx;

use crate::pragma::IgnoreType;

use super::counters::{
    CounterKind, CounterType, PendingInsertion, build_counter_stmt, dummy_expr, index_literal,
    inject_branch_counter_into_statement, prepend_counter,
};
use super::coverage_map::{PendingArm, PendingBranch, is_synthetic_span};
use super::ignore::{enclosing_destructure_property_pragma, is_ignored_case};
use super::{CoverageState, CoverageTransform};

impl<'arena> CoverageTransform<'_, 'arena> {
    /// Register the `if` branch and inject the counter for each arm the
    /// pragmas leave in place, synthesizing a missing `else` where needed.
    pub(super) fn instrument_if_branches(
        &mut self,
        stmt: &mut IfStatement<'arena>,
        ctx: &mut TraverseCtx<'arena, CoverageState>,
    ) {
        if self.in_ignored_subtree() {
            self.ignored_if_arm_push_counts.push(0);
            return;
        }
        let pragma = ctx.state.pragmas.get(stmt.span.start);
        self.record_ignored_if_arm(stmt, pragma);

        // istanbul-lib-instrument's `coverIfBranches` passes `n.loc`, the whole
        // `IfStatement` span, as the consequent location rather than the
        // narrower consequent block:
        // `insertBranchCounter(path.get('consequent'), branch, n.loc)` in
        // `istanbul-lib-instrument/src/visitor.js`. Reporters highlight that
        // range, so it is reproduced here. The consequent body span goes into
        // the side-table instead, because `v8_to_istanbul` has to resolve
        // arm[0] against V8's `BlockStatement` range and V8 emits no range
        // matching the whole-IfStatement convention.
        let consequent_body_span = stmt.consequent.span();
        let synthetic_anchor = consequent_body_span.end;

        // Every arm span is a pure pre-mutation read, so the whole arm vector
        // is known before the umbrella id is chosen. The else arm resolves to
        // its own span, or a zero-width anchor at the consequent's end when the
        // `if` has no (or an already-synthetic) `else`; its V8 body span begins
        // at the consequent's end and includes the `else` transition.
        let mut arms = Vec::new();
        let mut consequent_arm = None;
        if pragma != Some(IgnoreType::If) {
            consequent_arm = Some(arms.len());
            arms.push(PendingArm::with_body(stmt.span, consequent_body_span));
        }
        let mut else_arm = None;
        if pragma != Some(IgnoreType::Else) {
            let real_alternate =
                stmt.alternate.as_ref().map(GetSpan::span).filter(|span| !is_synthetic_span(*span));
            let arm_span =
                real_alternate.unwrap_or_else(|| Span::new(synthetic_anchor, synthetic_anchor));
            let v8_body_span = Span::new(synthetic_anchor, arm_span.end);
            else_arm = Some(arms.len());
            if self.istanbul_compat && real_alternate.is_none() {
                arms.push(PendingArm::empty_with_body(v8_body_span));
            } else {
                arms.push(PendingArm::with_body(arm_span, v8_body_span));
            }
        }

        // Skipping the umbrella skips every counter under this branch, but the
        // traversal still recurses; the pragma-arm bookkeeping above and
        // `pop_ignored_if_arms`'s pop stay balanced either way.
        let Some(reg) = self.register_branch(PendingBranch {
            branch_type: "if",
            umbrella_span: stmt.span,
            gate_arms: true,
            arms,
        }) else {
            return;
        };
        let cov_fn = self.cov_fn_name;

        // The synthesized `else {}` must land after the consequent counter.
        if let Some(arm) = consequent_arm
            && let Some(path_idx) = reg.slot(arm)
        {
            inject_branch_counter_into_statement(
                &mut stmt.consequent,
                CounterKind::branch(cov_fn, reg.branch_id, path_idx),
                ctx,
            );
        }
        if let Some(arm) = else_arm
            && let Some(path_idx) = reg.slot(arm)
        {
            self.synthesize_else_arm_and_inject(stmt, reg.branch_id, path_idx, ctx);
        }
    }

    /// Drop the ignored-arm spans pushed by the `if` being left.
    pub(super) fn pop_ignored_if_arms(&mut self) {
        if let Some(count) = self.ignored_if_arm_push_counts.pop() {
            for _ in 0..count {
                self.ignored_if_arm_spans.pop();
            }
        }
    }

    /// Register the `cond-expr` branch for a ternary and inject a counter into
    /// each arm the pragmas leave in place.
    pub(super) fn instrument_conditional_branches(
        &mut self,
        expr: &mut ConditionalExpression<'arena>,
        ctx: &TraverseCtx<'arena, CoverageState>,
    ) {
        if self.in_ignored_subtree()
            || ctx.state.pragmas.get(expr.span.start) == Some(IgnoreType::Next)
            || is_synthetic_span(expr.span)
        {
            return;
        }
        let ignore_consequent =
            ctx.state.pragmas.get(expr.consequent.span().start) == Some(IgnoreType::Next);
        let ignore_alternate =
            ctx.state.pragmas.get(expr.alternate.span().start) == Some(IgnoreType::Next);
        if ignore_consequent && ignore_alternate {
            return;
        }

        // istanbul drops only the pragma'd arm's location from the branch map,
        // so the entry survives with the one remaining arm still counted.
        let mut arms = Vec::new();
        let mut consequent_arm = None;
        if !ignore_consequent {
            consequent_arm = Some(arms.len());
            arms.push(PendingArm::new(expr.consequent.without_parentheses().span()));
        }
        let mut alternate_arm = None;
        if !ignore_alternate {
            alternate_arm = Some(arms.len());
            arms.push(PendingArm::new(expr.alternate.without_parentheses().span()));
        }

        let Some(reg) = self.register_branch(PendingBranch {
            branch_type: "cond-expr",
            umbrella_span: expr.span,
            gate_arms: true,
            arms,
        }) else {
            return;
        };

        if let Some(arm) = consequent_arm
            && let Some(path_idx) = reg.slot(arm)
        {
            prepend_counter(
                &mut expr.consequent,
                CounterKind::branch(self.cov_fn_name, reg.branch_id, path_idx),
                ctx,
            );
        }
        if let Some(arm) = alternate_arm
            && let Some(path_idx) = reg.slot(arm)
        {
            prepend_counter(
                &mut expr.alternate,
                CounterKind::branch(self.cov_fn_name, reg.branch_id, path_idx),
                ctx,
            );
        }
    }

    /// Register the `switch` branch and prepend a counter to each case body
    /// the pragmas leave in place.
    pub(super) fn instrument_switch_cases(
        &mut self,
        stmt: &mut SwitchStatement<'arena>,
        ctx: &TraverseCtx<'arena, CoverageState>,
    ) {
        if self.in_ignored_subtree() {
            return;
        }
        // Pass A: collect non-ignored case spans in case order. The `&mut`
        // injection below re-walks the same cases, so the arm index the k-th
        // surviving case gets here is the slot it reads back through `reg`.
        let mut arms = Vec::new();
        for case in &stmt.cases {
            if !is_ignored_case(case, &ctx.state.pragmas) {
                arms.push(PendingArm::new(case.span));
            }
        }
        let Some(reg) = self.register_branch(PendingBranch {
            branch_type: "switch",
            umbrella_span: stmt.span,
            gate_arms: true,
            arms,
        }) else {
            return;
        };

        let cov_fn = self.cov_fn_name;
        let mut arm = 0;
        for case in &mut stmt.cases {
            if is_ignored_case(case, &ctx.state.pragmas) {
                continue;
            }
            if let Some(path_idx) = reg.slot(arm) {
                let branch_stmt =
                    build_counter_stmt(CounterKind::branch(cov_fn, reg.branch_id, path_idx), ctx);
                case.consequent.insert(0, branch_stmt);
            }
            arm += 1;
        }
    }

    /// Register the `default-arg` branch for a parameter default and carry the
    /// parameter name into a function-valued default.
    pub(super) fn instrument_parameter_default(
        &mut self,
        param: &mut FormalParameter<'arena>,
        ctx: &TraverseCtx<'arena, CoverageState>,
    ) {
        if self.in_ignored_subtree() {
            return;
        }
        if ctx.state.pragmas.get(param.span.start) == Some(IgnoreType::Next) {
            return;
        }
        // Istanbul gives `function f(x = 1) {}` a `default-arg` branch with one
        // location, the default expression.
        if let Some(init) = &mut param.initializer {
            // `function f(cb = () => 1)` -> `fnMap[N].name = "cb"`: the inner
            // arrow or function is the direct initializer of the parameter
            // binding, so it inherits the parameter's name.
            if matches!(
                **init,
                Expression::FunctionExpression(_)
                    | Expression::ArrowFunctionExpression(_)
                    | Expression::ClassExpression(_)
            ) && let Some(id) = param.pattern.get_binding_identifier()
            {
                self.pending_name = Some(id.name.to_string());
            }
            let init_span = init.without_parentheses().span();
            let Some(reg) = self.register_branch(PendingBranch {
                branch_type: "default-arg",
                umbrella_span: param.span,
                gate_arms: true,
                arms: vec![PendingArm::new(init_span)],
            }) else {
                return;
            };
            if let Some(path_idx) = reg.slot(0) {
                prepend_counter(
                    init,
                    CounterKind::branch(self.cov_fn_name, reg.branch_id, path_idx),
                    ctx,
                );
            }
        }
    }

    /// Register the `default-arg` branch for a destructuring default and carry
    /// the binding name into a function-valued default.
    pub(super) fn instrument_destructuring_default(
        &mut self,
        pattern: &mut AssignmentPattern<'arena>,
        ctx: &TraverseCtx<'arena, CoverageState>,
    ) {
        if self.in_ignored_subtree() {
            return;
        }
        // `/* istanbul ignore next */` can sit at the pattern itself (shorthand
        // object property, array element) or one level up at the enclosing
        // `BindingProperty`. Either binding suppresses the `default-arg`
        // branch on this default value.
        if ctx.state.pragmas.get(pattern.span.start) == Some(IgnoreType::Next)
            || enclosing_destructure_property_pragma(ctx)
        {
            return;
        }
        // Carry the binding name into any inner function or arrow on the right
        // of the default, so `function f(cb = () => 1)` and the destructuring
        // equivalents surface as `fnMap[N].name = "cb"`.
        if matches!(
            pattern.right,
            Expression::FunctionExpression(_)
                | Expression::ArrowFunctionExpression(_)
                | Expression::ClassExpression(_)
        ) && let Some(id) = pattern.left.get_binding_identifier()
        {
            self.pending_name = Some(id.name.to_string());
        }
        // Istanbul types destructuring defaults (`const { x = 1 } = obj`) as
        // `default-arg` too.
        let right_span = pattern.right.without_parentheses().span();
        let Some(reg) = self.register_branch(PendingBranch {
            branch_type: "default-arg",
            umbrella_span: pattern.span,
            gate_arms: true,
            arms: vec![PendingArm::new(right_span)],
        }) else {
            return;
        };
        if let Some(path_idx) = reg.slot(0) {
            prepend_counter(
                &mut pattern.right,
                CounterKind::branch(self.cov_fn_name, reg.branch_id, path_idx),
                ctx,
            );
        }
    }

    pub(super) fn try_instrument_logical_assignment(
        &mut self,
        expr: &mut AssignmentExpression<'arena>,
        ctx: &TraverseCtx<'arena, CoverageState>,
    ) {
        if self.istanbul_compat || !is_logical_assignment_operator(expr.operator) {
            return;
        }
        let left_span = expr.left.span();
        let right_span = expr.right.without_parentheses().span();
        // `gate_arms: false`: the left counter rides the `BranchLeft` pending
        // insertion at slot 0 and the right is hard-wired to slot 1, so both
        // arms must be registered together or not at all.
        let Some(reg) = self.register_branch(PendingBranch {
            branch_type: "binary-expr",
            umbrella_span: expr.span,
            gate_arms: false,
            arms: vec![PendingArm::new(left_span), PendingArm::new(right_span)],
        }) else {
            return;
        };
        self.pending_insertions.push(PendingInsertion {
            target_start: expr.span.start,
            counter_id: reg.branch_id,
            counter_type: CounterType::BranchLeft,
        });
        prepend_counter(
            &mut expr.right,
            CounterKind::branch(self.cov_fn_name, reg.branch_id, 1),
            ctx,
        );
    }

    /// Record which arm spans of an `if` are pragma-ignored, so statements
    /// nested inside an ignored arm register no counters of their own.
    pub(super) fn record_ignored_if_arm(
        &mut self,
        stmt: &IfStatement<'arena>,
        pragma: Option<IgnoreType>,
    ) {
        let mut ignored_arm_count = 0_usize;
        if pragma == Some(IgnoreType::If) {
            self.ignored_if_arm_spans.push(stmt.consequent.span());
            ignored_arm_count += 1;
        } else if pragma == Some(IgnoreType::Else)
            && let Some(alt) = &stmt.alternate
        {
            self.ignored_if_arm_spans.push(alt.span());
            ignored_arm_count += 1;
        }
        self.ignored_if_arm_push_counts.push(ignored_arm_count);
    }

    /// Synthesize a missing else-arm block where needed and inject its branch
    /// counter, as istanbul-lib-instrument's `coverIfBranches` does. The arm
    /// span is resolved in `instrument_if_branches` before any mutation; the
    /// block is created here only once the arm is known to survive the gate, so
    /// a rejected else arm leaves no spurious `else {}` in the output.
    fn synthesize_else_arm_and_inject(
        &self,
        stmt: &mut IfStatement<'arena>,
        branch_id: usize,
        path_idx: usize,
        ctx: &mut TraverseCtx<'arena, CoverageState>,
    ) {
        if stmt.alternate.is_none() {
            let scope_id =
                ctx.create_child_scope_of_current(oxc_syntax::scope::ScopeFlags::empty());
            stmt.alternate = Some(Statement::new_block_statement_with_scope_id(
                SPAN,
                ArenaVec::new_in(ctx),
                scope_id,
                ctx,
            ));
        }
        if let Some(alt) = &mut stmt.alternate {
            inject_branch_counter_into_statement(
                alt,
                CounterKind::branch(self.cov_fn_name, branch_id, path_idx),
                ctx,
            );
        }
    }

    /// Wrap an optional-chain link's `object`/`callee` with the
    /// `cov_fn_oc(...)` helper so each `?.` site records whether the
    /// observed value was nullish (arm 0) or continued (arm 1). The
    /// branch entry is typed `optional-chain` with two locations: a
    /// zero-width slot anchored at the link's start, plus the link's
    /// full span. Both run at the same source position; the convention
    /// keeps the JSON-shape consistent with two-arm branch types and
    /// lets reporters render either arm without divergent special cases.
    pub(super) fn wrap_optional_chain_link(
        &mut self,
        object: &mut Expression<'arena>,
        link_span: Span,
        ctx: &TraverseCtx<'arena, CoverageState>,
    ) {
        // `gate_arms: false`: the `cov_fn_oc` helper references fixed arm
        // indices 0 and 1, so either both arms are registered or the link is
        // left unwrapped. Arm 0 is a zero-width anchor at the link's start;
        // arm 1 is the link's full span.
        let anchor = Span::new(link_span.start, link_span.start);
        let Some(reg) = self.register_branch(PendingBranch {
            branch_type: "optional-chain",
            umbrella_span: link_span,
            gate_arms: false,
            arms: vec![PendingArm::new(anchor), PendingArm::new(link_span)],
        }) else {
            return;
        };
        let branch_id = reg.branch_id;
        self.used_optional_chain_helper = true;

        // `cov_fn_oc(<original>, <branch_id>)` observes the value, increments
        // `b[id][0]` or `b[id][1]` on nullishness, and returns the value
        // unchanged so native `?.` semantics still fire. The three dispatch
        // points all gate on `track_optional_chain`, which is what sets the name.
        let oc_name = self
            .cov_fn_oc_name
            .expect("wrap_optional_chain_link runs only when track_optional_chain is on");
        let callee = Expression::new_identifier(SPAN, oc_name, ctx);
        let original = mem::replace(object, dummy_expr(ctx));
        let mut args = ArenaVec::new_in(ctx);
        args.push(Argument::from(original));
        args.push(Argument::from(index_literal(ctx, branch_id)));
        *object = Expression::new_call_expression(SPAN, callee, None, args, false, ctx);
    }
}

fn is_logical_assignment_operator(operator: oxc_syntax::operator::AssignmentOperator) -> bool {
    use oxc_syntax::operator::AssignmentOperator;

    matches!(
        operator,
        AssignmentOperator::LogicalOr
            | AssignmentOperator::LogicalAnd
            | AssignmentOperator::LogicalNullish
    )
}
