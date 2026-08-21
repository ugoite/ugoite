use anyhow::{Error, Result};
use opendal::Operator;
use serde_json::Value;
use std::collections::HashSet;
use ugoite_core::error::{AppError, ErrorCode};
use ugoite_core::query::EntryScope;
use ugoite_domain::entry::EntryRevision;
use ugoite_domain::form::FormDefinition;
use ugoite_domain::id::SpaceId;
use ugoite_storage::SpaceCatalogStore;
use uuid::Uuid;

async fn stable_space_id(operator: &Operator, workspace_path: &str) -> Result<SpaceId> {
    let metadata_path = format!("{}/meta.json", workspace_path.trim_end_matches('/'));
    if !operator.exists(&metadata_path).await? {
        return Err(anyhow::anyhow!(
            "unsupported Space layout: missing immutable metadata at {metadata_path}"
        ));
    }
    let metadata: Value =
        serde_json::from_slice(&crate::read_object_exact(operator, &metadata_path).await?)?;
    let directory_id = workspace_path
        .trim_matches('/')
        .rsplit('/')
        .next()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("unsupported Space layout: invalid workspace path"))?;
    let uuid = crate::space::validate_current_space_metadata(directory_id, &metadata)?;
    Ok(SpaceId::from(uuid))
}

/// Opens the authoritative logical Space workspace. Core deliberately sees no
/// Iceberg table, Arrow batch, or Catalog API; those remain implementation
/// details of `ugoite-iceberg`.
pub async fn native_workspace(
    operator: &Operator,
    workspace_path: &str,
) -> Result<crate::IcebergWorkspace> {
    let space_id = stable_space_id(operator, workspace_path).await?;
    let store = SpaceCatalogStore::new(operator.clone(), workspace_path)?;
    let store = if matches!(operator.info().scheme(), "s3" | "gcs" | "oss" | "azdls") {
        store.shared_read_only()
    } else {
        store.single_process()
    };
    crate::IcebergWorkspace::open_space(store, space_id, crate::WriteConfig::default()).await
}

pub async fn ensure_form_tables(
    operator: &Operator,
    workspace_path: &str,
    form_definition: &Value,
) -> Result<()> {
    crate::authorization::Authorizer::new(operator.clone())
        .ensure_authoritative_mutation_contract()?;
    let form = crate::form::to_domain_form(form_definition)?;
    // SQL helpers may call this function from read paths that lazily create
    // the system Form. That creation is still authoritative and must not
    // bypass the request's authorization write fence.
    crate::authorization::ensure_authorization_write_fence().await?;
    let workspace = native_workspace(operator, workspace_path).await?;
    if !workspace.has_form(form.id).await? {
        let command =
            crate::publication_context(format!("form-create:{}", form.id), "form.create", &form)?;
        workspace.commit(command)?.create_form(&form).await?;
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
            Error::from(AppError::not_found(
                ErrorCode::FormNotFound,
                format!("Form not found: {form_name}"),
            ))
        })?;
    Ok((workspace, form))
}

pub async fn revisions_for_form(
    operator: &Operator,
    workspace_path: &str,
    form_name: &str,
) -> Result<(FormDefinition, Vec<EntryRevision>)> {
    let (workspace, form) = domain_form_by_name(operator, workspace_path, form_name).await?;
    let revisions = workspace
        .read_revision_view(form.id, crate::RevisionView::All)
        .await?;
    Ok((form, revisions))
}

pub async fn latest_revisions_for_form(
    operator: &Operator,
    workspace_path: &str,
    form_name: &str,
) -> Result<(FormDefinition, Vec<EntryRevision>)> {
    let (workspace, form) = domain_form_by_name(operator, workspace_path, form_name).await?;
    let revisions = workspace
        .read_revision_view(form.id, crate::RevisionView::LatestIncludingTombstones)
        .await?;
    Ok((form, revisions))
}

/// Reads only the current revisions admitted by the caller's provider-side
/// scope. The scope is applied inside the canonical DataFusion latest-state
/// plan before Arrow/domain decoding.
pub async fn latest_revisions_for_form_authorized(
    operator: &Operator,
    workspace_path: &str,
    form_name: &str,
    entry_scope: EntryScope,
) -> Result<(FormDefinition, Vec<EntryRevision>)> {
    let (workspace, form) = domain_form_by_name(operator, workspace_path, form_name).await?;
    let revisions = workspace
        .read_revision_view_with_scope(
            form.id,
            entry_scope,
            crate::RevisionView::LatestIncludingTombstones,
        )
        .await?;
    Ok((form, revisions))
}

pub async fn latest_revisions_for_entry(
    operator: &Operator,
    workspace_path: &str,
    form_name: &str,
    entry_id: &str,
) -> Result<(FormDefinition, Vec<EntryRevision>)> {
    let (workspace, form) = domain_form_by_name(operator, workspace_path, form_name).await?;
    let entry_id = Uuid::parse_str(entry_id)
        .unwrap_or_else(|_| Uuid::new_v5(&Uuid::NAMESPACE_URL, entry_id.as_bytes()))
        .into();
    let revisions = workspace
        .read_latest_revisions_for_entry(form.id, entry_id)
        .await?;
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
