#![allow(dead_code, unused_imports)]

use anyhow::{Context, Result};
use opendal::services::Memory;
use opendal::Operator;
use serde_json::Value;
use std::collections::BTreeMap;
use ugoite_core::entry::StructuredEntryDraft;
use ugoite_domain::change::ChangeCommand;
use ugoite_iceberg::entry;
use ugoite_iceberg::integrity::IntegrityProvider;
use ugoite_iceberg::service::UgoiteService;
use uuid::Uuid;

#[allow(dead_code)]
pub fn setup_operator() -> Result<Operator> {
    let builder = Memory::default();
    let op = Operator::new(builder)?;
    Ok(op)
}

/// Test-only fixture parser. Product mutation APIs are structured-only; these
/// helpers keep low-level storage tests readable without restoring a Markdown
/// mutation adapter to production.
fn structured_draft(markdown: &str) -> Result<StructuredEntryDraft> {
    let (frontmatter, body) = if let Some(rest) = markdown.strip_prefix("---\n") {
        let (yaml, body) = rest
            .split_once("\n---\n")
            .context("fixture frontmatter is not closed")?;
        (
            serde_yaml::from_str::<serde_json::Value>(yaml)
                .context("fixture frontmatter is invalid")?,
            body,
        )
    } else {
        (serde_json::json!({}), markdown)
    };
    let form_name = frontmatter
        .get("form")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let tags = frontmatter
        .get("tags")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    let mut fields = BTreeMap::new();
    if let Some(values) = frontmatter.as_object() {
        for (key, value) in values {
            if key != "form" && key != "tags" {
                fields.insert(key.clone(), value.clone());
            }
        }
    }
    let mut current: Option<String> = None;
    let mut lines = Vec::new();
    let flush = |current: &mut Option<String>,
                 lines: &mut Vec<String>,
                 fields: &mut BTreeMap<String, Value>| {
        if let Some(key) = current.take() {
            fields.insert(key, Value::String(lines.join("\n").trim().to_string()));
        }
        lines.clear();
    };
    for line in body.lines() {
        if let Some(key) = line.strip_prefix("## ") {
            flush(&mut current, &mut lines, &mut fields);
            current = Some(key.trim().to_string());
        } else if current.is_some() {
            lines.push(line.to_string());
        }
    }
    flush(&mut current, &mut lines, &mut fields);
    Ok(StructuredEntryDraft {
        form_name,
        tags,
        fields,
        extra_attributes: BTreeMap::new(),
    })
}

pub async fn legacy_create_entry<I: IntegrityProvider>(
    op: &Operator,
    ws_path: &str,
    entry_id: &str,
    markdown: &str,
    author: &str,
    integrity: &I,
) -> Result<entry::EntryMeta> {
    let entry = entry::create_draft_entries_with_scopes_and_change(
        op,
        ws_path,
        vec![entry::EntryDraftRequest {
            entry_id: entry_id.to_string(),
            draft: structured_draft(markdown)?,
        }],
        author,
        integrity,
        None,
        None,
    )
    .await?
    .pop()
    .context("a one-entry test create must return one Entry")?;
    Ok(entry)
}

#[allow(clippy::too_many_arguments)]
pub async fn legacy_create_entry_with_scopes_and_change<I: IntegrityProvider>(
    op: &Operator,
    ws_path: &str,
    entry_id: &str,
    markdown: &str,
    author: &str,
    integrity: &I,
    relation_scopes: Option<&BTreeMap<String, ugoite_core::query::EntryScope>>,
    change: Option<ChangeCommand>,
) -> Result<entry::EntryMeta> {
    entry::create_draft_entries_with_scopes_and_change(
        op,
        ws_path,
        vec![entry::EntryDraftRequest {
            entry_id: entry_id.to_string(),
            draft: structured_draft(markdown)?,
        }],
        author,
        integrity,
        relation_scopes,
        change,
    )
    .await?
    .pop()
    .context("a one-entry test create must return one Entry")
}

pub async fn legacy_create_entry_with_scopes<I: IntegrityProvider>(
    op: &Operator,
    ws_path: &str,
    entry_id: &str,
    markdown: &str,
    author: &str,
    integrity: &I,
    relation_scopes: Option<&BTreeMap<String, ugoite_core::query::EntryScope>>,
) -> Result<entry::EntryMeta> {
    legacy_create_entry_with_scopes_and_change(
        op,
        ws_path,
        entry_id,
        markdown,
        author,
        integrity,
        relation_scopes,
        None,
    )
    .await
}

