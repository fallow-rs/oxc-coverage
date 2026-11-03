//! Opt-in wall-time sampling per `fnMap` id.
//!
//! A timed function body becomes:
//!
//! ```js
//! function f() {
//!   cov_x.f[0]++;
//!   let cov_x_t = --cov_x_cd[0] > 0 ? -1 : cov_x_ts(0);
//!   try { /* original statements */ } finally { cov_x_t < 0 || cov_x_te(0, cov_x_t); }
//! }
//! ```
//!
//! `cov_x_cd` holds one countdown per function. A call that is not sampled
//! costs one decrement, one compare, and the `try`. When the countdown reaches
//! zero, `cov_x_ts` resets it and reads the clock. `cov_x_te` records the
//! elapsed time of a sampled call. The fast path is inline, because a helper
//! call on each function call cost about twice as much in the measurements. The `finally` clause
//! runs when the body completes, so an async function is measured over its
//! awaits.
//!
//! An async body can also return a promise without `await` (for example
//! `async () => db.query(q)`). The `finally` clause then runs before that
//! promise settles. So in an async body, `return expr` becomes
//! `return cov_x_tr(0, cov_x_t, expr, cov_x_t = -1)`: for a sampled call,
//! `cov_x_tr` records the time when the returned promise settles, and the
//! `finally` clause records nothing.
//!
//! A generator is never timed, because its body suspends at each `yield` and
//! the `finally` clause runs only when the iterator closes.
//!
//! The wrap moves each function declaration of the body into the `try` block,
//! so the declaration becomes block scoped. A body is not timed when that
//! changes its meaning: a function declaration that has the name of a
//! parameter or of a `var` of the body, or two function declarations with one
//! name. A block cannot hold these declarations in strict mode, and in sloppy
//! mode a parameter keeps its value. Moving the declarations in front of the
//! `try` is not safe either, because a declaration there cannot read the
//! `let` and `const` names of the body.

use std::fmt::Write;

use oxc_allocator::Vec as ArenaVec;
use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, VisitMut};
use oxc_span::SPAN;
use oxc_syntax::operator::{
    AssignmentOperator, BinaryOperator, LogicalOperator, UnaryOperator, UpdateOperator,
};
use oxc_syntax::scope::ScopeFlags;
use oxc_traverse::TraverseCtx;

use super::CoverageState;
use super::counters::index_literal;

/// Number of log2 duration buckets per function. Bucket 0 holds calls under
/// one microsecond. Bucket `k` holds calls of `2^(k-1)` up to `2^k`
/// microseconds. The last bucket also holds every longer call.
pub const TIMING_BUCKETS: u32 = 32;

/// Name of the global object that holds the timing data of each file.
pub const TIMING_VARIABLE: &str = "__fallow_timing__";

/// Arena-interned names of the timing helpers of one file.
#[derive(Clone, Copy)]
pub(super) struct TimingNames<'a> {
    /// `${cov_fn_name}_cd`: the countdown per function.
    pub(super) countdown: &'a str,
    /// `${cov_fn_name}_ts`: reset the countdown and return the start time.
    pub(super) start: &'a str,
    /// `${cov_fn_name}_te`: end a call and record a sampled duration.
    pub(super) end: &'a str,
    /// `${cov_fn_name}_tr`: end an async call at the settle of its return value.
    pub(super) settle: &'a str,
    /// `${cov_fn_name}_t`: the local that holds the start time.
    pub(super) local: &'a str,
}

