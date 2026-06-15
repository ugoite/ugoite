use anyhow::Result;
use opendal::Operator;
use serde_json::Value;

use crate::integrity::RealIntegrityProvider;
use crate::{asset, entry, form, search, space, storage::operator_from_uri};

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
        space::create_space(&self.operator, space_id, &self.root_uri).await
    }

    pub async fn list_space_ids(&self) -> Result<Vec<String>> {
        space::list_spaces(&self.operator).await
    }

    pub async fn get_space(&self, space_id: &str) -> Result<Value> {
        space::get_space_raw(&self.operator, space_id).await
    }

    pub async fn patch_space(&self, space_id: &str, patch: &Value) -> Result<Value> {
        space::patch_space(&self.operator, space_id, patch).await
    }

    pub async fn ensure_space(&self, space_id: &str) -> Result<()> {
        space::get_space(&self.operator, space_id).await.map(|_| ())
    }

    pub async fn list_forms(&self, space_id: &str) -> Result<Vec<Value>> {
        form::list_forms(&self.operator, &self.workspace_path(space_id)).await
    }

    pub async fn get_form(&self, space_id: &str, form_name: &str) -> Result<Value> {
        form::get_form(&self.operator, &self.workspace_path(space_id), form_name).await
    }

    pub async fn upsert_form(&self, space_id: &str, form_def: &Value) -> Result<()> {
        form::upsert_form(&self.operator, &self.workspace_path(space_id), form_def).await
    }

    pub async fn create_entry(
        &self,
        space_id: &str,
        entry_id: &str,
        markdown: &str,
        author: &str,
    ) -> Result<Value> {
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
        entry::list_entries(&self.operator, &self.workspace_path(space_id)).await
    }

    pub async fn get_entry(&self, space_id: &str, entry_id: &str) -> Result<Value> {
        entry::get_entry(&self.operator, &self.workspace_path(space_id), entry_id).await
    }

    pub async fn search_entries(
        &self,
        space_id: &str,
        query: &str,
    ) -> Result<Vec<search::SearchResult>> {
        search::search_entries(&self.operator, &self.workspace_path(space_id), query).await
    }

    pub async fn list_assets(&self, space_id: &str) -> Result<Vec<asset::AssetInfo>> {
        asset::list_assets(&self.operator, &self.workspace_path(space_id)).await
    }

    pub async fn save_asset(
        &self,
        space_id: &str,
        filename: &str,
        content: &[u8],
    ) -> Result<asset::AssetInfo> {
        asset::save_asset(
            &self.operator,
            &self.workspace_path(space_id),
            filename,
            content,
        )
        .await
    }

    pub async fn delete_asset(&self, space_id: &str, asset_id: &str) -> Result<()> {
        asset::delete_asset(&self.operator, &self.workspace_path(space_id), asset_id).await
    }
}
