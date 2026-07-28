use anyhow::{anyhow, Result};
use opendal::Operator;
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};
use uuid::Uuid;

use crate::integrity::RealIntegrityProvider;
use crate::{
    asset,
    authorization::{Authorizer, ResourceKind, ResourceRef},
    entry, form, index, preferences, saved_sql, search, space, sql_session,
    storage::operator_from_uri,
};
use ugoite_core::error::AppError;
use ugoite_domain::id::{
    validate_asset_id, validate_entry_id, validate_form_name, validate_revision_id,
    validate_space_id, validate_sql_id, validate_sql_session_id,
};
use ugoite_domain::identity::Action;

pub const MEMBERSHIP_MANAGED_SPACE_SETTING_KEYS: &[&str] = &[
    "admin_user_ids",
    "invitations",
    "member_roles",
    "members",
    "member_invitations",
    "membership_version",
    "owner_user_id",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpacePermission {
    Read,
    WriteContent,
    ManageSpace,
    ManageMembers,
}

#[derive(Clone)]
pub struct UgoiteService {
    operator: Operator,
    root_uri: String,
}

impl UgoiteService {
    pub fn new(root_uri: impl Into<String>) -> Result<Self> {
        let root_uri = root_uri.into();
        let operator = operator_from_uri(&root_uri)?;
        Ok(Self { operator, root_uri })
    }

    pub fn from_operator(operator: Operator, root_uri: impl Into<String>) -> Self {
        Self {
            operator,
            root_uri: root_uri.into(),
        }
    }

    pub fn operator(&self) -> &Operator {
        &self.operator
    }

    pub fn root_uri(&self) -> &str {
        &self.root_uri
    }

    pub fn workspace_path(&self, space_id: &str) -> String {
        format!("spaces/{space_id}")
    }

    pub async fn create_space(&self, space_id: &str) -> Result<()> {
        validate_storage_id(validate_space_id(space_id))?;
        space::create_space(&self.operator, space_id, &self.root_uri).await
    }

    /// Creates an operator-local Space with an immutable UUIDv7 directory and
    /// no application principal. A node must explicitly claim it before remote use.
    pub async fn create_operator_space(&self, slug: &str) -> Result<Uuid> {
        validate_storage_id(validate_space_id(slug))?;
        if self.space_id_by_slug(slug).await?.is_some() {
            return Err(AppError::conflict(
                ugoite_core::error::ErrorCode::SpaceAlreadyExists,
                format!("Space slug already exists: {slug}"),
            )
            .into());
        }
        let space_id = Uuid::now_v7();
        space::create_space_with_identity(&self.operator, space_id, slug, &self.root_uri).await?;
        Ok(space_id)
    }

    pub async fn create_space_for_principal(
        &self,
        slug: &str,
        principal_id: Uuid,
        display_name: &str,
    ) -> Result<Uuid> {
        validate_storage_id(validate_space_id(slug))?;
        if self.space_id_by_slug(slug).await?.is_some() {
            return Err(AppError::conflict(
                ugoite_core::error::ErrorCode::SpaceAlreadyExists,
                format!("Space slug already exists: {slug}"),
            )
            .into());
        }
        let space_uid = Uuid::now_v7();
        let space_id = space_uid.to_string();
        space::create_space_with_identity(&self.operator, space_uid, slug, &self.root_uri).await?;
        Authorizer::new(self.operator.clone())
            .initialize_owner(&space_id, space_uid, principal_id, display_name)
            .await?;
        Ok(space_uid)
    }

    pub async fn space_uid(&self, space_id: &str) -> Result<Uuid> {
        let raw = self.get_space(space_id).await?;
        raw.get("space_uid")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("Space is missing immutable space_uid"))
            .and_then(|value| Uuid::parse_str(value).map_err(anyhow::Error::from))
    }

    pub async fn list_space_ids(&self) -> Result<Vec<String>> {
        space::list_spaces(&self.operator).await
    }

    pub async fn space_id_by_slug(&self, slug: &str) -> Result<Option<String>> {
        for space_id in self.list_space_ids().await? {
            let meta = self.get_space(&space_id).await?;
            if meta.get("slug").and_then(Value::as_str) == Some(slug) {
                return Ok(Some(space_id));
            }
        }
        Ok(None)
    }

    pub async fn get_space(&self, space_id: &str) -> Result<Value> {
        validate_storage_id(validate_space_id(space_id))?;
        space::get_space_raw(&self.operator, space_id).await
    }

    pub async fn patch_space(&self, space_id: &str, patch: &Value) -> Result<Value> {
        validate_storage_id(validate_space_id(space_id))?;
        validate_public_space_patch(patch)?;
        space::patch_space(&self.operator, space_id, patch).await
    }

    pub async fn ensure_space(&self, space_id: &str) -> Result<()> {
        validate_storage_id(validate_space_id(space_id))?;
        space::get_space(&self.operator, space_id).await.map(|_| ())
    }

    pub async fn list_forms(&self, space_id: &str) -> Result<Vec<Value>> {
        validate_storage_id(validate_space_id(space_id))?;
        form::list_forms(&self.operator, &self.workspace_path(space_id)).await
    }

    pub async fn get_form(&self, space_id: &str, form_name: &str) -> Result<Value> {
        validate_storage_id(validate_space_id(space_id))?;
        validate_storage_id(validate_form_name(form_name))?;
        form::get_form(&self.operator, &self.workspace_path(space_id), form_name).await
    }

    pub async fn upsert_form(&self, space_id: &str, form_def: &Value) -> Result<()> {
        validate_storage_id(validate_space_id(space_id))?;
        form::upsert_form(&self.operator, &self.workspace_path(space_id), form_def).await
    }

    pub async fn create_entry(
        &self,
        space_id: &str,
        entry_id: &str,
        markdown: &str,
        author: &str,
    ) -> Result<Value> {
        validate_storage_id(validate_space_id(space_id))?;
        validate_storage_id(validate_entry_id(entry_id))?;
        let integrity = RealIntegrityProvider::from_space(&self.operator, space_id).await?;
        let workspace = self.workspace_path(space_id);
        entry::create_entry(
            &self.operator,
            &workspace,
            entry_id,
            markdown,
            author,
            &integrity,
        )
        .await?;
        entry::get_entry(&self.operator, &workspace, entry_id).await
    }

    pub async fn list_entries(&self, space_id: &str) -> Result<Vec<Value>> {
        validate_storage_id(validate_space_id(space_id))?;
        entry::list_entries(&self.operator, &self.workspace_path(space_id)).await
    }

    pub async fn get_entry(&self, space_id: &str, entry_id: &str) -> Result<Value> {
        validate_storage_id(validate_space_id(space_id))?;
        validate_storage_id(validate_entry_id(entry_id))?;
        entry::get_entry(&self.operator, &self.workspace_path(space_id), entry_id).await
    }

    pub async fn update_entry(
        &self,
        space_id: &str,
        entry_id: &str,
        markdown: &str,
        parent_revision_id: Option<&str>,
        author: &str,
        assets: Option<Vec<Value>>,
    ) -> Result<Value> {
        validate_storage_id(validate_space_id(space_id))?;
        validate_storage_id(validate_entry_id(entry_id))?;
        if let Some(parent_revision_id) = parent_revision_id {
            validate_storage_id(validate_revision_id(parent_revision_id))?;
        }
        let integrity = RealIntegrityProvider::from_space(&self.operator, space_id).await?;
        entry::update_entry(
            &self.operator,
            &self.workspace_path(space_id),
            entry_id,
            markdown,
            parent_revision_id,
            author,
            assets,
            &integrity,
        )
        .await
    }

    pub async fn delete_entry(
        &self,
        space_id: &str,
        entry_id: &str,
        hard_delete: bool,
    ) -> Result<()> {
        validate_storage_id(validate_space_id(space_id))?;
        validate_storage_id(validate_entry_id(entry_id))?;
        entry::delete_entry(
            &self.operator,
            &self.workspace_path(space_id),
            entry_id,
            hard_delete,
        )
        .await
    }

    pub async fn entry_history(&self, space_id: &str, entry_id: &str) -> Result<Value> {
        validate_storage_id(validate_space_id(space_id))?;
        validate_storage_id(validate_entry_id(entry_id))?;
        entry::get_entry_history(&self.operator, &self.workspace_path(space_id), entry_id).await
    }

    pub async fn entry_revision(
        &self,
        space_id: &str,
        entry_id: &str,
        revision_id: &str,
    ) -> Result<Value> {
        validate_storage_id(validate_space_id(space_id))?;
        validate_storage_id(validate_entry_id(entry_id))?;
        validate_storage_id(validate_revision_id(revision_id))?;
        Ok(serde_json::to_value(
            entry::get_entry_revision_content(
                &self.operator,
                &self.workspace_path(space_id),
                entry_id,
                revision_id,
            )
            .await?,
        )?)
    }

    pub async fn restore_entry(
        &self,
        space_id: &str,
        entry_id: &str,
        revision_id: &str,
        author: &str,
    ) -> Result<Value> {
        validate_storage_id(validate_space_id(space_id))?;
        validate_storage_id(validate_entry_id(entry_id))?;
        validate_storage_id(validate_revision_id(revision_id))?;
        let integrity = RealIntegrityProvider::from_space(&self.operator, space_id).await?;
        entry::restore_entry(
            &self.operator,
            &self.workspace_path(space_id),
            entry_id,
            revision_id,
            author,
            &integrity,
        )
        .await
    }

    pub async fn list_entry_options(
        &self,
        space_id: &str,
        form: Option<&str>,
        query: Option<&str>,
        limit: usize,
    ) -> Result<Vec<entry::EntrySummary>> {
        validate_storage_id(validate_space_id(space_id))?;
        if let Some(form) = form {
            validate_storage_id(validate_form_name(form))?;
        }
        entry::list_entry_summaries(
            &self.operator,
            &self.workspace_path(space_id),
            form,
            query,
            limit,
        )
        .await
    }

    pub async fn list_entry_options_authorized(
        &self,
        space_id: &str,
        principal_id: Uuid,
        form: Option<&str>,
        query: Option<&str>,
        limit: usize,
    ) -> Result<Vec<entry::EntrySummary>> {
        let allowed = self.authorized_entry_ids(space_id, principal_id).await?;
        entry::list_entry_summaries_authorized(
            &self.operator,
            &self.workspace_path(space_id),
            form,
            query,
            limit,
            Some(&allowed),
        )
        .await
    }

    pub async fn list_entry_options_authorized_for_principals(
        &self,
        space_id: &str,
        principal_ids: &[Uuid],
        form: Option<&str>,
        query: Option<&str>,
        limit: usize,
    ) -> Result<Vec<entry::EntrySummary>> {
        let allowed = self
            .authorized_entry_ids_for_principals(space_id, principal_ids)
            .await?;
        entry::list_entry_summaries_authorized(
            &self.operator,
            &self.workspace_path(space_id),
            form,
            query,
            limit,
            Some(&allowed),
        )
        .await
    }

    pub async fn search_entries(
        &self,
        space_id: &str,
        query: &str,
    ) -> Result<Vec<search::SearchResult>> {
        validate_storage_id(validate_space_id(space_id))?;
        search::search_entries(&self.operator, &self.workspace_path(space_id), query).await
    }

    pub async fn query_entries(&self, space_id: &str, filter: &Value) -> Result<Vec<Value>> {
        validate_storage_id(validate_space_id(space_id))?;
        index::query_index(
            &self.operator,
            &self.workspace_path(space_id),
            &filter.to_string(),
        )
        .await
    }

    pub async fn execute_sql_query(&self, space_id: &str, sql: &str) -> Result<Vec<Value>> {
        validate_storage_id(validate_space_id(space_id))?;
        index::execute_sql_query(&self.operator, &self.workspace_path(space_id), sql).await
    }

    pub async fn require_resource_action(
        &self,
        space_id: &str,
        principal_id: Uuid,
        action: Action,
        kind: ResourceKind,
        resource_id: &str,
        parent: Option<ResourceRef>,
    ) -> Result<()> {
        let parent = if matches!(kind, ResourceKind::Asset) && parent.is_none() {
            self.asset_parent_entry(space_id, resource_id)
                .await?
                .map(|id| ResourceRef {
                    kind: ResourceKind::Entry,
                    id,
                    parent: None,
                })
        } else {
            parent
        };
        Authorizer::new(self.operator.clone())
            .require(
                space_id,
                principal_id,
                action,
                Some(&ResourceRef {
                    kind,
                    id: resource_id.to_string(),
                    parent: parent.map(Box::new),
                }),
            )
            .await
    }

    pub async fn authorized_entry_ids(
        &self,
        space_id: &str,
        principal_id: Uuid,
    ) -> Result<HashSet<String>> {
        let entries = self.list_entries(space_id).await?;
        let resources: Vec<ResourceRef> = entries
            .iter()
            .filter_map(|entry| entry.get("id").and_then(Value::as_str).map(str::to_string))
            .map(|id| ResourceRef {
                kind: ResourceKind::Entry,
                id,
                parent: None,
            })
            .collect();
        Ok(Authorizer::new(self.operator.clone())
            .filter_authorized_resources(space_id, principal_id, resources, Action::Read)
            .await?
            .into_iter()
            .collect())
    }

    /// Build the Core-owned Form → Entry authorization boundary used by the
    /// DataFusion adapter. A Form absent from this map is deliberately not a
    /// SQL relation at all; it must not become an empty but discoverable view.
    pub async fn authorized_form_entry_ids(
        &self,
        space_id: &str,
        principal_id: Uuid,
    ) -> Result<BTreeMap<String, HashSet<String>>> {
        let allowed = self.authorized_entry_ids(space_id, principal_id).await?;
        let mut by_form = BTreeMap::<String, HashSet<String>>::new();
        for entry in self.list_entries(space_id).await? {
            let Some(entry_id) = entry.get("id").and_then(Value::as_str) else {
                continue;
            };
            let Some(form) = entry.get("form").and_then(Value::as_str) else {
                continue;
            };
            if allowed.contains(entry_id) {
                by_form
                    .entry(form.to_ascii_lowercase())
                    .or_default()
                    .insert(entry_id.to_string());
            }
        }
        Ok(by_form)
    }

    pub async fn authorized_entry_ids_for_principals(
        &self,
        space_id: &str,
        principal_ids: &[Uuid],
    ) -> Result<HashSet<String>> {
        let mut principals = principal_ids.iter().copied();
        let Some(first) = principals.next() else {
            return Ok(HashSet::new());
        };
        let mut allowed = self.authorized_entry_ids(space_id, first).await?;
        for principal_id in principals {
            let next = self.authorized_entry_ids(space_id, principal_id).await?;
            allowed.retain(|entry_id| next.contains(entry_id));
        }
        Ok(allowed)
    }

    pub async fn authorized_form_entry_ids_for_principals(
        &self,
        space_id: &str,
        principal_ids: &[Uuid],
    ) -> Result<BTreeMap<String, HashSet<String>>> {
        let allowed = self
            .authorized_entry_ids_for_principals(space_id, principal_ids)
            .await?;
        let mut by_form = BTreeMap::<String, HashSet<String>>::new();
        for entry in self.list_entries(space_id).await? {
            let (Some(entry_id), Some(form)) = (
                entry.get("id").and_then(Value::as_str),
                entry.get("form").and_then(Value::as_str),
            ) else {
                continue;
            };
            if allowed.contains(entry_id) {
                by_form
                    .entry(form.to_ascii_lowercase())
                    .or_default()
                    .insert(entry_id.to_string());
            }
        }
        Ok(by_form)
    }

    pub async fn list_entries_authorized_for_principals(
        &self,
        space_id: &str,
        principal_ids: &[Uuid],
    ) -> Result<Vec<Value>> {
        let allowed = self
            .authorized_entry_ids_for_principals(space_id, principal_ids)
            .await?;
        Ok(self
            .list_entries(space_id)
            .await?
            .into_iter()
            .filter(|entry| {
                entry
                    .get("id")
                    .and_then(Value::as_str)
                    .is_some_and(|id| allowed.contains(id))
            })
            .collect())
    }

    pub async fn list_entries_authorized(
        &self,
        space_id: &str,
        principal_id: Uuid,
    ) -> Result<Vec<Value>> {
        let allowed = self.authorized_entry_ids(space_id, principal_id).await?;
        Ok(self
            .list_entries(space_id)
            .await?
            .into_iter()
            .filter(|entry| {
                entry
                    .get("id")
                    .and_then(Value::as_str)
                    .is_some_and(|id| allowed.contains(id))
            })
            .collect())
    }

    pub async fn filter_json_resources_authorized(
        &self,
        space_id: &str,
        principal_id: Uuid,
        kind: ResourceKind,
        id_field: &str,
        values: Vec<Value>,
    ) -> Result<Vec<Value>> {
        let mut resources = Vec::new();
        for value in &values {
            let Some(id) = value.get(id_field).and_then(Value::as_str) else {
                continue;
            };
            let parent = if matches!(kind, ResourceKind::Asset) {
                self.asset_parent_entry(space_id, id)
                    .await?
                    .map(|entry_id| {
                        Box::new(ResourceRef {
                            kind: ResourceKind::Entry,
                            id: entry_id,
                            parent: None,
                        })
                    })
            } else {
                None
            };
            resources.push(ResourceRef {
                kind: kind.clone(),
                id: id.to_string(),
                parent,
            });
        }
        let allowed = Authorizer::new(self.operator.clone())
            .filter_authorized_resources(space_id, principal_id, resources, Action::Read)
            .await?;
        Ok(values
            .into_iter()
            .filter(|value| {
                value
                    .get(id_field)
                    .and_then(Value::as_str)
                    .is_some_and(|id| allowed.contains(id))
            })
            .collect())
    }

    pub async fn filter_json_resources_authorized_for_principals(
        &self,
        space_id: &str,
        principal_ids: &[Uuid],
        kind: ResourceKind,
        id_field: &str,
        values: Vec<Value>,
    ) -> Result<Vec<Value>> {
        let mut filtered = values;
        for principal_id in principal_ids {
            filtered = self
                .filter_json_resources_authorized(
                    space_id,
                    *principal_id,
                    kind.clone(),
                    id_field,
                    filtered,
                )
                .await?;
        }
        Ok(filtered)
    }

    pub async fn search_entries_authorized_for_principals(
        &self,
        space_id: &str,
        principal_ids: &[Uuid],
        query: &str,
    ) -> Result<Vec<search::SearchResult>> {
        let allowed = self
            .authorized_entry_ids_for_principals(space_id, principal_ids)
            .await?;
        search::search_entries_authorized(
            &self.operator,
            &self.workspace_path(space_id),
            query,
            &allowed,
        )
        .await
    }

    pub async fn query_entries_authorized_for_principals(
        &self,
        space_id: &str,
        principal_ids: &[Uuid],
        filter: &Value,
    ) -> Result<Vec<Value>> {
        let allowed = self
            .authorized_entry_ids_for_principals(space_id, principal_ids)
            .await?;
        index::query_index_authorized(
            &self.operator,
            &self.workspace_path(space_id),
            &filter.to_string(),
            &allowed,
        )
        .await
    }

    pub async fn create_sql_session_authorized_for_principals(
        &self,
        space_id: &str,
        principal_ids: &[Uuid],
        sql: &str,
    ) -> Result<Value> {
        validate_storage_id(validate_space_id(space_id))?;
        let allowed = self
            .authorized_form_entry_ids_for_principals(space_id, principal_ids)
            .await?;
        sql_session::create_sql_session_authorized_for_principals_by_form(
            &self.operator,
            &self.workspace_path(space_id),
            sql,
            &allowed,
            principal_ids,
        )
        .await
    }

    pub async fn require_sql_session_principals(
        &self,
        space_id: &str,
        session_id: &str,
        principal_ids: &[Uuid],
    ) -> Result<()> {
        validate_storage_id(validate_space_id(space_id))?;
        validate_storage_id(validate_sql_session_id(session_id))?;
        sql_session::require_session_principals(
            &self.operator,
            &self.workspace_path(space_id),
            session_id,
            principal_ids,
        )
        .await
    }

    async fn asset_parent_entry(&self, space_id: &str, asset_id: &str) -> Result<Option<String>> {
        Ok(self
            .list_entries(space_id)
            .await?
            .into_iter()
            .find_map(|entry| {
                let linked = entry
                    .get("assets")
                    .and_then(Value::as_array)
                    .is_some_and(|assets| {
                        assets
                            .iter()
                            .any(|asset| asset.get("id").and_then(Value::as_str) == Some(asset_id))
                    });
                linked
                    .then(|| entry.get("id").and_then(Value::as_str).map(str::to_string))
                    .flatten()
            }))
    }

    pub async fn search_entries_authorized(
        &self,
        space_id: &str,
        principal_id: Uuid,
        query: &str,
    ) -> Result<Vec<search::SearchResult>> {
        let allowed = self.authorized_entry_ids(space_id, principal_id).await?;
        search::search_entries_authorized(
            &self.operator,
            &self.workspace_path(space_id),
            query,
            &allowed,
        )
        .await
    }

    pub async fn query_entries_authorized(
        &self,
        space_id: &str,
        principal_id: Uuid,
        filter: &Value,
    ) -> Result<Vec<Value>> {
        let allowed = self.authorized_entry_ids(space_id, principal_id).await?;
        index::query_index_authorized(
            &self.operator,
            &self.workspace_path(space_id),
            &filter.to_string(),
            &allowed,
        )
        .await
    }

    pub async fn execute_sql_query_authorized(
        &self,
        space_id: &str,
        principal_id: Uuid,
        sql: &str,
    ) -> Result<Vec<Value>> {
        let allowed = self
            .authorized_form_entry_ids(space_id, principal_id)
            .await?;
        index::execute_sql_query_authorized_by_form(
            &self.operator,
            &self.workspace_path(space_id),
            sql,
            &allowed,
        )
        .await
    }

    pub async fn reindex(&self, space_id: &str) -> Result<()> {
        validate_storage_id(validate_space_id(space_id))?;
        index::reindex_all(&self.operator, &self.workspace_path(space_id)).await
    }

    pub async fn space_stats(&self, space_id: &str) -> Result<Value> {
        validate_storage_id(validate_space_id(space_id))?;
        index::get_space_stats(&self.operator, &self.workspace_path(space_id)).await
    }

    pub async fn list_assets(&self, space_id: &str) -> Result<Vec<asset::AssetInfo>> {
        validate_storage_id(validate_space_id(space_id))?;
        asset::list_assets(&self.operator, &self.workspace_path(space_id)).await
    }

    pub async fn save_asset(
        &self,
        space_id: &str,
        filename: &str,
        content: &[u8],
    ) -> Result<asset::AssetInfo> {
        validate_storage_id(validate_space_id(space_id))?;
        asset::save_asset(
            &self.operator,
            &self.workspace_path(space_id),
            filename,
            content,
        )
        .await
    }

    pub async fn read_asset(&self, space_id: &str, asset_id: &str) -> Result<asset::AssetContent> {
        validate_storage_id(validate_space_id(space_id))?;
        validate_storage_id(validate_asset_id(asset_id))?;
        asset::read_asset(&self.operator, &self.workspace_path(space_id), asset_id).await
    }

    pub async fn delete_asset(&self, space_id: &str, asset_id: &str) -> Result<()> {
        validate_storage_id(validate_space_id(space_id))?;
        validate_storage_id(validate_asset_id(asset_id))?;
        asset::delete_asset(&self.operator, &self.workspace_path(space_id), asset_id).await
    }

    pub async fn get_user_preferences(
        &self,
        user_id: &str,
    ) -> Result<preferences::UserPreferences> {
        preferences::get_user_preferences(&self.operator, user_id).await
    }

    pub async fn patch_user_preferences(
        &self,
        user_id: &str,
        patch: &Value,
    ) -> Result<preferences::UserPreferences> {
        preferences::patch_user_preferences(&self.operator, user_id, patch).await
    }

    pub async fn create_sql_session(&self, space_id: &str, sql: &str) -> Result<Value> {
        validate_storage_id(validate_space_id(space_id))?;
        sql_session::create_sql_session(&self.operator, &self.workspace_path(space_id), sql).await
    }

    pub async fn create_sql_session_authorized(
        &self,
        space_id: &str,
        principal_id: Uuid,
        sql: &str,
    ) -> Result<Value> {
        validate_storage_id(validate_space_id(space_id))?;
        let allowed = self.authorized_entry_ids(space_id, principal_id).await?;
        sql_session::create_sql_session_authorized(
            &self.operator,
            &self.workspace_path(space_id),
            sql,
            &allowed,
        )
        .await
    }

    pub async fn get_sql_session(&self, space_id: &str, session_id: &str) -> Result<Value> {
        validate_storage_id(validate_space_id(space_id))?;
        validate_storage_id(validate_sql_session_id(session_id))?;
        sql_session::get_sql_session_status(
            &self.operator,
            &self.workspace_path(space_id),
            session_id,
        )
        .await
    }

    pub async fn get_sql_session_count(&self, space_id: &str, session_id: &str) -> Result<u64> {
        validate_storage_id(validate_space_id(space_id))?;
        validate_storage_id(validate_sql_session_id(session_id))?;
        sql_session::get_sql_session_count(
            &self.operator,
            &self.workspace_path(space_id),
            session_id,
        )
        .await
    }

    pub async fn get_sql_session_rows(
        &self,
        space_id: &str,
        session_id: &str,
        offset: usize,
        limit: usize,
    ) -> Result<Value> {
        validate_storage_id(validate_space_id(space_id))?;
        validate_storage_id(validate_sql_session_id(session_id))?;
        sql_session::get_sql_session_rows(
            &self.operator,
            &self.workspace_path(space_id),
            session_id,
            offset,
            limit,
        )
        .await
    }

    pub async fn list_saved_sql(&self, space_id: &str) -> Result<Vec<Value>> {
        validate_storage_id(validate_space_id(space_id))?;
        saved_sql::list_sql(&self.operator, &self.workspace_path(space_id)).await
    }

    pub async fn create_saved_sql(
        &self,
        space_id: &str,
        sql_id: &str,
        payload: &saved_sql::SqlPayload,
        author: &str,
    ) -> Result<Value> {
        validate_storage_id(validate_space_id(space_id))?;
        validate_storage_id(validate_sql_id(sql_id))?;
        let integrity = RealIntegrityProvider::from_space(&self.operator, space_id).await?;
        saved_sql::create_sql(
            &self.operator,
            &self.workspace_path(space_id),
            sql_id,
            payload,
            author,
            &integrity,
        )
        .await
    }

    pub async fn get_saved_sql(&self, space_id: &str, sql_id: &str) -> Result<Value> {
        validate_storage_id(validate_space_id(space_id))?;
        validate_storage_id(validate_sql_id(sql_id))?;
        saved_sql::get_sql(&self.operator, &self.workspace_path(space_id), sql_id).await
    }

    pub async fn update_saved_sql(
        &self,
        space_id: &str,
        sql_id: &str,
        payload: &saved_sql::SqlPayload,
        parent_revision_id: Option<&str>,
        author: &str,
    ) -> Result<Value> {
        validate_storage_id(validate_space_id(space_id))?;
        validate_storage_id(validate_sql_id(sql_id))?;
        if let Some(parent_revision_id) = parent_revision_id {
            validate_storage_id(validate_revision_id(parent_revision_id))?;
        }
        let integrity = RealIntegrityProvider::from_space(&self.operator, space_id).await?;
        saved_sql::update_sql(
            &self.operator,
            &self.workspace_path(space_id),
            sql_id,
            payload,
            parent_revision_id,
            author,
            &integrity,
        )
        .await
    }

    pub async fn delete_saved_sql(&self, space_id: &str, sql_id: &str) -> Result<()> {
        validate_storage_id(validate_space_id(space_id))?;
        validate_storage_id(validate_sql_id(sql_id))?;
        saved_sql::delete_sql(&self.operator, &self.workspace_path(space_id), sql_id).await
    }

    pub async fn test_storage_connection(
        &self,
        config: &space::StorageConnectionTestConfig,
    ) -> Result<Value> {
        space::test_storage_connection(config).await
    }
}

fn validate_storage_id(
    result: std::result::Result<(), ugoite_domain::id::IdentifierError>,
) -> Result<()> {
    result.map_err(|error| AppError::invalid_identifier(error.to_string()).into())
}

pub fn validate_public_space_patch(patch: &Value) -> Result<()> {
    let reserved_keys: Vec<&str> = patch
        .as_object()
        .map(|object| {
            object
                .keys()
                .map(String::as_str)
                .filter(|key| MEMBERSHIP_MANAGED_SPACE_SETTING_KEYS.contains(key))
                .collect()
        })
        .unwrap_or_default();

    let mut reserved_keys = reserved_keys;
    if let Some(settings_obj) = patch.get("settings").and_then(Value::as_object) {
        for key in settings_obj.keys().map(String::as_str) {
            if MEMBERSHIP_MANAGED_SPACE_SETTING_KEYS.contains(&key) {
                reserved_keys.push(key);
            }
        }
    }
    reserved_keys.sort_unstable();
    reserved_keys.dedup();
    if reserved_keys.is_empty() {
        return Ok(());
    }

    Err(anyhow!(
        "space patch does not allow membership-managed settings keys: {}. Use the dedicated member commands instead.",
        reserved_keys.join(", ")
    ))
}