/// Wrap the statements of `body` in `try { .. } finally { end(id, t) }` and
/// declare `let t = start(id)` in front of the `try`.
///
/// Statements in front of the first original statement (the function counter)
/// stay outside the `try`, so the counter does not count toward the time.
pub(super) fn wrap_body_with_timing<'a>(
    body: &mut FunctionBody<'a>,
    timed: TimedBody,
    names: TimingNames<'a>,
    ctx: &mut TraverseCtx<'a, CoverageState>,
) {
    let TimedBody { fn_id, leading, is_async } = timed;
    if is_async {
        let mut rewriter = AsyncReturnRewriter { fn_id, names, ctx };
        for stmt in body.statements.iter_mut().skip(leading) {
            rewriter.visit_statement(stmt);
        }
    }
    let leading = leading.min(body.statements.len());
    let mut outer = ArenaVec::new_in(ctx);
    let mut inner = ArenaVec::new_in(ctx);
    for (index, stmt) in body.statements.drain(..).enumerate() {
        if index < leading {
            outer.push(stmt);
        } else {
            inner.push(stmt);
        }
    }

    let start_call = start_expression(fn_id, names, ctx);
    let declarator = VariableDeclarator::new(
        SPAN,
        BindingPattern::new_binding_identifier(SPAN, names.local, ctx),
        None,
        Some(start_call),
        false,
        ctx,
    );
    outer.push(Statement::new_variable_declaration(
        SPAN,
        VariableDeclarationKind::Let,
        vec_of(ctx, [declarator]),
        false,
        ctx,
    ));

    let end_call = end_expression(fn_id, names, ctx);
    let end_stmt = Statement::new_expression_statement(SPAN, end_call, ctx);

    let try_scope = ctx.create_child_scope_of_current(ScopeFlags::empty());
    let finally_scope = ctx.create_child_scope_of_current(ScopeFlags::empty());
    let block = BlockStatement::boxed_with_scope_id(SPAN, inner, try_scope, ctx);
    let finalizer =
        BlockStatement::boxed_with_scope_id(SPAN, vec_of(ctx, [end_stmt]), finally_scope, ctx);
    outer.push(Statement::new_try_statement(SPAN, block, None, Some(finalizer), ctx));
    body.statements = outer;
}

/// Whether [`wrap_body_with_timing`] keeps the meaning of `body`.
///
/// It is false when a function declaration of the body has the name of a
/// parameter, of a `var` of the body, or of another function declaration of
/// the body. Inside the `try` block these declarations are a syntax error or
/// change the value of a name.
pub(super) fn can_wrap_body<'a>(params: &FormalParameters<'a>, body: &FunctionBody<'a>) -> bool {
    let functions = body_function_names(&body.statements);
    if functions.is_empty() {
        return true;
    }
    let mut names = VarNames::default();
    for pattern in params.iter_bindings() {
        names.add(pattern);
    }
    names.visit_statements(&body.statements);
    let mut seen: Vec<Ident<'a>> = Vec::with_capacity(functions.len());
    for name in functions {
        if seen.contains(&name) || names.names.contains(&name) {
            return false;
        }
        seen.push(name);
    }
    true
}

/// The names of the function declarations at the top level of a body.
fn body_function_names<'a>(statements: &[Statement<'a>]) -> Vec<Ident<'a>> {
    let mut names = Vec::new();
    for stmt in statements {
        let mut stmt = stmt;
        while let Statement::LabeledStatement(labeled) = stmt {
            stmt = &labeled.body;
        }
        if let Statement::FunctionDeclaration(func) = stmt
            && func.body.is_some()
            && let Some(id) = &func.id
        {
            names.push(id.name);
        }
    }
    names
}

/// The `var` names of one body and the parameter names, but not the names of
/// a nested function or class.
#[derive(Default)]
struct VarNames<'a> {
    names: Vec<Ident<'a>>,
}

impl<'a> VarNames<'a> {
    fn add(&mut self, pattern: &BindingPattern<'a>) {
        self.names.extend(pattern.get_binding_identifiers().into_iter().map(|id| id.name));
    }
}

