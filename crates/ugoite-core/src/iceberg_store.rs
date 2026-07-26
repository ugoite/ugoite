use anyhow::{anyhow, Context, Result};
use iceberg::{Catalog, TableIdent};
use opendal::Operator;
use serde_json::Value;
use std::collections::HashSet;
use std::sync::Arc;
use ugoite_domain::id::SpaceId;
use ugoite_storage::SpaceCatalogStore;
use uuid::Uuid;

const FORM_DEFINITION_PROPERTY: &str = "ugoite.form.definition.v1";

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

/// Opens the one authoritative Catalog for a Space.
///
/// Filesystem and in-memory adapters are deliberately configured for explicit
/// single-process mode until their shared conditional-object probes are
/// enabled by the server storage configuration.
pub async fn native_workspace(
    operator: &Operator,
    workspace_path: &str,
) -> Result<ugoite_iceberg::IcebergWorkspace> {
    let space_id = stable_space_id(operator, workspace_path).await?;
    let store = SpaceCatalogStore::new(operator.clone(), workspace_path)?.single_process();
    Ok(ugoite_iceberg::IcebergWorkspace::open_space(
        store,
        space_id,
        ugoite_iceberg::WriteConfig::default(),
    )
    .await?)
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
        ugoite_iceberg::physical_form_name(form.id),
    );
    if !workspace.catalog().table_exists(&table).await? {
        workspace.create_form(&form).await?;
    }
    Ok(())
}

async fn table_for_form_name(
    operator: &Operator,
    workspace_path: &str,
    form_name: &str,
) -> Result<(Arc<dyn Catalog>, iceberg::table::Table)> {
    let workspace = native_workspace(operator, workspace_path).await?;
    let catalog = workspace.catalog();
    for table in catalog.list_tables(workspace.namespace()).await? {
        let loaded = catalog.load_table(&table).await?;
        let Some(raw) = loaded.metadata().properties().get(FORM_DEFINITION_PROPERTY) else {
            continue;
        };
        let form: ugoite_domain::form::FormDefinition = serde_json::from_str(raw)?;
        if form.name == form_name {
            return Ok((catalog, loaded));
        }
    }
    Err(anyhow!(
        "Form is not registered in the authoritative SpaceCatalog: {form_name}"
    ))
}

pub async fn load_entries_table(
    operator: &Operator,
    workspace_path: &str,
    form_name: &str,
) -> Result<(Arc<dyn Catalog>, iceberg::table::Table)> {
    table_for_form_name(operator, workspace_path, form_name).await
}

pub async fn load_revisions_table(
    operator: &Operator,
    workspace_path: &str,
    form_name: &str,
) -> Result<(Arc<dyn Catalog>, iceberg::table::Table)> {
    table_for_form_name(operator, workspace_path, form_name).await
}

pub async fn load_form_schema_fields(
    operator: &Operator,
    workspace_path: &str,
    form_name: &str,
) -> Result<Option<HashSet<String>>> {
    let form = load_form_definition(operator, workspace_path, form_name).await?;
    let fields = form
        .get("fields")
        .and_then(Value::as_object)
        .map(|fields| fields.keys().cloned().collect());
    Ok(fields)
}

pub async fn list_form_names(operator: &Operator, workspace_path: &str) -> Result<Vec<String>> {
    let workspace = native_workspace(operator, workspace_path).await?;
    let catalog = workspace.catalog();
    let mut names = Vec::new();
    for table in catalog.list_tables(workspace.namespace()).await? {
        let loaded = catalog.load_table(&table).await?;
        let Some(raw) = loaded.metadata().properties().get(FORM_DEFINITION_PROPERTY) else {
            continue;
        };
        let form: ugoite_domain::form::FormDefinition = serde_json::from_str(raw)?;
        names.push(form.name);
    }
    names.sort();
    names.dedup();
    Ok(names)
}

pub async fn load_form_definition(
    operator: &Operator,
    workspace_path: &str,
    form_name: &str,
) -> Result<Value> {
    let (_, table) = table_for_form_name(operator, workspace_path, form_name).await?;
    let raw = table
        .metadata()
        .properties()
        .get(FORM_DEFINITION_PROPERTY)
        .context("Form definition missing from Iceberg table metadata")?;
    let form: ugoite_domain::form::FormDefinition = serde_json::from_str(raw)?;
    Ok(crate::form::from_domain_form(&form))
}
