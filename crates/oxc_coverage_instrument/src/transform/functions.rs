//! `fnMap` entry registration for functions, arrows, methods and class
//! members, plus name inheritance for function-valued properties.

use std::mem;

use oxc_ast::ast::*;
use oxc_span::{GetSpan, SPAN, Span};
use oxc_traverse::TraverseCtx;

use crate::pragma::IgnoreType;

use super::counters::{CounterKind, build_counter_stmt, dummy_expr, prepend_counter};
use super::coverage_map::is_synthetic_span;
use super::ignore::method_ignored_by_pragma;
use super::names::{callback_argument_name, method_label, property_key_to_name};
use super::{CoverageState, CoverageTransform};

impl<'arena> CoverageTransform<'_, 'arena> {
    /// Register the `fnMap` entry for a function and queue its counter for the
    /// body hook, or record that its subtree is suppressed.
    pub(super) fn register_function_entry(
        &mut self,
        func: &Function<'arena>,
        ctx: &TraverseCtx<'arena, CoverageState>,
    ) {
        let has_pragma = ctx.state.pragmas.get(func.span.start) == Some(IgnoreType::Next);
        let ignored_named_function_expression = func.r#type == FunctionType::FunctionExpression
            && func
                .id
                .as_ref()
                .is_some_and(|id| self.ignore_class_methods.contains(&id.name.to_string()));
        // Istanbul cascades pragma and `ignoreClassMethods` skips into the body.
        let pragma_skip = has_pragma
            || self.skip_next
            || self.in_ignored_subtree()
            || ignored_named_function_expression;
        let fn_counter_only_skip = self.skip_fn_counter_only;
        self.skip_next = false;
        self.skip_fn_counter_only = false;
        self.ignored_fn_stack.push(pragma_skip);
        if pragma_skip {
            self.pending_name = None;
            self.pending_method_decl = None;
            return;
        }
        if fn_counter_only_skip {
            if func.body.is_some() {
                self.pending_fn_counters.push(None);
            }
            self.pending_name = None;
            self.pending_method_decl = None;
            return;
        }

        let name = self.resolve_function_name(func, ctx);
        // istanbul-lib-instrument points `decl` at the identifier itself:
        // `function foo()` gives the `foo` identifier span, a method gives a
        // marker at the method start (recorded by `register_method_definition`
        // or `register_object_property` before this hook runs), and an
        // anonymous `function ()` gives a
        // one-character marker where the name would have been.
        let decl_span = if let Some(id) = &func.id {
            id.span
        } else if let Some(span) = self.pending_method_decl.take() {
            span
        } else {
            Span::new(func.span.start, func.span.start + 1)
        };
        if let Some(body) = &func.body {
            // Pushed for every function with a body so the stack stays balanced
            // with the body hook's pop; `None` emits no counter.
            let fn_id = self.add_function(name, decl_span, body.span);
            self.pending_fn_counters.push(fn_id);
        }
    }

    /// Prepend the function counter to a body, if the function was registered.
    pub(super) fn insert_function_counter(
        &mut self,
        body: &mut FunctionBody<'arena>,
        ctx: &TraverseCtx<'arena, CoverageState>,
    ) {
        if self.in_ignored_subtree() {
            return;
        }
        // Popped unconditionally to stay balanced with `register_function_entry`;
        // only a registered function (`Some`) gets a counter.
        if let Some(Some(fn_id)) = self.pending_fn_counters.pop() {
            let cov_fn = self.cov_fn_name;
            let counter = build_counter_stmt(CounterKind::func(cov_fn, fn_id), ctx);
            body.statements.insert(0, counter);
        }
    }

    /// Register the `fnMap` entry for an arrow, or record that its subtree is
    /// suppressed.
    pub(super) fn register_arrow_entry(
        &mut self,
        arrow: &ArrowFunctionExpression<'arena>,
        ctx: &TraverseCtx<'arena, CoverageState>,
    ) {
        // Recorded before the pragma gate below, so the push stays balanced
        // with `convert_arrow_expression_body`'s unconditional pop, and for
        // every arrow: an ignored one still has its body wrapped, so it still
        // needs the `return` formed on the way out. A pre-expanded body no
        // longer reads as concise, hence the second half.
        let was_concise =
            arrow.body.is_expression() || self.pre_expanded_arrows.contains(arrow.span.start);
        self.arrow_expression_bodies.push(was_concise);

        let pragma_skip = ctx.state.pragmas.get(arrow.span.start) == Some(IgnoreType::Next)
            || self.skip_next
            || self.in_ignored_subtree();
        // Only pragma-driven skips suppress body statements.
        self.ignored_fn_stack.push(pragma_skip);
        if pragma_skip {
            self.skip_next = false;
            self.pending_name = None;
            return;
        }

        let pending_name = self.pending_name.take();
        let name = if self.istanbul_compat {
            format!("(anonymous_{})", self.fn_map.len())
        } else {
            pending_name
                .or_else(|| {
                    self.name_callback_arguments.then(|| callback_argument_name(ctx)).flatten()
                })
                .unwrap_or_else(|| format!("(anonymous_{})", self.fn_map.len()))
        };
        // `arrow.body.span()` dispatches to the block for `(a) => { .. }` and to
        // the bare expression for `(a) => a * 2`, which is the `loc` istanbul
        // records. A wrapper built by either expansion carries the expression's
        // own span, so it reads the same either way. Both leave out parentheses
        // around the expression, which Babel does not keep.
        let body_span = arrow
            .body
            .as_expression()
            .map_or_else(|| arrow.body.span(), |expr| expr.without_parentheses().span());
        let fn_id =
            self.add_function(name, Span::new(arrow.span.start, arrow.span.start + 1), body_span);

        // The counter needs a block to live in, which a concise arrow only gets
        // once its body is wrapped, so the body hook is what inserts it. Pushed
        // unconditionally to stay balanced with that hook's pop; `None` emits
        // no counter.
        self.pending_fn_counters.push(fn_id);
    }

    /// Turn the expression wrapped in a block by either expansion into a
    /// `return`, so the arrow keeps yielding its value now that its body is a
    /// block holding the coverage counters.
    pub(super) fn convert_arrow_expression_body(
        &mut self,
        arrow: &mut ArrowFunctionExpression<'arena>,
        ctx: &TraverseCtx<'arena, CoverageState>,
    ) {
        // Converting the wrapped expression into a `return` has to happen after
        // the body has been traversed: the `ExpressionStatement` is what earns
        // the statement counter, and the `return` replacing it is synthetic.
        let was_expression = self.arrow_expression_bodies.pop().unwrap_or(false);
        if was_expression
            && let Some(block) = arrow.body.as_function_body_mut()
            && !block.statements.is_empty()
        {
            // The wrapped expression is the last statement, not the first:
            // `insert_function_counter` has already prepended `++cov.f[N];`.
            if let Some(Statement::ExpressionStatement(expr_stmt)) = block.statements.last_mut() {
                let dummy = dummy_expr(ctx);
                let expr = mem::replace(&mut expr_stmt.expression, dummy);
                let last_idx = block.statements.len() - 1;
                block.statements[last_idx] = Statement::new_return_statement(SPAN, Some(expr), ctx);
            }
        }
        self.ignored_fn_stack.pop();
    }

    /// Name an anonymous default export `"default"`, per istanbul convention.
    pub(super) fn name_anonymous_default_export(
        &mut self,
        decl: &ExportDefaultDeclaration<'arena>,
    ) {
        if self.in_ignored_subtree() {
            return;
        }
        // Named function exports (`export default function foo`) keep their
        // declared identifier. Class exports do not need handling here
        // because their constructor (if any) receives its name via
        // `register_method_definition`, and a class without a constructor
        // produces no `fnMap` entry at all.
        let anonymous = match &decl.declaration {
            ExportDefaultDeclarationKind::FunctionDeclaration(func) => func.id.is_none(),
            ExportDefaultDeclarationKind::ArrowFunctionExpression(_) => true,
            _ => false,
        };
        if anonymous {
            self.pending_name = Some("default".to_string());
        }
    }

    /// Carry a method's name and `decl` span into its inner `Function`, or
    /// record that the method is suppressed.
    pub(super) fn register_method_definition(
        &mut self,
        method: &MethodDefinition<'arena>,
        ctx: &TraverseCtx<'arena, CoverageState>,
    ) {
        let parent_ignored = self.in_ignored_subtree();
        let key_span = method.key.span();
        if method_ignored_by_pragma(method, &ctx.state.pragmas, self.skip_next) {
            self.ignored_prop_stack.push(true);
            self.skip_next = false;
            return;
        }
        self.ignored_prop_stack.push(false);
        if parent_ignored {
            return;
        }
        if matches!(method.key, PropertyKey::PrivateIdentifier(_)) {
            // istanbul-lib-instrument instruments private method bodies for
            // statement coverage but does not surface them in `fnMap`.
            self.skip_fn_counter_only = true;
            return;
        }
        let named_key = (!method.computed).then_some(key_span);
        let decl = self.method_decl_span(method.span.start, named_key);
        let Some(name) = property_key_to_name(&method.key) else {
            self.pending_method_decl = Some(decl);
            return;
        };
        if self.ignore_class_methods.contains(&name) {
            if let Some(ignored) = self.ignored_prop_stack.last_mut() {
                *ignored = true;
            }
            return;
        }
        self.pending_name = Some(method_label(method.kind, name));
        self.pending_method_decl = Some(decl);
    }

    /// The `decl` of a method or a method-like object property.
    ///
    /// istanbul-lib-instrument has no identifier to point at, so it marks the
    /// first character of the method node, which includes modifiers such as
    /// `static`, `async`, `get` and `*`. The default profile widens that
    /// marker to the end of a non-computed key (`static bar` in
    /// `class C { static bar() {} }`).
    fn method_decl_span(&self, method_start: u32, named_key: Option<Span>) -> Span {
        match named_key {
            Some(key) if !self.istanbul_compat => Span::new(method_start, key.end),
            _ => Span::new(method_start, method_start.saturating_add(1)),
        }
    }

    /// Attach the statement counter for a class property initializer.
    pub(super) fn instrument_property_definition(
        &mut self,
        prop: &mut PropertyDefinition<'arena>,
        ctx: &TraverseCtx<'arena, CoverageState>,
    ) {
        let parent_ignored = self.in_ignored_subtree();
        let has_ignore_next =
            ctx.state.pragmas.get(prop.span.start) == Some(IgnoreType::Next) || self.skip_next;
        self.ignored_prop_stack.push(has_ignore_next);
        if has_ignore_next {
            self.skip_next = false;
            return;
        }
        if parent_ignored {
            return;
        }

        // Istanbul gives each class property initializer (`class Foo { x = expr }`)
        // a statement counter, but a `PropertyDefinition` is a class element
        // rather than a `Statement`, so the statement hook never sees it. The
        // initializer is wrapped in place: `x = (++cov.s[N], expr)`.
        let Some(value) = prop.value.as_mut() else { return };
        let span = value.without_parentheses().span();
        if is_synthetic_span(span) {
            return;
        }

        if let Some(stmt_id) = self.add_statement(span) {
            prepend_counter(value, CounterKind::stmt(self.cov_fn_name, stmt_id), ctx);
        }
    }

    /// Carry an object property's key name into its function-valued value, or
    /// record that the property is suppressed.
    pub(super) fn register_object_property(
        &mut self,
        prop: &ObjectProperty<'arena>,
        ctx: &TraverseCtx<'arena, CoverageState>,
    ) {
        let is_method_like =
            prop.method || matches!(prop.kind, PropertyKind::Get | PropertyKind::Set);
        let key_has_ignore_next = ctx.state.pragmas.get(prop.span.start) == Some(IgnoreType::Next)
            || ctx.state.pragmas.get(prop.key.span().start) == Some(IgnoreType::Next)
            || self.skip_next;
        let is_function_valued = matches!(
            prop.value,
            Expression::FunctionExpression(_) | Expression::ArrowFunctionExpression(_)
        );
        // On a method-like property the pragma suppresses the method, on a
        // plain data property it suppresses the value. A function-valued data
        // property is left to the inner function/arrow hook, which consumes
        // `skip_next` itself.
        let has_ignore_next = key_has_ignore_next && (is_method_like || !is_function_valued);
        self.push_prop_ignore_frame(has_ignore_next);

        if is_method_like && !has_ignore_next && !self.in_ignored_subtree() {
            let named_key = (!prop.computed).then(|| prop.key.span());
            self.pending_method_decl = Some(self.method_decl_span(prop.span.start, named_key));
        }

        // Carry the property's key name into the inner function or arrow so
        // `fnMap[N].name` reads as the source does instead of
        // `(anonymous_N)`. Covers shorthand methods, accessors and
        // function-valued properties.
        if !has_ignore_next && !self.in_ignored_subtree() {
            let inherits_name = is_method_like || is_function_valued;
            if inherits_name && let Some(base) = property_key_to_name(&prop.key) {
                let label = match prop.kind {
                    PropertyKind::Get => format!("get {base}"),
                    PropertyKind::Set => format!("set {base}"),
                    PropertyKind::Init => base,
                };
                self.pending_name = Some(label);
            }
        }
    }
}
