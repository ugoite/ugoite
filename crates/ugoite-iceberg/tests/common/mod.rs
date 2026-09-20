#![allow(dead_code, unused_imports)]

use anyhow::{Context, Result};
use opendal::services::Memory;
use opendal::Operator;
use serde_json::Value;
use std::collections::BTreeMap;
use ugoite_core::entry::{self as core_entry, StructuredEntryDraft};
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

/// Test-only adapter for fixtures that historically used whole-Entry Markdown
/// to seed the storage layer. Product mutation APIs are structured-only; these
/// helpers keep older low-level tests focused on the behavior they cover while
/// constructing the same structured draft used by production.
fn structured_draft(markdown: &str) -> Result<StructuredEntryDraft> {
    let conversion = core_entry::legacy_markdown_to_draft(markdown, "");
    if !conversion.diagnostics.is_empty() {
        return Err(core_entry::markdown_conversion_error(&conversion.diagnostics).into());
    }
    Ok(conversion.draft)
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
        Some(draft.title),
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
        Some(draft.title),
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
            Some(draft.title),
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
            Some(draft.title),
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
            Some(draft.title),
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
            Some(draft.title),
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
