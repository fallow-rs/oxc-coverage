//! AST-level coverage transform, driven by `oxc_traverse`.
//!
//! Collects the statement, function and branch spans that make up the
//! coverage map while injecting the matching counter expressions
//! (`cov_fn.s[N]++` and friends) into the AST, converting arrow expression
//! bodies to block bodies where a counter needs a statement slot.
//!
//! ## Example
//!
//! Input:
//!
//! ```js
//! function f(x) {
//!   if (x) return 1;
//! }
//! ```
//!
//! Output, below the preamble that defines `cov_1abc`:
//!
//! ```js
//! function f(x) {
//!   ++cov_1abc.f[0];
//!   ++cov_1abc.s[0];
//!   if (x) {
//!     ++cov_1abc.b[0][0];
//!     ++cov_1abc.s[1];
//!     return 1;
//!   } else {
//!     ++cov_1abc.b[0][1];
//!   }
//! }
//! ```
//!
//! ## Files
//!
//! - `mod.rs`: the transform state and the `Traverse` impl that dispatches
//!   each visitor hook to the file owning that concern.
//! - `branches.rs`: `if`, ternary, `switch`, default-argument, logical
//!   assignment and optional-chain branch instrumentation.
//! - `counters.rs`: construction of the counter expressions and statements,
//!   and the pending-insertion records that place them.
//! - `coverage_map.rs`: span to `Location` conversion and the `statementMap` /
//!   `fnMap` / `branchMap` registration, including the eager-remap gate.
//! - `functions.rs`: `fnMap` entry registration for functions, arrows, methods
//!   and class members, and class-field counter hoisting.
//! - `ignore.rs`: the predicates deciding whether a node is suppressed by a
//!   pragma, an enclosing ignored subtree, or `ignoreClassMethods`.
//! - `logical.rs`: logical-chain flattening and per-leaf counter wrapping.
//! - `names.rs`: `fnMap` name inference from declarations, bindings, property
//!   keys, assignment targets and call arguments.
//! - `preamble.rs`: the per-file coverage-object IIFE inserted after directives.
//! - `statements.rs`: statement-counter placement, including the hoists that
//!   keep `Function.name` inference intact.

use std::collections::BTreeMap;

use oxc_allocator::{Allocator, Vec as ArenaVec};
use oxc_ast::ast::*;
use oxc_coverage_types::{BranchEntry, FnEntry, Location};
use oxc_span::{GetSpan, Span};
use oxc_traverse::{Traverse, TraverseCtx};

use crate::{
    arrow_body::{PreExpandedArrows, wrap_concise_arrow_body},
    pragma::{IgnoreType, PragmaMap},
    source_text,
};

mod branches;
mod counters;
mod coverage_map;
mod functions;
mod ignore;
mod logical;
mod names;
mod preamble;
mod statements;

use counters::PendingInsertion;
use coverage_map::is_synthetic_span;
use ignore::{
    is_ignored_case, jsx_attribute_ignored, jsx_child_ignored, jsx_spread_attribute_ignored,
};
use names::assignment_target_name;

pub use preamble::{PreambleInputs, djb31_hex, generate_cov_fn_name, generate_preamble_source};

/// State carried through the traverse for coverage instrumentation.
pub struct CoverageState {
    /// Pragma map for istanbul/v8 ignore directives.
    pub pragmas: PragmaMap,
}

