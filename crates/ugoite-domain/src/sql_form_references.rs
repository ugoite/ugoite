//! Scope-aware classification of quoted Form-name relation references.
//!
//! This syntax helper is shared by server binding and local editor adapters. It
//! does not resolve identity or grant authorization; those remain server duties.

use sqlparser_datafusion::ast::{
    Ident, ObjectName, ObjectNamePart, Statement, VisitMut, VisitorMut,
};
use sqlparser_datafusion::dialect::GenericDialect;
use sqlparser_datafusion::parser::Parser;
use std::collections::{BTreeMap, BTreeSet};
use std::ops::ControlFlow;

/// Relation names discovered while applying the authoritative CTE scope rules.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SqlFormNameReferences {
    /// Quoted SQL Form names (legacy opaque relations omitted).
    pub form_names: BTreeSet<String>,
    /// Every quoted single-part relation, including legacy opaque relations.
    pub quoted_relation_names: BTreeSet<String>,
    /// Alias keys rewritten by [`resolve_form_name_references_in_statements`].
    pub used_aliases: BTreeSet<String>,
    /// Whether any relation was rewritten.
    pub changed: bool,
}

pub fn is_legacy_sql_relation_name(name: &str) -> bool {
    let Some((prefix, id)) = name.split_at_checked("form_".len()) else {
        return false;
    };
    prefix.eq_ignore_ascii_case("form_")
        && id.len() == 32
        && id.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Traverse parsed SQL statements with the same nested CTE visibility used by
/// server binding. If `aliases` is non-empty, matching quoted Form names are
/// rewritten to their stable relations after collecting their reference names.
pub fn resolve_form_name_references_in_statements<'a>(
    statements: impl IntoIterator<Item = &'a mut Statement>,
    aliases: &BTreeMap<String, String>,
) -> Result<SqlFormNameReferences, String> {
    struct Resolver<'a> {
        aliases: &'a BTreeMap<String, String>,
        cte_scopes: Vec<BTreeSet<String>>,
        error: Option<String>,
        result: SqlFormNameReferences,
    }

    impl VisitorMut for Resolver<'_> {
        type Break = ();

        fn pre_visit_query(
            &mut self,
            query: &mut sqlparser_datafusion::ast::Query,
        ) -> ControlFlow<()> {
            self.cte_scopes.push(
                query
                    .with
                    .as_ref()
                    .into_iter()
                    .flat_map(|with| with.cte_tables.iter())
                    .map(|cte| {
                        let identifier = &cte.alias.name;
                        if identifier.quote_style.is_some() {
                            identifier.value.clone()
                        } else {
                            identifier.value.to_ascii_lowercase()
                        }
                    })
                    .collect(),
            );
            ControlFlow::Continue(())
        }

        fn post_visit_query(
            &mut self,
            _query: &mut sqlparser_datafusion::ast::Query,
        ) -> ControlFlow<()> {
            self.cte_scopes.pop();
            ControlFlow::Continue(())
        }

        fn pre_visit_relation(&mut self, relation: &mut ObjectName) -> ControlFlow<()> {
            let Some(identifier) = relation.0.last().and_then(|part| part.as_ident()) else {
                return ControlFlow::Continue(());
            };
            let name = identifier.value.as_str();
            let normalized_name = if identifier.quote_style.is_some() {
                name.to_string()
            } else {
                name.to_ascii_lowercase()
            };
            if relation.0.len() == 1
                && self
                    .cte_scopes
                    .iter()
                    .rev()
                    .any(|scope| scope.contains(&normalized_name))
            {
                return ControlFlow::Continue(());
            }
            if relation.0.len() == 1 && identifier.quote_style == Some('"') {
                self.result.quoted_relation_names.insert(name.to_string());
                if !is_legacy_sql_relation_name(name) {
                    self.result.form_names.insert(name.to_string());
                }
            }
            let case_match = self
                .aliases
                .keys()
                .any(|alias| alias.eq_ignore_ascii_case(name));
            if !case_match {
                return ControlFlow::Continue(());
            }
            if relation.0.len() != 1
                || identifier.quote_style != Some('"')
                || !self.aliases.contains_key(name)
            {
                self.error = Some(format!(
                    "Form name {name} must be referenced as a double-quoted SQL relation"
                ));
                return ControlFlow::Break(());
            }
            let resolved = self.aliases.get(name).expect("checked alias above");
            self.result.used_aliases.insert(name.to_string());
            relation.0 = vec![ObjectNamePart::Identifier(Ident::new(resolved))];
            self.result.changed = true;
            ControlFlow::Continue(())
        }
    }

    let mut resolver = Resolver {
        aliases,
        cte_scopes: Vec::new(),
        error: None,
        result: SqlFormNameReferences::default(),
    };
    for statement in statements {
        if statement.visit(&mut resolver).is_break() {
            break;
        }
    }
    match resolver.error {
        Some(error) => Err(error),
        None => Ok(resolver.result),
    }
}

/// Classify quoted Form-name table references using the shared Rust CTE-scope
/// visitor. Parse errors are returned so callers can preserve guards fail-closed.
pub fn collect_quoted_form_name_references(sql: &str) -> Result<BTreeSet<String>, String> {
    let dialect = GenericDialect {};
    let mut statements = Parser::new(&dialect)
        .with_recursion_limit(256)
        .try_with_sql(sql)
        .map_err(|error| error.to_string())?
        .parse_statements()
        .map_err(|error| error.to_string())?;
    let result = resolve_form_name_references_in_statements(&mut statements, &BTreeMap::new())?;
    Ok(result.form_names)
}

#[cfg(test)]
mod tests {
    use super::collect_quoted_form_name_references;
    use std::collections::BTreeSet;

    #[test]
    fn excludes_cte_shadowed_form_names() {
        let references = collect_quoted_form_name_references(
            "WITH \"Expenses\" AS (SELECT 1) SELECT * FROM \"Expenses\"",
        )
        .unwrap();
        assert!(references.is_empty());
    }

    #[test]
    fn respects_nested_cte_scopes_and_keeps_outer_form_references() {
        let nested_only = collect_quoted_form_name_references(
            "SELECT * FROM (WITH \"Expenses\" AS (SELECT 1) SELECT * FROM \"Expenses\") AS nested",
        )
        .unwrap();
        assert!(nested_only.is_empty());

        let mixed = collect_quoted_form_name_references(
            "SELECT * FROM (WITH \"Expenses\" AS (SELECT 1) SELECT * FROM \"Expenses\") AS nested JOIN \"Expenses\" AS actual ON TRUE",
        )
        .unwrap();
        assert_eq!(mixed, BTreeSet::from(["Expenses".to_string()]));
    }

    #[test]
    fn invalid_sql_fails_closed_for_guard_pruning() {
        assert!(collect_quoted_form_name_references("SELECT (").is_err());
    }
}