pub async fn legacy_update_entry<I: IntegrityProvider>(
    op: &Operator,
    ws_path: &str,
    entry_id: &str,
    markdown: &str,
    parent_revision_id: Option<&str>,
    author: &str,
    integrity: &I,
) -> Result<Value> {
    let draft = structured_draft(markdown)?;
    entry::update_structured_entry_authorized_with_change(
        op,
        ws_path,
        entry_id,
        draft.form_name,
        Some(draft.tags),
        draft.fields,
        draft.extra_attributes,
        parent_revision_id,
        author,
        integrity,
        None,
        None,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn legacy_update_entry_authorized_with_change<I: IntegrityProvider>(
    op: &Operator,
    ws_path: &str,
    entry_id: &str,
    markdown: &str,
    parent_revision_id: Option<&str>,
    author: &str,
    integrity: &I,
    relation_scopes: Option<&BTreeMap<String, ugoite_core::query::EntryScope>>,
    change: Option<ChangeCommand>,
) -> Result<Value> {
    let draft = structured_draft(markdown)?;
    entry::update_structured_entry_authorized_with_change(
        op,
        ws_path,
        entry_id,
        draft.form_name,
        Some(draft.tags),
        draft.fields,
        draft.extra_attributes,
        parent_revision_id,
        author,
        integrity,
        relation_scopes,
        change,
    )
    .await
}

#[derive(Debug, Clone)]
pub struct LegacyEntryCreateRequest {
    pub entry_id: String,
    pub content: String,
}

impl LegacyEntryCreateRequest {
    pub fn new(entry_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            entry_id: entry_id.into(),
            content: content.into(),
        }
    }
}

pub async fn legacy_create_entries<I: IntegrityProvider>(
    op: &Operator,
    ws_path: &str,
    requests: Vec<LegacyEntryCreateRequest>,
    author: &str,
    integrity: &I,
) -> Result<Vec<entry::EntryMeta>> {
    let drafts = requests
        .into_iter()
        .map(|request| {
            Ok(entry::EntryDraftRequest {
                entry_id: request.entry_id,
                draft: structured_draft(&request.content)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    entry::create_draft_entries_with_scopes_and_change(
        op, ws_path, drafts, author, integrity, None, None,
    )
    .await
}

/// Re-export the storage entry API for legacy fixture tests while replacing
/// only the removed whole-document mutation functions with structured test
/// adapters above.
pub mod legacy_entry {
    pub use super::{
        legacy_create_entries as create_entries, legacy_create_entry as create_entry,
        legacy_create_entry_with_scopes as create_entry_with_scopes,
        legacy_create_entry_with_scopes_and_change as create_entry_with_scopes_and_change,
        legacy_update_entry as update_entry,
        legacy_update_entry_authorized_with_change as update_entry_authorized_with_change,
        LegacyEntryCreateRequest as EntryCreateRequest,
    };
    pub use ugoite_iceberg::entry::*;
}

#[async_trait::async_trait]
pub trait LegacyServiceEntryExt {
    async fn create_entry(
        &self,
        space_id: &str,
        entry_id: &str,
        markdown: &str,
        author: &str,
    ) -> Result<Value>;

    async fn create_entry_authorized_for_principals(
        &self,
        space_id: &str,
        entry_id: &str,
        markdown: &str,
        author: &str,
        principal_ids: &[Uuid],
    ) -> Result<Value>;

    async fn update_entry(
        &self,
        space_id: &str,
        entry_id: &str,
        markdown: &str,
        parent_revision_id: Option<&str>,
        author: &str,
    ) -> Result<Value>;

    async fn update_entry_authorized_for_principals(
        &self,
        space_id: &str,
        entry_id: &str,
        markdown: &str,
        parent_revision_id: Option<&str>,
        author: &str,
        principal_ids: &[Uuid],
    ) -> Result<Value>;
}

#[async_trait::async_trait]
impl LegacyServiceEntryExt for UgoiteService {
    async fn create_entry(
        &self,
        space_id: &str,
        entry_id: &str,
        markdown: &str,
        author: &str,
    ) -> Result<Value> {
        let draft = structured_draft(markdown)?;
        self.create_structured_entry_with_receipt(
            space_id,
            entry_id,
            draft.form_name.context("test Entry form is missing")?,
            draft.tags,
            draft.fields,
            draft.extra_attributes,
            author,
        )
        .await
        .map(|(value, _)| value)
    }

    async fn create_entry_authorized_for_principals(
        &self,
        space_id: &str,
        entry_id: &str,
        markdown: &str,
        author: &str,
        principal_ids: &[Uuid],
    ) -> Result<Value> {
        let draft = structured_draft(markdown)?;
        self.create_structured_entry_authorized_for_principals(
            space_id,
            entry_id,
            draft.form_name.context("test Entry form is missing")?,
            draft.tags,
            draft.fields,
            draft.extra_attributes,
            author,
            principal_ids,
        )
        .await
    }

    async fn update_entry(
        &self,
        space_id: &str,
        entry_id: &str,
        markdown: &str,
        parent_revision_id: Option<&str>,
        author: &str,
    ) -> Result<Value> {
        let draft = structured_draft(markdown)?;
        self.update_structured_entry(
            space_id,
            entry_id,
            draft.form_name,
            draft.fields,
            draft.extra_attributes,
            parent_revision_id,
            author,
        )
        .await
    }

    async fn update_entry_authorized_for_principals(
        &self,
        space_id: &str,
        entry_id: &str,
        markdown: &str,
        parent_revision_id: Option<&str>,
        author: &str,
        principal_ids: &[Uuid],
    ) -> Result<Value> {
        let draft = structured_draft(markdown)?;
        self.update_structured_entry_authorized_for_principals(
            space_id,
            entry_id,
            draft.form_name,
            Some(draft.tags),
            draft.fields,
            draft.extra_attributes,
            parent_revision_id,
            author,
            principal_ids,
        )
        .await
    }
}