/// Collects coverage metadata and injects counter expressions via AST mutation.
pub struct CoverageTransform<'src, 'arena> {
    source: &'src str,
    line_offsets: Vec<u32>,
    /// True when the source is pure ASCII so columns can be reported as
    /// `offset - line_start` without walking chars for UTF-16 width.
    source_is_ascii: bool,
    /// Function entries indexed by sequential id. Materialized into a
    /// `BTreeMap<String, FnEntry>` once in `build_file_coverage`.
    pub fn_map: Vec<FnEntry>,
    /// Statement locations indexed by sequential id.
    pub statement_map: Vec<Location>,
    /// Branch entries indexed by sequential id.
    pub branch_map: Vec<BranchEntry>,
    /// Body byte spans parallel to `branch_map[i].locations[j]`. For most
    /// branch shapes the body span equals the location span; the exception is
    /// if-arm 0, where `locations[0]` records the whole `IfStatement` span
    /// (istanbul convention) while the body span records the consequent
    /// BlockStatement / inner-statement span. `v8_to_istanbul` consumes these
    /// when resolving arm counts because V8 only emits per-block ranges and
    /// has no range tight to istanbul's whole-IfStatement convention. Slots
    /// with `(0, 0)` represent unknown bodies (e.g. synthetic else-arms) and
    /// are skipped by callers.
    pub branch_arm_body_byte_spans: Vec<Vec<(u32, u32)>>,
    /// Names inherited from a parent node (binding, property key, assignment
    /// target), keyed on the start of the function or arrow that takes them.
    inferred_names: BTreeMap<u32, String>,
    /// Counters to inject before the statement whose span starts at
    /// [`PendingInsertion::target_start`].
    pending_insertions: Vec<PendingInsertion>,
    /// Stack of pending function entry counters. Supports nested functions and
    /// arrows, where an inner function is entered before the outer's body is
    /// visited. `None` is pushed when the eager-compose gate skipped the
    /// function, so the push/pop balance with the traversal nesting is
    /// preserved; the body hook emits a counter only for `Some` entries.
    pending_fn_counters: Vec<Option<usize>>,
    /// Per-arrow record of whether the arrow was written with a concise body,
    /// pushed by `register_arrow_entry` and popped by
    /// `convert_arrow_expression_body`. Oxc dropped
    /// `ArrowFunctionExpression::expression` in 0.143, and the body shape cannot
    /// answer the question on the way out because the arrow has a block by then.
    arrow_expression_bodies: Vec<bool>,
    /// Arrows whose concise body was wrapped before the traverse started,
    /// because a span-rewriting pass ran in between. Read on entry, where a
    /// wrapped body no longer looks concise. Empty for every caller that runs
    /// no such pass.
    pre_expanded_arrows: PreExpandedArrows,
    /// Per-frame record of whether the current function or arrow is being ignored
    /// (i.e. its subtree should not be instrumented). Mirrors Istanbul's `path.skip()`:
    /// when true at any ancestor frame, statements in the body are not counted.
    ignored_fn_stack: Vec<bool>,
    /// Per-statement record of whether an `ignore next` pragma targets that
    /// statement. While any frame is true, the full statement subtree is skipped.
    ignored_stmt_stack: Vec<bool>,
    /// Per-class-property record of whether an `ignore next` pragma targets
    /// the property. Property definitions are not statements in Oxc's AST, but
    /// Istanbul still treats their initializer subtree as skippable.
    ignored_prop_stack: Vec<bool>,
    /// Per-switch-case record of whether an `ignore next` pragma targets the
    /// case label or, in the default profile, its first consequent statement.
    ignored_switch_case_stack: Vec<bool>,
    /// Per-conditional, per-logical-expression and per-default record of
    /// whether an `ignore next` pragma targets that node. A default is a
    /// parameter default or an `AssignmentPattern` in Babel terms.
    ignored_expr_stack: Vec<bool>,
    /// Spans for `if` arms suppressed by `/* istanbul ignore if */` or
    /// `/* istanbul ignore else */`. The branch visitor decides which arm is
    /// suppressed, while statement/function visitors use this to skip nested
    /// counters as Istanbul does.
    ignored_if_arm_spans: Vec<Span>,
    /// Number of ignored arm spans pushed by each entered `if`, so exit can pop
    /// only the spans owned by that node.
    ignored_if_arm_push_counts: Vec<usize>,
    /// When true, skip instrumentation for the next node.
    skip_next: bool,
    /// `fnMap` ids reserved when a method or a method-like object property
    /// is entered, keyed on the start of its value function. The value
    /// function hook pops the entry, so a function in a decorator or in a
    /// computed key, which the traverse enters first, gets a later id. `None`
    /// is reserved for a method that gets no counter (a private method, or one
    /// the eager-compose gate dropped).
    reserved_method_fns: Vec<(u32, Option<usize>)>,
    /// True while traversing a `VariableDeclaration` carrying an `ignore next`
    /// pragma. Consumed by `enter_variable_declarator` to skip both the
    /// per-declarator statement counter and any inner function counter.
    skip_current_var_decl: bool,
    /// Coverage function name, pre-allocated in the AST arena so counter
    /// builders can reference it as `&'arena str` without re-interning per call.
    cov_fn_name: &'arena str,
    /// `${cov_fn_name}_bt` helper name, also pre-interned. Only set when
    /// `report_logic` is enabled.
    cov_fn_bt_name: Option<&'arena str>,
    /// `${cov_fn_name}_oc` optional-chain link observer, pre-interned so
    /// wrapping a `?.` link costs no allocation. `None` when
    /// `track_optional_chain` is off, where no link is ever wrapped.
    cov_fn_oc_name: Option<&'arena str>,
    /// When true, adds truthy-value tracking (`bT`) for logical expression operands.
    report_logic: bool,
    /// When true (the default), receiver-safe optional-chaining (`?.`) links are
    /// wrapped in the `cov_fn_oc` helper and registered as `optional-chain`
    /// branches. Receiver-bound optional calls stay native so their `this`
    /// binding is preserved. When false every chain is left native.
    track_optional_chain: bool,
    /// Class method names to exclude from coverage instrumentation.
    ignore_class_methods: Vec<String>,
    /// When true, an anonymous function/arrow that is a direct call/`new`
    /// argument is named from the callee instead of `(anonymous_N)`.
    name_callback_arguments: bool,
    /// When true, a private class method (`#name() {}`) gets its own `fnMap`
    /// entry. Only the V8 collect path sets it.
    record_private_methods: bool,
    /// Whether transform details should match `istanbul-lib-instrument`.
    istanbul_compat: bool,
    /// Branch IDs of logical expression branches (for building the `bT` map).
    pub logical_branch_ids: Vec<usize>,
    /// Set once any optional-chain link has been wrapped in the `cov_fn_oc`
    /// helper, so the preamble emits that helper. Read instead of scanning the
    /// branch map for an `optional-chain` type, because an eager fold can
    /// collapse an `optional-chain` entry onto a differently-typed one (the
    /// fold key excludes `branch_type`) while the emitted `_oc` call remains.
    pub used_optional_chain_helper: bool,
    /// Eager-compose gate. When `Some`, a coverage point whose positions do not
    /// remap through the input source map is not instrumented at all: no map
    /// entry is registered and no counter is emitted, so the runtime coverage
    /// object and the emitted counters agree. `None` for every non-eager
    /// caller, where gating is a strict no-op.
    eager_remapper: Option<&'src oxc_coverage_source_maps::PositionRemapper>,
    /// Statement ids already handed out per remapped location, so statements
    /// that collapse onto one original location under the eager gate share a
    /// counter. Empty outside eager mode.
    eager_statement_ids: BTreeMap<coverage_map::EagerMergeKey, usize>,
    /// Function ids already handed out per remapped `decl` location; the
    /// function counterpart of `eager_statement_ids`.
    eager_function_ids: BTreeMap<coverage_map::EagerMergeKey, usize>,
    /// Branch ids already handed out per remapped surviving-arm vector, so
    /// branches that collapse onto one original arm vector under the eager gate
    /// share a counter. Empty outside eager mode. Mirrors `eager_statement_ids`
    /// / `eager_function_ids`.
    eager_branch_ids: BTreeMap<coverage_map::BranchKey, usize>,
    /// Set when an eager function fold merges two functions whose identities
    /// differ, so `finalize` drops the `x_fallow_functionMap` overlay the way
    /// the deferred merge does. Never set outside eager mode.
    pub eager_function_overlay_conflict: bool,
}

