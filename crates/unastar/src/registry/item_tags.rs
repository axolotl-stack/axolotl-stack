//! Item tag expressions: the Molang `tags` form of item descriptors
//! (`q.all_tags('minecraft:is_spear')`, `q.any_tag(...)`), resolved once
//! against the item registry into the items they match.

use thiserror::Error;
use unastar_rules::molang::eval::{self, MolangContext, Value};
use unastar_rules::molang::{self, BuiltinQuery, Expr, QueryContext, QueryId, QueryRegistry};

use super::ItemRegistry;
use super::item::ItemEntry;

/// Why a tag expression cannot be resolved to items.
#[derive(Debug, Error)]
pub enum TagExpressionError {
    #[error("Molang: {0}")]
    Molang(String),
    #[error("query.{0} needs an item-context binding")]
    Query(String),
    #[error("tag queries require string tag arguments")]
    TagArgument,
    #[error("random expressions cannot be frozen to item predicates")]
    Random,
    #[error("expression requires unavailable mutable/entity context")]
    Context,
}

/// The items `source` is true for. Only pure item-tag expressions resolve:
/// any other query, randomness or entity context is an error, never
/// silently false.
pub fn items_matching<'a>(
    source: &str,
    items: &'a ItemRegistry,
) -> Result<Vec<&'a ItemEntry>, TagExpressionError> {
    let molang_error =
        |reason: &dyn std::fmt::Display| TagExpressionError::Molang(reason.to_string());
    let mut expression = molang::parse(source).map_err(|reason| molang_error(&reason))?;
    let mut unsupported = None;
    expression.walk(&mut |node| match node {
        Expr::Query(query, args) => {
            if !matches!(query.name.as_ref(), "any_tag" | "all_tags") {
                unsupported = Some(TagExpressionError::Query(query.name.to_string()));
            } else if !args.iter().all(|arg| matches!(arg, Expr::Str(_))) {
                unsupported = Some(TagExpressionError::TagArgument);
            }
        }
        Expr::Math(function, _) if function.is_random() => {
            unsupported = Some(TagExpressionError::Random);
        }
        Expr::This
        | Expr::Var(..)
        | Expr::Assign(..)
        | Expr::Arrow(..)
        | Expr::Loop(..)
        | Expr::Break
        | Expr::Continue => {
            unsupported = Some(TagExpressionError::Context);
        }
        _ => {}
    });
    if let Some(error) = unsupported {
        return Err(error);
    }
    expression
        .bind(QueryRegistry::vanilla(), QueryContext::Runtime)
        .map_err(|reason| molang_error(&reason))?;
    items
        .iter()
        .filter_map(
            |item| match eval::evaluate(&expression, &mut ItemTagContext(item)) {
                Ok(value) if value.truthy() => Some(Ok(item)),
                Ok(_) => None,
                Err(error) => Some(Err(molang_error(&error))),
            },
        )
        .collect()
}

/// Used only at resolution, after the AST rejects every non-item binding.
struct ItemTagContext<'a>(&'a ItemEntry);

impl MolangContext for ItemTagContext<'_> {
    fn query(&mut self, id: QueryId, args: &[Value]) -> Option<Value> {
        let has_tag = |value: &Value| match value {
            Value::Str(tag) => self.0.has_tag(tag),
            _ => unreachable!("tag query arguments validated as string literals"),
        };
        let result = match BuiltinQuery::from_id(id) {
            Some(BuiltinQuery::AllTags) => args.iter().all(has_tag),
            Some(BuiltinQuery::AnyTag) => args.iter().any(has_tag),
            _ => unreachable!("tag query set validated before evaluation"),
        };
        Some(Value::Num(if result { 1.0 } else { 0.0 }))
    }

    fn variable(&self, _: &str) -> Option<Value> {
        unreachable!("item predicate has no variables")
    }
    fn set_variable(&mut self, _: &str, _: Value) {
        unreachable!("item predicate has no assignments")
    }
    fn context(&self, _: &str) -> Option<Value> {
        unreachable!("item predicate has no context variables")
    }
    fn this(&self) -> Value {
        unreachable!("item predicate has no entity")
    }
    fn enter(&mut self, _: u64) -> bool {
        unreachable!("item predicate has no entity arrow")
    }
    fn leave(&mut self) {
        unreachable!("item predicate has no entity arrow")
    }
    fn random(&mut self) -> f32 {
        unreachable!("item predicate has no random operations")
    }
}