impl<'a> Visit<'a> for VarNames<'a> {
    fn visit_function(&mut self, _it: &Function<'a>, _flags: ScopeFlags) {}

    fn visit_arrow_function_expression(&mut self, _it: &ArrowFunctionExpression<'a>) {}

    fn visit_class(&mut self, _it: &Class<'a>) {}

    fn visit_variable_declaration(&mut self, it: &VariableDeclaration<'a>) {
        if it.kind == VariableDeclarationKind::Var {
            for declarator in &it.declarations {
                self.add(&declarator.id);
            }
        }
    }
}

/// `--cd[id] > 0 ? -1 : start(id)`.
fn start_expression<'a>(
    fn_id: usize,
    names: TimingNames<'a>,
    ctx: &TraverseCtx<'a, CoverageState>,
) -> Expression<'a> {
    let slot = MemberExpression::new_computed_member_expression(
        SPAN,
        Expression::new_identifier(SPAN, names.countdown, ctx),
        index_literal(ctx, fn_id),
        false,
        ctx,
    );
    let decrement = Expression::new_update_expression(
        SPAN,
        UpdateOperator::Decrement,
        true,
        SimpleAssignmentTarget::from(slot),
        ctx,
    );
    let test = Expression::new_binary_expression(
        SPAN,
        decrement,
        BinaryOperator::GreaterThan,
        index_literal(ctx, 0),
        ctx,
    );
    let start = call(names.start, vec_of(ctx, [index_literal(ctx, fn_id)]), ctx);
    Expression::new_conditional_expression(SPAN, test, minus_one(ctx), start, ctx)
}

/// `t < 0 || end(id, t)`.
fn end_expression<'a>(
    fn_id: usize,
    names: TimingNames<'a>,
    ctx: &TraverseCtx<'a, CoverageState>,
) -> Expression<'a> {
    let test = Expression::new_binary_expression(
        SPAN,
        Expression::new_identifier(SPAN, names.local, ctx),
        BinaryOperator::LessThan,
        index_literal(ctx, 0),
        ctx,
    );
    let end = call(
        names.end,
        vec_of(
            ctx,
            [index_literal(ctx, fn_id), Expression::new_identifier(SPAN, names.local, ctx)],
        ),
        ctx,
    );
    Expression::new_logical_expression(SPAN, test, LogicalOperator::Or, end, ctx)
}

fn minus_one<'a>(ctx: &TraverseCtx<'a, CoverageState>) -> Expression<'a> {
    Expression::new_unary_expression(SPAN, UnaryOperator::UnaryNegation, index_literal(ctx, 1), ctx)
}

/// The body to wrap.
#[derive(Clone, Copy)]
pub(super) struct TimedBody {
    /// The `fnMap` id of the function.
    pub(super) fn_id: usize,
    /// Number of statements in front of the original statements.
    pub(super) leading: usize,
    /// Whether the function is async.
    pub(super) is_async: bool,
}

/// Rewrite each `return expr` of one async body, but not of a nested function,
/// to `return settle(id, t, expr, t = -1)`.
struct AsyncReturnRewriter<'c, 'a> {
    fn_id: usize,
    names: TimingNames<'a>,
    ctx: &'c TraverseCtx<'a, CoverageState>,
}

impl<'a> VisitMut<'a> for AsyncReturnRewriter<'_, 'a> {
    fn visit_function(&mut self, _it: &mut Function<'a>, _flags: ScopeFlags) {}

    fn visit_arrow_function_expression(&mut self, _it: &mut ArrowFunctionExpression<'a>) {}

    fn visit_class(&mut self, _it: &mut Class<'a>) {}

    fn visit_return_statement(&mut self, it: &mut ReturnStatement<'a>) {
        let Some(argument) = it.argument.take() else { return };
        let ctx = self.ctx;
        let local = self.names.local;
        let clear = Expression::new_assignment_expression(
            SPAN,
            AssignmentOperator::Assign,
            AssignmentTarget::new_assignment_target_identifier(SPAN, local, ctx),
            minus_one(ctx),
            ctx,
        );
        it.argument = Some(call(
            self.names.settle,
            vec_of(
                ctx,
                [
                    index_literal(ctx, self.fn_id),
                    Expression::new_identifier(SPAN, local, ctx),
                    argument,
                    clear,
                ],
            ),
            ctx,
        ));
    }
}

fn call<'a>(
    callee: &'a str,
    args: ArenaVec<'a, Expression<'a>>,
    ctx: &TraverseCtx<'a, CoverageState>,
) -> Expression<'a> {
    let mut arguments = ArenaVec::new_in(ctx);
    arguments.extend(args.into_iter().map(Argument::from));
    Expression::new_call_expression(
        SPAN,
        Expression::new_identifier(SPAN, callee, ctx),
        None,
        arguments,
        false,
        ctx,
    )
}

fn vec_of<'a, T, const N: usize>(
    ctx: &TraverseCtx<'a, CoverageState>,
    items: [T; N],
) -> ArenaVec<'a, T> {
    let mut out = ArenaVec::with_capacity_in(N, ctx);
    out.extend(items);
    out
}