/// Inputs to [`CoverageTransform::new`], grouped so the constructor stays at
/// a single parameter even as new options accrete.
pub struct TransformInit<'src, 'arena> {
    /// Bump allocator owning the AST being traversed.
    pub allocator: &'arena Allocator,
    /// The source text; used for line-offset precomputation and span lookups.
    pub source: &'src str,
    /// Per-file IIFE function name (e.g. `cov_<hash>`); copied into the arena
    /// so AST identifiers can refer to it for the lifetime of the traversal.
    pub cov_fn_name: &'src str,
    /// When true, emits the truthy-value tracker (`bT` counters) for logical
    /// expression operands.
    pub report_logic: bool,
    /// When true (the default), receiver-safe optional-chain (`?.`) links are
    /// tracked via the `cov_fn_oc` helper. Receiver-bound optional calls and all
    /// links when false are left native.
    pub track_optional_chain: bool,
    /// Class method and named-function-expression identifiers to skip,
    /// matching Istanbul's `ignoreClassMethods` semantics.
    pub ignore_class_methods: Vec<String>,
    /// When true, name an otherwise-anonymous function/arrow that is a direct
    /// call/`new` argument from the callee (`arr.map(cb)` -> `"map"`). Opt-in;
    /// Istanbul leaves these `(anonymous_N)`.
    pub name_callback_arguments: bool,
    /// When true, a private class method (`#name() {}`) gets a `fnMap` entry
    /// named `#name`. Istanbul gives a private method no `fnMap` entry, so
    /// instrumentation keeps this false. The V8 collect path sets it, because
    /// V8 reports a function range for the private method, and a consumer must
    /// be able to match that range to the method.
    pub record_private_methods: bool,
    /// Arrows whose concise body a caller wrapped before handing the program
    /// over, because a pass between parse and traverse rewrites spans. Default
    /// (empty) for every caller that runs no such pass.
    pub pre_expanded_arrows: PreExpandedArrows,
    /// Whether transform details should match `istanbul-lib-instrument`.
    pub istanbul_compat: bool,
    /// Eager-compose position-remap gate. `Some` only when
    /// `compose_input_source_map` is on and a usable input map is present;
    /// `None` for every other caller, where gating is a strict no-op.
    pub eager_remapper: Option<&'src oxc_coverage_source_maps::PositionRemapper>,
}

