use anyhow::Result;
use opendal::Operator;
use serde_json::Value;
use std::collections::BTreeMap;
use ugoite_core::query::EntryScope;

use crate::entry;
pub use ugoite_domain::search::KeywordSearchResult;

/// Keyword search over one bounded, authorized current-state DataFusion
/// payload plan.
pub async fn search_entries(
    op: &Operator,
    ws_path: &str,
    query: &str,
    limit: usize,
) -> Result<Vec<KeywordSearchResult>> {
    let relation_scopes = entry::list_form_names(op, ws_path)
        .await?
        .into_iter()
        .map(|form_name| (form_name.to_ascii_lowercase(), EntryScope::AllCurrent))
        .collect();
    search_entries_with_scopes(op, ws_path, query, &relation_scopes, limit).await
}

/// Searches typed/system columns in one globally ordered DataFusion candidate
/// plan. Search intentionally excludes `extra_attributes` and opaque asset or
/// object-list structs; the searchable typed column set is defined by the
/// Form field type in the plan builder.
pub async fn search_entries_with_scopes(
    op: &Operator,
    ws_path: &str,
    query: &str,
    relation_scopes: &std::collections::BTreeMap<String, EntryScope>,
    limit: usize,
) -> Result<Vec<KeywordSearchResult>> {
    search_entries_with_scopes_after(op, ws_path, query, relation_scopes, limit, None).await
}

pub async fn search_entries_with_scopes_after(
    op: &Operator,
    ws_path: &str,
    query: &str,
    relation_scopes: &std::collections::BTreeMap<String, EntryScope>,
    limit: usize,
    after: Option<(&str, &str, &str)>,
) -> Result<Vec<KeywordSearchResult>> {
    let candidates = crate::index::query_entry_candidates_authorized_after(
        op,
        ws_path,
        relation_scopes,
        None,
        Some(query),
        limit,
        after,
    )
    .await?;
    let mut results = BTreeMap::new();
    for candidate in candidates {
        let result = KeywordSearchResult {
            id: candidate.entry_id,
            title: candidate.title,
            form: candidate.form_name,
            created_at: candidate.created_at,
            updated_at: candidate.updated_at,
        };
        results.insert((result.form.clone(), result.id.clone()), result);
    }

    // AssetText is a trusted internal projection. The authorized current
    // Entry scan remains the ACL boundary: an asset match is promoted only
    // after its current Entry has passed the caller's Form/Entry scope.
    // Derived failures intentionally degrade to native search.
    if !query.trim().is_empty() {
        if let Ok(Some(matching_assets)) =
            crate::derived_relation::asset_text_search_matches(op, ws_path, query).await
        {
            if !matching_assets.is_empty() {
                if let Ok(asset_rows) = crate::index::query_entry_rows_authorized(
                    op,
                    ws_path,
                    relation_scopes,
                    None,
                    None,
                    crate::MAX_NORMAL_READ_ROWS,
                    0,
                )
                .await
                {
                    for (form_name, row) in asset_rows {
                        if row.deleted || !row_references_asset(&row.fields, &matching_assets) {
                            continue;
                        }
                        let result = KeywordSearchResult {
                            id: row.entry_id,
                            title: row.title,
                            form: form_name,
                            created_at: row.created_at,
                            updated_at: row.updated_at,
                        };
                        if is_after_cursor(&result, after) {
                            results.insert((result.form.clone(), result.id.clone()), result);
                        }
                    }
                }
            }
        }
    }

    let mut results = results.into_values().collect::<Vec<_>>();
    results.sort_by(|left, right| {
        left.title
            .cmp(&right.title)
            .then_with(|| left.id.cmp(&right.id))
            .then_with(|| left.form.cmp(&right.form))
    });
    results.truncate(limit);
    Ok(results)
}

fn is_after_cursor(
    result: &KeywordSearchResult,
    after: Option<(&str, &str, &str)>,
) -> bool {
    after.is_none_or(|(title, id, form)| {
        (result.title.as_str(), result.id.as_str(), result.form.as_str()) > (title, id, form)
    })
}

fn row_references_asset(
    value: &Value,
    matching_assets: &std::collections::HashSet<String>,
) -> bool {
    match value {
        Value::Array(values) => values
            .iter()
            .any(|value| row_references_asset(value, matching_assets)),
        Value::Object(object) => {
            object
                .get("asset_id")
                .and_then(Value::as_str)
                .is_some_and(|asset_id| matching_assets.contains(asset_id))
                || object
                    .values()
                    .any(|value| row_references_asset(value, matching_assets))
        }
        _ => false,
    }
}
