//! Name inference for `fnMap` entries, from declarations, bindings, property
//! keys, assignment targets and call arguments.

use oxc_ast::ast::*;
use oxc_span::GetSpan;
use oxc_traverse::TraverseCtx;

use super::{CoverageState, CoverageTransform};

impl<'arena> CoverageTransform<'_, 'arena> {
    /// Record `name` for the function or arrow that is `value`, through
    /// parentheses. Any other value gets no name.
    ///
    /// The name is keyed on the start of the function node, so only that node
    /// can take it. A function in a decorator, a computed key, a member object,
    /// a class body or a later statement has a different start.
    pub(super) fn infer_name_for_value(&mut self, value: &Expression<'_>, name: Option<String>) {
        let Some(name) = name else { return };
        if self.istanbul_compat {
            return;
        }
        let value = value.without_parentheses();
        if matches!(
            value,
            Expression::FunctionExpression(_) | Expression::ArrowFunctionExpression(_)
        ) {
            self.inferred_names.insert(value.span().start, name);
        }
    }

    /// Take the name recorded for the function or arrow that starts at `start`.
    pub(super) fn take_inferred_name(&mut self, start: u32) -> Option<String> {
        self.inferred_names.remove(&start)
    }

    pub(super) fn resolve_function_name(
        &mut self,
        func: &Function,
        ctx: &TraverseCtx<'arena, CoverageState>,
    ) -> String {
        // An own name comes first, as for `Function.prototype.name`:
        // `const f = function g() {}` gives `g`.
        let inferred_name = self.take_inferred_name(func.span.start);
        if let Some(id) = &func.id {
            return id.name.to_string();
        }
        if let Some(name) = inferred_name {
            return name;
        }
        if !self.istanbul_compat
            && self.name_callback_arguments
            && let Some(name) = callback_argument_name(ctx)
        {
            return name;
        }
        format!("(anonymous_{})", self.fn_map.len())
    }
}

/// Derive a name for an otherwise-anonymous function or arrow that is a direct
/// argument of a call or `new` expression, taken from the callee:
/// `arr.map(cb)` -> `"map"`, `foo(cb)` -> `"foo"`,
/// `new Promise(cb)` -> `"Promise"`. Istanbul leaves these `(anonymous_N)`, so
/// this only runs under `name_callback_arguments`.
///
/// The immediate ancestor must be the call or `new` *arguments* position, so an
/// IIFE callee (`(() => {})()`) or a function returned from another function
/// stays anonymous. Only the callee is read: the argument-position ancestor
/// exposes no sibling arguments, so a leading string literal (route path, test
/// name) is not reachable from here.
pub(super) fn callback_argument_name(ctx: &TraverseCtx<'_, CoverageState>) -> Option<String> {
    use oxc_traverse::Ancestor;
    // Oxc keeps `ParenthesizedExpression` as a real node, so `foo((cb))` nests
    // the callback inside a paren whose parent is the arguments position. Any
    // other ancestor, a callee position for an IIFE among them, means this is
    // not a callback.
    let mut ancestors = ctx.ancestors();
    let callee = loop {
        match ancestors.next()? {
            Ancestor::ParenthesizedExpressionExpression(_) => {}
            Ancestor::CallExpressionArguments(call) => break call.callee(),
            Ancestor::NewExpressionArguments(new_expr) => break new_expr.callee(),
            _ => return None,
        }
    };
    callee_name(callee)
}

/// Extract a display name from a call or `new` callee. Follows the same
/// binding-name rules used elsewhere in this module: a bare identifier keeps
/// its name, a member access uses the property name, and a computed access
/// uses a string-literal key. `(foo)(cb)` unwraps to `foo`, since Oxc keeps
/// the parens Babel strips. A computed non-string index or a call result
/// yields no name.
fn callee_name(callee: &Expression<'_>) -> Option<String> {
    match callee {
        Expression::Identifier(ident) => Some(ident.name.to_string()),
        Expression::StaticMemberExpression(member) => Some(member.property.name.to_string()),
        Expression::ComputedMemberExpression(member) => match &member.expression {
            Expression::StringLiteral(lit) => Some(lit.value.display().to_string()),
            _ => None,
        },
        Expression::ParenthesizedExpression(paren) => callee_name(&paren.expression),
        _ => None,
    }
}

/// Derive a name from a `PropertyKey` that is a literal identifier, string,
/// number or no-substitution template. A genuinely computed key
/// (`[Symbol.iterator]`, `['m' + 1]`) yields `None`, which falls back to the
/// `(anonymous_N)` placeholder as istanbul-lib-instrument does for non-static
/// keys.
pub(super) fn property_key_to_name(key: &PropertyKey<'_>) -> Option<String> {
    match key {
        PropertyKey::StaticIdentifier(id) => Some(id.name.to_string()),
        PropertyKey::PrivateIdentifier(id) => Some(format!("#{}", id.name)),
        PropertyKey::StringLiteral(s) => Some(s.value.display().to_string()),
        PropertyKey::NumericLiteral(n) => {
            Some(n.raw.map_or_else(|| n.value.to_string(), |raw| raw.to_string()))
        }
        PropertyKey::TemplateLiteral(t) if t.expressions.is_empty() => t
            .quasis
            .first()
            .and_then(|quasi| quasi.value.cooked)
            .map(|cooked| cooked.display().to_string()),
        _ => None,
    }
}

/// The name an assignment gives to a function on its right side.
///
/// `o.foo = function () {}`, `o['bar'] = () => {}` and `x = () => {}` carry the
/// target name into the function, as JavaScript NamedEvaluation does for a
/// plain identifier. A compound or logical operator gives no name.
pub(super) fn assignment_target_name(expr: &AssignmentExpression<'_>) -> Option<String> {
    use oxc_syntax::operator::AssignmentOperator;

    if !matches!(expr.operator, AssignmentOperator::Assign) {
        return None;
    }
    match &expr.left {
        AssignmentTarget::AssignmentTargetIdentifier(id) => Some(id.name.to_string()),
        AssignmentTarget::StaticMemberExpression(member) => Some(member.property.name.to_string()),
        AssignmentTarget::ComputedMemberExpression(member) => match &member.expression {
            Expression::StringLiteral(lit) => Some(lit.value.display().to_string()),
            _ => None,
        },
        _ => None,
    }
}

/// The binding name of a simple assignment target, for a destructuring default
/// such as `[a = () => 1] = []`.
pub(super) fn simple_target_name(target: &AssignmentTarget<'_>) -> Option<String> {
    match target {
        AssignmentTarget::AssignmentTargetIdentifier(id) => Some(id.name.to_string()),
        _ => None,
    }
}

pub(super) fn method_label(kind: MethodDefinitionKind, name: String) -> String {
    match kind {
        MethodDefinitionKind::Get => format!("get {name}"),
        MethodDefinitionKind::Set => format!("set {name}"),
        MethodDefinitionKind::Method | MethodDefinitionKind::Constructor => name,
    }
}