impl<'src, 'arena> CoverageTransform<'src, 'arena> {
    pub fn new(init: TransformInit<'src, 'arena>) -> Self {
        let TransformInit {
            allocator,
            source,
            cov_fn_name,
            report_logic,
            track_optional_chain,
            ignore_class_methods,
            name_callback_arguments,
            record_private_methods,
            istanbul_compat,
            pre_expanded_arrows,
            eager_remapper,
        } = init;
        let cov_fn_name = allocator.alloc_str(cov_fn_name);
        Self {
            source,
            line_offsets: source_text::line_starts(source),
            source_is_ascii: source.is_ascii(),
            fn_map: Vec::new(),
            statement_map: Vec::new(),
            branch_map: Vec::new(),
            branch_arm_body_byte_spans: Vec::new(),
            inferred_names: BTreeMap::new(),
            pending_insertions: Vec::new(),
            pending_fn_counters: Vec::new(),
            arrow_expression_bodies: Vec::new(),
            pre_expanded_arrows,
            ignored_fn_stack: Vec::new(),
            ignored_stmt_stack: Vec::new(),
            ignored_prop_stack: Vec::new(),
            ignored_switch_case_stack: Vec::new(),
            ignored_expr_stack: Vec::new(),
            ignored_if_arm_spans: Vec::new(),
            ignored_if_arm_push_counts: Vec::new(),
            skip_next: false,
            reserved_method_fns: Vec::new(),
            skip_current_var_decl: false,
            cov_fn_name,
            cov_fn_bt_name: report_logic.then(|| allocator.alloc_str(&format!("{cov_fn_name}_bt"))),
            cov_fn_oc_name: track_optional_chain
                .then(|| allocator.alloc_str(&format!("{cov_fn_name}_oc"))),
            report_logic,
            track_optional_chain,
            ignore_class_methods,
            name_callback_arguments,
            record_private_methods,
            istanbul_compat,
            logical_branch_ids: Vec::new(),
            used_optional_chain_helper: false,
            eager_remapper,
            eager_statement_ids: BTreeMap::new(),
            eager_function_ids: BTreeMap::new(),
            eager_branch_ids: BTreeMap::new(),
            eager_function_overlay_conflict: false,
        }
    }
}

