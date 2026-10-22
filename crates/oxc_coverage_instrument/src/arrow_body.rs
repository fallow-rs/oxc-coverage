//! Concise arrow body expansion.
//!
//! A concise arrow (`(x) => x * 2`) has no block for a coverage counter to
//! live in. Up to oxc 0.142 the parser produced one anyway, a `FunctionBody`
//! holding a single `ExpressionStatement`, both spanning the expression;
//! 0.143 models the body as a bare `Expression` instead, which traverse walks
//! as an expression, so `enter_function_body`, `enter_statement` and
//! `exit_statements` never fire for it. Coverage needs the 0.142 shape back:
//! the block is where `++cov.f[N]` goes, and the `ExpressionStatement` is what
//! earns the arrow's `statementMap` entry.
//!
//! [`wrap_concise_arrow_body`] rebuilds that shape. It runs from two places:
//!
//! - during the coverage traverse, from `enter_arrow_function_body`, on any
//!   body still concise when the arrow is entered;
//! - before the TypeScript strip pass, from
//!   [`pre_expand_concise_arrow_bodies`].
//!
//! The early pass exists because the strip pass rewrites the body of
//! `(x: number) => x as unknown as string` down to `x`, keeping only the
//! inner expression's span. Wrapping after that would record the stripped
//! span as the arrow's `fnMap` `loc` and as its statement entry, shrinking
//! both. Wrapping before it puts the span in a block the strip pass does not
//! touch, exactly as the 0.142 parser did.
//!
//! Wrapping is only half of the shape change. The block holds an
//! `ExpressionStatement`, and the `return` that keeps the arrow yielding its
//! value is formed on the way out of the coverage traverse. So the early pass
//! may only run when that traverse will follow: wrapped without it,
//! `(x) => x * 2` is emitted as `(x) => { x * 2; }` and returns `undefined`.

use std::collections::BTreeSet;

use oxc_allocator::{Allocator, ReplaceWith};
use oxc_ast::{
    ast::{ArrowFunctionBody, ArrowFunctionExpression, Program, Statement},
    builder::{AstBuilder, GetAstBuilder},
};
use oxc_ast_visit::{VisitMut, walk_mut::walk_arrow_function_expression};
use oxc_span::GetSpan;

/// Give a concise arrow body the block wrapper the 0.142 parser produced: a
/// `FunctionBody` holding one `ExpressionStatement`, both carrying the
/// expression's own span, so `statementMap` and the source map are unchanged.
/// Parentheses around the expression are left out of that span: Babel drops
/// them from its AST, so istanbul records `() => (a, b)` as `a, b`.
///
/// Returns whether the body was concise, and so was wrapped. The wrapper must
/// stay an `ExpressionStatement`: the `return` that keeps the arrow yielding
/// its value is only formed on the way out of the traverse, with a synthetic
/// span that `is_synthetic_span` keeps out of the coverage map.
pub fn wrap_concise_arrow_body<'arena>(
    body: &mut ArrowFunctionBody<'arena>,
    builder: &impl GetAstBuilder<'arena>,
) -> bool {
    let Some(expr) = body.as_expression() else {
        return false;
    };
    let span = expr.without_parentheses().span();
    body.replace_with(|expr| {
        let stmt = Statement::new_expression_statement(span, expr.into_expression(), builder);
        ArrowFunctionBody::new_function_body(span, [], [stmt], builder)
    });
    true
}

/// The arrows [`pre_expand_concise_arrow_bodies`] wrapped, identified by
/// `ArrowFunctionExpression::span.start`.
///
/// The transform cannot re-derive this: once wrapped, a concise body is
/// indistinguishable from a block the source wrote, and 0.143 has no
/// `ArrowFunctionExpression::expression` flag left to consult. It still has to
/// know, because only a wrapped body gets its expression turned back into a
/// `return` on the way out.
///
/// Empty for every path that does not pre-expand, where the traverse hook
/// still sees a bare expression and answers the question itself.
#[derive(Debug, Default)]
pub struct PreExpandedArrows(BTreeSet<u32>);

impl PreExpandedArrows {
    /// Whether the arrow starting at `arrow_start` was wrapped by the
    /// pre-expansion pass.
    pub fn contains(&self, arrow_start: u32) -> bool {
        self.0.contains(&arrow_start)
    }

    /// Whether nothing was wrapped, which every path that skips pre-expansion
    /// reports. Callers that bypass the coverage traverse assert on it: a
    /// wrapped body with no traverse behind it never gets its `return`.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Wrap every concise arrow body in `program`, before a pass that rewrites
/// spans can reach it.
pub fn pre_expand_concise_arrow_bodies<'arena>(
    allocator: &'arena Allocator,
    program: &mut Program<'arena>,
) -> PreExpandedArrows {
    let mut expander =
        ConciseArrowBodyExpander { builder: AstBuilder::new(allocator), wrapped: BTreeSet::new() };
    expander.visit_program(program);
    PreExpandedArrows(expander.wrapped)
}

/// Visitor behind [`pre_expand_concise_arrow_bodies`].
struct ConciseArrowBodyExpander<'arena> {
    builder: AstBuilder<'arena>,
    wrapped: BTreeSet<u32>,
}

impl<'arena> VisitMut<'arena> for ConciseArrowBodyExpander<'arena> {
    fn visit_arrow_function_expression(&mut self, arrow: &mut ArrowFunctionExpression<'arena>) {
        if wrap_concise_arrow_body(&mut arrow.body, &self.builder) {
            self.wrapped.insert(arrow.span.start);
        }
        // The walk re-reads `body` below, so a body wrapped here is descended
        // into as a block and any nested arrow is wrapped in turn.
        walk_arrow_function_expression(self, arrow);
    }
}
