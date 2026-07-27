use anyhow::{anyhow, Result};
use iceberg::TableIdent;
use opendal::Operator;
use serde_json::Value;
use std::collections::HashSet;
use ugoite_domain::entry::EntryRevision;
use ugoite_domain::form::FormDefinition;
use ugoite_domain::id::SpaceId;
use ugoite_storage::SpaceCatalogStore;
use uuid::Uuid;

async fn stable_space_id(operator: &Operator, workspace_path: &str) -> Result<SpaceId> {
    let metadata_path = format!("{}/meta.json", workspace_path.trim_end_matches('/'));
    if operator.exists(&metadata_path).await? {
        let metadata: Value =
            serde_json::from_slice(&operator.read(&metadata_path).await?.to_vec())?;
        if let Some(raw) = metadata
            .get("space_uid")
            .or_else(|| metadata.get("space_id"))
            .and_then(Value::as_str)
        {
            if let Ok(uuid) = Uuid::parse_str(raw) {
                return Ok(SpaceId::from(uuid));
            }
        }
    }
    Ok(SpaceId::from(Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        workspace_path.as_bytes(),
    )))
}

/// Opens the authoritative logical Space workspace. Core deliberately sees no
/// Iceberg table, Arrow batch, or Catalog API; those remain implementation
/// details of `ugoite-iceberg`.
pub async fn native_workspace(
    operator: &Operator,
    workspace_path: &str,
) -> Result<crate::IcebergWorkspace> {
    let space_id = stable_space_id(operator, workspace_path).await?;
    let store = SpaceCatalogStore::new(operator.clone(), workspace_path)?.single_process();
    crate::IcebergWorkspace::open_space(store, space_id, crate::WriteConfig::default()).await
}

pub async fn ensure_form_tables(
    operator: &Operator,
    workspace_path: &str,
    form_definition: &Value,
) -> Result<()> {
    let form = crate::form::to_domain_form(form_definition)?;
    let workspace = native_workspace(operator, workspace_path).await?;
    let table = TableIdent::new(
        workspace.namespace().clone(),
        crate::physical_form_name(form.id),
    );
    if !workspace.catalog().table_exists(&table).await? {
        workspace.create_form(&form).await?;
    }
    Ok(())
}

async fn domain_form_by_name(
    operator: &Operator,
    workspace_path: &str,
    form_name: &str,
) -> Result<(crate::IcebergWorkspace, FormDefinition)> {
    let workspace = native_workspace(operator, workspace_path).await?;
    let form = workspace
        .list_forms()
        .await?
        .into_iter()
        .find(|form| form.name == form_name)
        .ok_or_else(|| {
            anyhow!("Form is not registered in the authoritative SpaceCatalog: {form_name}")
        })?;
    Ok((workspace, form))
}

pub async fn revisions_for_form(
    operator: &Operator,
    workspace_path: &str,
    form_name: &str,
) -> Result<(FormDefinition, Vec<EntryRevision>)> {
    let (workspace, form) = domain_form_by_name(operator, workspace_path, form_name).await?;
    let revisions = workspace.read_revisions(form.id).await?;
    Ok((form, revisions))
}

pub async fn load_form_schema_fields(
    operator: &Operator,
    workspace_path: &str,
    form_name: &str,
) -> Result<Option<HashSet<String>>> {
    let form = load_domain_form(operator, workspace_path, form_name).await?;
    Ok(Some(
        form.fields.into_iter().map(|field| field.name).collect(),
    ))
}

pub async fn list_form_names(operator: &Operator, workspace_path: &str) -> Result<Vec<String>> {
    let workspace = native_workspace(operator, workspace_path).await?;
    Ok(workspace
        .list_forms()
        .await?
        .into_iter()
        .map(|form| form.name)
        .collect())
}

pub async fn load_domain_form(
    operator: &Operator,
    workspace_path: &str,
    form_name: &str,
) -> Result<FormDefinition> {
    let (_, form) = domain_form_by_name(operator, workspace_path, form_name).await?;
    Ok(form)
}

pub async fn load_form_definition(
    operator: &Operator,
    workspace_path: &str,
    form_name: &str,
) -> Result<Value> {
    Ok(crate::form::from_domain_form(
        &load_domain_form(operator, workspace_path, form_name).await?,
    ))
}