impl<'a> Traverse<'a, CoverageState> for CoverageTransform<'_, 'a> {
    fn enter_function(
        &mut self,
        func: &mut Function<'a>,
        ctx: &mut TraverseCtx<'a, CoverageState>,
    ) {
        self.register_function_entry(func, ctx);
    }

    fn exit_function(
        &mut self,
        _func: &mut Function<'a>,
        _ctx: &mut TraverseCtx<'a, CoverageState>,
    ) {
        self.ignored_fn_stack.pop();
    }

    fn enter_function_body(
        &mut self,
        body: &mut FunctionBody<'a>,
        ctx: &mut TraverseCtx<'a, CoverageState>,
    ) {
        self.insert_function_counter(body, ctx);
    }

    fn enter_arrow_function_expression(
        &mut self,
        arrow: &mut ArrowFunctionExpression<'a>,
        ctx: &mut TraverseCtx<'a, CoverageState>,
    ) {
        self.register_arrow_entry(arrow, ctx);
    }

    fn enter_arrow_function_body(
        &mut self,
        body: &mut ArrowFunctionBody<'a>,
        ctx: &mut TraverseCtx<'a, CoverageState>,
    ) {
        // Runs before traverse descends: the walk re-reads the body after this
        // hook, so a concise body wrapped here is visited as a block, and its
        // `enter_function_body`, `enter_statement` and `exit_statements` hooks
        // fire as they did for the block the 0.142 parser produced. A body the
        // pre-expansion already wrapped is left alone.
        wrap_concise_arrow_body(body, ctx);
    }

    fn exit_arrow_function_expression(
        &mut self,
        arrow: &mut ArrowFunctionExpression<'a>,
        ctx: &mut TraverseCtx<'a, CoverageState>,
    ) {
        self.convert_arrow_expression_body(arrow, ctx);
    }

    fn enter_variable_declaration(
        &mut self,
        decl: &mut VariableDeclaration<'a>,
        ctx: &mut TraverseCtx<'a, CoverageState>,
    ) {
        // `VariableDeclaration` is a container statement (see
        // `is_container_statement`), so `enter_statement` never sees its
        // pragma; consult it here.
        if ctx.state.pragmas.get(decl.span.start) == Some(IgnoreType::Next) {
            self.skip_current_var_decl = true;
        }
    }

    fn exit_variable_declaration(
        &mut self,
        _decl: &mut VariableDeclaration<'a>,
        _ctx: &mut TraverseCtx<'a, CoverageState>,
    ) {
        self.skip_current_var_decl = false;
    }

    fn enter_variable_declarator(
        &mut self,
        decl: &mut VariableDeclarator<'a>,
        ctx: &mut TraverseCtx<'a, CoverageState>,
    ) {
        self.instrument_variable_declarator(decl, ctx);
    }

    fn enter_export_default_declaration(
        &mut self,
        decl: &mut ExportDefaultDeclaration<'a>,
        _ctx: &mut TraverseCtx<'a, CoverageState>,
    ) {
        self.name_anonymous_default_export(decl);
    }

    fn enter_method_definition(
        &mut self,
        method: &mut MethodDefinition<'a>,
        ctx: &mut TraverseCtx<'a, CoverageState>,
    ) {
        self.register_method_definition(method, ctx);
    }