/// Inputs to [`append_timing_helpers`].
pub struct TimingPreambleInputs<'a> {
    /// The file path, the key of the file in `__fallow_timing__`.
    pub path: &'a str,
    /// The hash of the coverage map. A changed hash resets the timing data.
    pub coverage_hash: &'a str,
    /// Per-file coverage function name; the helper names derive from it.
    pub cov_fn_name: &'a str,
    /// Number of `fnMap` entries.
    pub function_count: usize,
    /// One call in this many is timed, per function.
    pub sample_every: u32,
}

/// Append the timing store and the start and end helpers to the preamble.
///
/// The countdown of each function starts at a random value in
/// `1..=sample_every`, so the first sampled call of each function is not
/// always the cold first call.
pub fn append_timing_helpers(buf: &mut String, inputs: &TimingPreambleInputs<'_>) {
    let TimingPreambleInputs { path, coverage_hash, cov_fn_name, function_count, sample_every } =
        *inputs;
    let path_json = serde_json::to_string(path).expect("serializing a &str to JSON is infallible");
    let hash_json =
        serde_json::to_string(coverage_hash).expect("serializing a &str to JSON is infallible");
    let n = function_count;
    let every = sample_every;
    let buckets = TIMING_BUCKETS;
    let last = TIMING_BUCKETS - 1;
    let var = TIMING_VARIABLE;
    let _ = writeln!(
        buf,
        "var {cov_fn_name}_tm = (function () {{ var g = typeof globalThis !== 'undefined' ? globalThis : typeof global !== 'undefined' ? global : typeof self !== 'undefined' ? self : this; if (!g.{var}) {{ g.{var} = Object.create(null); }} var path = {path_json}; var hash = {hash_json}; var e = g.{var}[path]; if (!e || e.hash !== hash) {{ e = {{ path: path, hash: hash, sampleEvery: {every}, unit: 'ms', c: [], s: [], b: [] }}; for (var i = 0; i < {n}; i++) {{ e.c.push(0); e.s.push(0); e.b.push(null); }} g.{var}[path] = e; }} return e; }})();"
    );
    let _ = writeln!(
        buf,
        "var {cov_fn_name}_cd = new Int32Array({n}); for (var {cov_fn_name}_i = 0; {cov_fn_name}_i < {n}; {cov_fn_name}_i++) {{ {cov_fn_name}_cd[{cov_fn_name}_i] = 1 + Math.floor(Math.random() * {every}); }}"
    );
    let _ = writeln!(
        buf,
        "var {cov_fn_name}_now = typeof performance !== 'undefined' && typeof performance.now === 'function' ? function () {{ return performance.now(); }} : Date.now;"
    );
    let _ = writeln!(
        buf,
        "function {cov_fn_name}_ts(id) {{ {cov_fn_name}_cd[id] = {every}; return {cov_fn_name}_now(); }}"
    );
    let _ = writeln!(
        buf,
        "function {cov_fn_name}_tr(id, t, v) {{ if (t >= 0 && v instanceof Promise) {{ var done = function () {{ {cov_fn_name}_te(id, t); }}; v.then(done, done); }} else {{ {cov_fn_name}_te(id, t); }} return v; }}"
    );
    let _ = writeln!(
        buf,
        "function {cov_fn_name}_te(id, t) {{ if (t < 0) {{ return; }} var d = {cov_fn_name}_now() - t; if (!(d >= 0)) {{ d = 0; }} var m = {cov_fn_name}_tm; m.c[id]++; m.s[id] += d; var u = Math.floor(d * 1000); var k = u < 1 ? 0 : 32 - Math.clz32(u > 4294967295 ? 4294967295 : u); if (k > {last}) {{ k = {last}; }} var b = m.b[id]; if (b === null) {{ b = m.b[id] = []; for (var i = 0; i < {buckets}; i++) {{ b.push(0); }} }} b[k]++; }}"
    );
}