    fn exit_method_definition(
        &mut self,
        _method: &mut MethodDefinition<'a>,
        _ctx: &mut TraverseCtx<'a, CoverageState>,
    ) {
        self.ignored_prop_stack.pop();
    }

    fn enter_property_definition(
        &mut self,
        prop: &mut PropertyDefinition<'a>,
        ctx: &mut TraverseCtx<'a, CoverageState>,
    ) {
        self.instrument_property_definition(prop, ctx);
    }

    fn exit_property_definition(
        &mut self,
        _prop: &mut PropertyDefinition<'a>,
        _ctx: &mut TraverseCtx<'a, CoverageState>,
    ) {
        self.ignored_prop_stack.pop();
    }

    fn enter_object_property(
        &mut self,
        prop: &mut ObjectProperty<'a>,
        ctx: &mut TraverseCtx<'a, CoverageState>,
    ) {
        self.register_object_property(prop, ctx);
    }

    fn exit_object_property(
        &mut self,
        _prop: &mut ObjectProperty<'a>,
        _ctx: &mut TraverseCtx<'a, CoverageState>,
    ) {
        self.ignored_prop_stack.pop();
    }

    fn enter_statement(
        &mut self,
        stmt: &mut Statement<'a>,
        ctx: &mut TraverseCtx<'a, CoverageState>,
    ) {
        self.register_statement_counter(stmt, ctx);
    }

    fn exit_statement(
        &mut self,
        _stmt: &mut Statement<'a>,
        _ctx: &mut TraverseCtx<'a, CoverageState>,
    ) {
        self.finish_statement();
    }

    fn exit_statements(
        &mut self,
        stmts: &mut ArenaVec<'a, Statement<'a>>,
        ctx: &mut TraverseCtx<'a, CoverageState>,
    ) {
        self.insert_pending_statement_counters(stmts, ctx);
    }

    fn enter_if_statement(
        &mut self,
        stmt: &mut IfStatement<'a>,
        ctx: &mut TraverseCtx<'a, CoverageState>,
    ) {
        self.instrument_if_branches(stmt, ctx);
    }

    fn exit_if_statement(
        &mut self,
        _stmt: &mut IfStatement<'a>,
        _ctx: &mut TraverseCtx<'a, CoverageState>,
    ) {
        self.pop_ignored_if_arms();
    }

    fn enter_conditional_expression(
        &mut self,
        expr: &mut ConditionalExpression<'a>,
        ctx: &mut TraverseCtx<'a, CoverageState>,
    ) {
        self.instrument_conditional_branches(expr, ctx);
        self.push_expr_ignore_frame(expr.span.start, &ctx.state.pragmas);
    }

    fn exit_conditional_expression(
        &mut self,
        _expr: &mut ConditionalExpression<'a>,
        _ctx: &mut TraverseCtx<'a, CoverageState>,
    ) {
        self.ignored_expr_stack.pop();
    }

    fn enter_switch_statement(
        &mut self,
        stmt: &mut SwitchStatement<'a>,
        ctx: &mut TraverseCtx<'a, CoverageState>,
    ) {
        self.instrument_switch_cases(stmt, ctx);
    }

    fn enter_switch_case(
        &mut self,
        case: &mut SwitchCase<'a>,
        ctx: &mut TraverseCtx<'a, CoverageState>,
    ) {
        self.ignored_switch_case_stack.push(is_ignored_case(
            case,
            &ctx.state.pragmas,
            self.istanbul_compat,
        ));
    }

    fn exit_switch_case(
        &mut self,
        _case: &mut SwitchCase<'a>,
        _ctx: &mut TraverseCtx<'a, CoverageState>,
    ) {
        self.ignored_switch_case_stack.pop();
    }

    fn enter_jsx_attribute(
        &mut self,
        attr: &mut JSXAttribute<'a>,
        ctx: &mut TraverseCtx<'a, CoverageState>,
    ) {
        let ignored = jsx_attribute_ignored(attr, &ctx.state.pragmas, self.skip_next);
        self.push_prop_ignore_frame(ignored);
    }

    fn exit_jsx_attribute(
        &mut self,
        _attr: &mut JSXAttribute<'a>,
        _ctx: &mut TraverseCtx<'a, CoverageState>,
    ) {
        self.ignored_prop_stack.pop();
    }

    fn enter_jsx_spread_attribute(
        &mut self,
        attr: &mut JSXSpreadAttribute<'a>,
        ctx: &mut TraverseCtx<'a, CoverageState>,
    ) {
        let ignored = jsx_spread_attribute_ignored(attr, &ctx.state.pragmas, self.skip_next);
        self.push_prop_ignore_frame(ignored);
    }

    fn exit_jsx_spread_attribute(
        &mut self,
        _attr: &mut JSXSpreadAttribute<'a>,
        _ctx: &mut TraverseCtx<'a, CoverageState>,
    ) {
        self.ignored_prop_stack.pop();
    }

    fn enter_jsx_child(
        &mut self,
        child: &mut JSXChild<'a>,
        ctx: &mut TraverseCtx<'a, CoverageState>,
    ) {
        let ignored = jsx_child_ignored(child, &ctx.state.pragmas, self.skip_next);
        self.push_prop_ignore_frame(ignored);
    }

    fn exit_jsx_child(
        &mut self,
        _child: &mut JSXChild<'a>,
        _ctx: &mut TraverseCtx<'a, CoverageState>,
    ) {
        self.ignored_prop_stack.pop();
    }

    fn enter_logical_expression(
        &mut self,
        expr: &mut LogicalExpression<'a>,
        ctx: &mut TraverseCtx<'a, CoverageState>,
    ) {
        self.instrument_logical_expression(expr, ctx);
        self.push_expr_ignore_frame(expr.span.start, &ctx.state.pragmas);
    }

    fn exit_logical_expression(
        &mut self,
        _expr: &mut LogicalExpression<'a>,
        _ctx: &mut TraverseCtx<'a, CoverageState>,
    ) {
        self.ignored_expr_stack.pop();
    }

    // Istanbul does not treat `for` / `while` / `do-while` as branches.
    // Loop coverage comes from the statement counters in the body alone.

    fn exit_with_statement(
        &mut self,
        stmt: &mut WithStatement<'a>,
        ctx: &mut TraverseCtx<'a, CoverageState>,
    ) {
        self.inject_pending_counters_into_statement_child(&mut stmt.body, ctx);
    }

    fn exit_labeled_statement(
        &mut self,
        stmt: &mut LabeledStatement<'a>,
        _ctx: &mut TraverseCtx<'a, CoverageState>,
    ) {
        let body_span = stmt.body.span();
        if !is_synthetic_span(body_span) {
            // Wrapping `label: while (...)` in a block would break
            // `continue label`, so the child counter is emitted before the
            // label instead.
            self.retarget_pending_insertions(body_span.start, stmt.span.start);
        }
    }

    fn exit_do_while_statement(
        &mut self,
        stmt: &mut DoWhileStatement<'a>,
        ctx: &mut TraverseCtx<'a, CoverageState>,
    ) {
        self.inject_pending_counters_into_statement_child(&mut stmt.body, ctx);
    }

    fn exit_while_statement(
        &mut self,
        stmt: &mut WhileStatement<'a>,
        ctx: &mut TraverseCtx<'a, CoverageState>,
    ) {
        self.inject_pending_counters_into_statement_child(&mut stmt.body, ctx);
    }

    fn exit_for_statement(
        &mut self,
        stmt: &mut ForStatement<'a>,
        ctx: &mut TraverseCtx<'a, CoverageState>,
    ) {
        self.inject_pending_counters_into_statement_child(&mut stmt.body, ctx);
    }

    fn exit_for_in_statement(
        &mut self,
        stmt: &mut ForInStatement<'a>,
        ctx: &mut TraverseCtx<'a, CoverageState>,
    ) {
        self.inject_pending_counters_into_statement_child(&mut stmt.body, ctx);
    }

    fn exit_for_of_statement(
        &mut self,
        stmt: &mut ForOfStatement<'a>,
        ctx: &mut TraverseCtx<'a, CoverageState>,
    ) {
        self.inject_pending_counters_into_statement_child(&mut stmt.body, ctx);
    }

    fn enter_formal_parameter(
        &mut self,
        param: &mut FormalParameter<'a>,
        ctx: &mut TraverseCtx<'a, CoverageState>,
    ) {
        self.instrument_parameter_default(param, ctx);
    }

    fn exit_formal_parameter(
        &mut self,
        _param: &mut FormalParameter<'a>,
        _ctx: &mut TraverseCtx<'a, CoverageState>,
    ) {
        self.ignored_expr_stack.pop();
    }

    fn enter_static_member_expression(
        &mut self,
        member: &mut StaticMemberExpression<'a>,
        ctx: &mut TraverseCtx<'a, CoverageState>,
    ) {
        if self.track_optional_chain && member.optional && !self.in_ignored_subtree() {
            self.wrap_optional_chain_link(&mut member.object, member.span, ctx);
        }
    }

    fn enter_computed_member_expression(
        &mut self,
        member: &mut ComputedMemberExpression<'a>,
        ctx: &mut TraverseCtx<'a, CoverageState>,
    ) {
        if self.track_optional_chain && member.optional && !self.in_ignored_subtree() {
            self.wrap_optional_chain_link(&mut member.object, member.span, ctx);
        }
    }

    fn enter_call_expression(
        &mut self,
        call: &mut CallExpression<'a>,
        ctx: &mut TraverseCtx<'a, CoverageState>,
    ) {
        // Wrapping `object.method` in `_oc(...)` turns its Reference into a
        // plain function value, so the following `?.()` would call it without
        // `object` as `this`. Leave receiver-bound calls native.
        if self.track_optional_chain
            && call.optional
            && call.callee.get_member_expr().is_none()
            && !self.in_ignored_subtree()
        {
            self.wrap_optional_chain_link(&mut call.callee, call.span, ctx);
        }
    }

    fn enter_assignment_pattern(
        &mut self,
        pattern: &mut AssignmentPattern<'a>,
        ctx: &mut TraverseCtx<'a, CoverageState>,
    ) {
        self.instrument_destructuring_default(pattern, ctx);
    }

    fn exit_assignment_pattern(
        &mut self,
        _pattern: &mut AssignmentPattern<'a>,
        _ctx: &mut TraverseCtx<'a, CoverageState>,
    ) {
        self.ignored_expr_stack.pop();
    }

    fn enter_assignment_target_with_default(
        &mut self,
        target: &mut AssignmentTargetWithDefault<'a>,
        ctx: &mut TraverseCtx<'a, CoverageState>,
    ) {
        self.instrument_assignment_target_default(target, ctx);
    }

    fn exit_assignment_target_with_default(
        &mut self,
        _target: &mut AssignmentTargetWithDefault<'a>,
        _ctx: &mut TraverseCtx<'a, CoverageState>,
    ) {
        self.ignored_expr_stack.pop();
    }

    fn enter_assignment_target_property_identifier(
        &mut self,
        target: &mut AssignmentTargetPropertyIdentifier<'a>,
        ctx: &mut TraverseCtx<'a, CoverageState>,
    ) {
        self.instrument_shorthand_target_default(target, ctx);
    }

    fn enter_assignment_expression(
        &mut self,
        expr: &mut AssignmentExpression<'a>,
        ctx: &mut TraverseCtx<'a, CoverageState>,
    ) {
        if self.in_ignored_subtree() {
            return;
        }

        self.infer_name_for_value(&expr.right, assignment_target_name(expr));
        self.try_instrument_logical_assignment(expr, ctx);
    }
}
