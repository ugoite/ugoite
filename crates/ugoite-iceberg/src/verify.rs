//! Read-only Space integrity verification for operators.

use anyhow::{Context, Result};
use opendal::Operator;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{authorization::Authorizer, iceberg_store, space};

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VerifyStatus {
    Valid,
    ValidWithRebuildableDerivedState,
    Invalid,
    Incomplete,
}

#[derive(Debug, Clone, Serialize)]
pub struct VerifySection {
    pub status: VerifyStatus,
    pub checked: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SpaceVerifyReport {
    pub schema_version: u32,
    pub space_id: String,
    pub valid: bool,
    pub status: VerifyStatus,
    pub deep: bool,
    pub sections: VerifySections,
}

#[derive(Debug, Clone, Serialize)]
pub struct VerifySections {
    pub metadata: VerifySection,
    pub catalog: VerifySection,
    pub forms: VerifySection,
    pub entries: VerifySection,
    pub changes_and_audit: VerifySection,
    pub assets: VerifySection,
    pub derived: VerifySection,
    pub authorization: VerifySection,
}

fn section(status: VerifyStatus, checked: bool, detail: Option<String>) -> VerifySection {
    VerifySection {
        status,
        checked,
        detail,
        count: None,
    }
}

/// Inspects a Space's authoritative data and its separately managed Node
/// authorization state. The verifier never invokes a repair path.
pub async fn verify_space(
    operator: &Operator,
    space_id: &str,
    deep: bool,
) -> Result<SpaceVerifyReport> {
    let workspace_path = format!("spaces/{space_id}");
    let metadata_value = match space::get_space_raw_read_only(operator, space_id).await {
        Ok(metadata) => metadata,
        Err(error) => {
            let status = evidence_error_status(&error);
            return Ok(metadata_failure_report(
                space_id,
                deep,
                status,
                error.to_string(),
            ));
        }
    };
    let space_uid = match space::validate_current_space_metadata(space_id, &metadata_value) {
        Ok(space_uid) => space_uid,
        Err(error) => {
            return Ok(metadata_failure_report(
                space_id,
                deep,
                VerifyStatus::Invalid,
                error.to_string(),
            ));
        }
    };
    let metadata = section(VerifyStatus::Valid, true, None);

    // `open_space_read_only` deliberately skips Catalog namespace creation.
    let workspace = match iceberg_store::native_workspace_read_only(operator, &workspace_path).await
    {
        Ok(workspace) => workspace,
        Err(error) => {
            return Ok(catalog_failure_report(
                space_id,
                deep,
                evidence_error_status(&error),
                error.to_string(),
            ));
        }
    };
    let health = match workspace.health_report(&[]).await {
        Ok(health) => health,
        Err(error) => {
            return Ok(catalog_failure_report(
                space_id,
                deep,
                evidence_error_status(&error),
                error.to_string(),
            ));
        }
    };
    let issue_codes = health
        .catalog_head
        .issue
        .iter()
        .map(|issue| issue.code)
        .chain(
            health
                .tables
                .iter()
                .filter_map(|table| table.issue.as_ref().map(|issue| issue.code)),
        )
        .collect::<Vec<_>>();
    let catalog_status = if health.status == crate::health::HealthStatus::Healthy {
        VerifyStatus::Valid
    } else if issue_codes
        .iter()
        .any(|code| health_issue_status(code) == VerifyStatus::Invalid)
    {
        VerifyStatus::Invalid
    } else {
        VerifyStatus::Incomplete
    };
    let catalog = VerifySection {
        status: catalog_status,
        checked: true,
        detail: (!issue_codes.is_empty()).then(|| issue_codes.to_vec().join(",")),
        count: Some(health.tables.len()),
    };

    let forms_result = crate::form::list_forms_read_only(operator, &workspace_path).await;
    let forms = match forms_result {
        Ok(forms) => {
            let mut issue = None;
            for form in &forms {
                if let Err(error) = crate::form::to_domain_form(form) {
                    issue = Some(error.to_string());
                    break;
                }
            }
            VerifySection {
                status: if issue.is_some() {
                    VerifyStatus::Invalid
                } else {
                    VerifyStatus::Valid
                },
                checked: true,
                detail: issue,
                count: Some(forms.len()),
            }
        }
        Err(error) => section(evidence_error_status(&error), true, Some(error.to_string())),
    };

    let entries_result = if catalog_status == VerifyStatus::Valid {
        Some(verify_entries(operator, &workspace_path).await)
    } else {
        None
    };
    let (entries, asset_references) = match entries_result {
        Some(Ok((entry_count, revision_count, asset_references))) => (
            VerifySection {
                status: VerifyStatus::Valid,
                checked: true,
                detail: Some(format!("{entry_count} Entries; all revisions verified")),
                count: Some(revision_count),
            },
            asset_references,
        ),
        Some(Err(error)) => (
            section(evidence_error_status(&error), true, Some(error.to_string())),
            Vec::new(),
        ),
        None => (
            section(
                VerifyStatus::Incomplete,
                false,
                Some("Catalog is invalid; Entry tables were not opened".to_string()),
            ),
            Vec::new(),
        ),
    };
    let changes_and_audit = match crate::audit::verify_integrity(operator, space_id).await {
        Ok(count) => VerifySection {
            status: VerifyStatus::Valid,
            checked: true,
            detail: None,
            count: Some(count),
        },
        Err(error) => section(evidence_error_status(&error), true, Some(error.to_string())),
    };
    let assets = match verify_assets(operator, &workspace_path, &asset_references, deep).await {
        Ok(count) => VerifySection {
            status: VerifyStatus::Valid,
            checked: true,
            detail: None,
            count: Some(count),
        },
        Err(error) => section(evidence_error_status(&error), true, Some(error.to_string())),
    };
    let derived = match crate::derived_relation::asset_text_refresh_needed(
        operator,
        &workspace_path,
    )
    .await
    {
        Ok(true) => section(
            VerifyStatus::ValidWithRebuildableDerivedState,
            true,
            Some(
                "derived indexes are stale or corrupt and can be rebuilt with `ugoite index run`"
                    .to_string(),
            ),
        ),
        Ok(false) => section(VerifyStatus::Valid, true, None),
        Err(error) => section(evidence_error_status(&error), true, Some(error.to_string())),
    };

    let authorization = match Authorizer::new(operator.clone())
        .state_if_present(space_id, space_uid)
        .await
    {
        Ok(Some(_)) => section(VerifyStatus::Valid, true, None),
        Ok(None) => section(
            VerifyStatus::Incomplete,
            true,
            Some("authorization state is not initialized".to_string()),
        ),
        Err(error) => section(evidence_error_status(&error), true, Some(error.to_string())),
    };

    let sections = VerifySections {
        metadata,
        catalog,
        forms,
        entries,
        changes_and_audit,
        assets,
        derived,
        authorization,
    };
    let statuses = [
        sections.metadata.status,
        sections.catalog.status,
        sections.forms.status,
        sections.entries.status,
        sections.changes_and_audit.status,
        sections.assets.status,
        sections.derived.status,
    ];
    let status = if statuses.contains(&VerifyStatus::Invalid) {
        VerifyStatus::Invalid
    } else if statuses.contains(&VerifyStatus::Incomplete) {
        VerifyStatus::Incomplete
    } else if statuses.contains(&VerifyStatus::ValidWithRebuildableDerivedState) {
        VerifyStatus::ValidWithRebuildableDerivedState
    } else {
        VerifyStatus::Valid
    };
    Ok(SpaceVerifyReport {
        schema_version: 1,
        space_id: space_id.to_string(),
        valid: matches!(
            status,
            VerifyStatus::Valid | VerifyStatus::ValidWithRebuildableDerivedState
        ),
        status,
        deep,
        sections,
    })
}

fn metadata_failure_report(
    space_id: &str,
    deep: bool,
    status: VerifyStatus,
    detail: String,
) -> SpaceVerifyReport {
    let invalid = section(status, true, Some(detail));
    let incomplete = section(
        VerifyStatus::Incomplete,
        false,
        Some("not inspected because Space identity could not be validated".to_string()),
    );
    SpaceVerifyReport {
        schema_version: 1,
        space_id: space_id.to_string(),
        valid: false,
        status,
        deep,
        sections: VerifySections {
            metadata: invalid,
            catalog: incomplete.clone(),
            forms: incomplete.clone(),
            entries: incomplete.clone(),
            changes_and_audit: incomplete.clone(),
            assets: incomplete.clone(),
            derived: incomplete.clone(),
            authorization: incomplete,
        },
    }
}

fn catalog_failure_report(
    space_id: &str,
    deep: bool,
    status: VerifyStatus,
    detail: String,
) -> SpaceVerifyReport {
    let metadata = section(VerifyStatus::Valid, true, None);
    let catalog = section(status, true, Some(detail));
    let incomplete = section(
        VerifyStatus::Incomplete,
        false,
        Some("not inspected because Catalog evidence is unavailable".to_string()),
    );
    let authorization = section(
        VerifyStatus::Incomplete,
        false,
        Some("not inspected because Catalog verification could not complete".to_string()),
    );
    SpaceVerifyReport {
        schema_version: 1,
        space_id: space_id.to_string(),
        valid: false,
        status,
        deep,
        sections: VerifySections {
            metadata,
            catalog,
            forms: incomplete.clone(),
            entries: incomplete.clone(),
            changes_and_audit: incomplete.clone(),
            assets: incomplete,
            derived: section(
                VerifyStatus::ValidWithRebuildableDerivedState,
                true,
                Some("derived indexes can be rebuilt separately".to_string()),
            ),
            authorization,
        },
    }
}

fn evidence_error_status(error: &anyhow::Error) -> VerifyStatus {
    if error.chain().any(|cause| {
        cause
            .downcast_ref::<opendal::Error>()
            .is_some_and(|error| error.kind() == opendal::ErrorKind::NotFound)
    }) {
        VerifyStatus::Invalid
    } else if error.chain().any(|cause| {
        cause.downcast_ref::<opendal::Error>().is_some()
            || cause
                .downcast_ref::<tokio::time::error::Elapsed>()
                .is_some()
    }) || error.to_string().contains("failed to probe Space metadata")
    {
        VerifyStatus::Incomplete
    } else {
        VerifyStatus::Invalid
    }
}

fn health_issue_status(code: &str) -> VerifyStatus {
    match code {
        "catalog_head_unreadable"
        | "publication_unreadable"
        | "table_metadata_unavailable"
        | "manifest_list_unavailable"
        | "manifest_unavailable" => VerifyStatus::Incomplete,
        _ => VerifyStatus::Invalid,
    }
}

async fn verify_entries(
    operator: &Operator,
    workspace_path: &str,
) -> Result<(usize, usize, Vec<Value>)> {
    let (entry_count, revision_count, assets) =
        crate::entry::verify_history_integrity(operator, workspace_path).await?;
    Ok((entry_count, revision_count, assets))
}

async fn verify_assets(
    operator: &Operator,
    workspace_path: &str,
    references: &[Value],
    deep: bool,
) -> Result<usize> {
    let mut seen = std::collections::BTreeMap::new();
    let metadata_path = format!("{}/meta.json", workspace_path.trim_end_matches('/'));
    let workspace = if operator.exists(&metadata_path).await? {
        Some(crate::iceberg_store::native_workspace_read_only(operator, workspace_path).await?)
    } else {
        None
    };
    for value in references {
        let reference: ugoite_domain::entry::AssetReference =
            serde_json::from_value(serde_json::json!({
                "asset_id": value.get("asset_id"),
                "name": value.get("name"),
                "media_type": value.get("media_type"),
                "size_bytes": value.get("size_bytes"),
                "sha256": value.get("sha256"),
            }))
            .context("decode Asset reference")?;
        reference.validate().context("validate Asset reference")?;
        if let Some((size_bytes, sha256)) = seen.get(&reference.asset_id) {
            if *size_bytes != reference.size_bytes || sha256 != &reference.sha256 {
                anyhow::bail!("Asset {} has conflicting references", reference.asset_id);
            }
            continue;
        }
        seen.insert(
            reference.asset_id.clone(),
            (reference.size_bytes, reference.sha256.clone()),
        );
        let path = if let Some(workspace) = &workspace {
            crate::asset::published_object_path(workspace, workspace_path, &reference.asset_id)
                .await?
        } else {
            format!("{workspace_path}/assets/{}", reference.asset_id)
        };
        let metadata = operator
            .stat(&path)
            .await
            .context("stat referenced Asset")?;
        if metadata.content_length() != reference.size_bytes {
            anyhow::bail!(
                "Asset {} size does not match its reference",
                reference.asset_id
            );
        }
        if deep {
            let bytes = operator
                .read(&path)
                .await
                .context("read referenced Asset")?;
            let digest = hex::encode(Sha256::digest(bytes.to_bytes()));
            if digest != reference.sha256 {
                anyhow::bail!(
                    "Asset {} SHA-256 does not match its reference",
                    reference.asset_id
                );
            }
        }
    }
    Ok(seen.len())
}

#[cfg(test)]
mod tests {
    use super::{health_issue_status, VerifyStatus};

    #[test]
    fn unavailable_catalog_evidence_is_incomplete() {
        for code in [
            "catalog_head_unreadable",
            "publication_unreadable",
            "table_metadata_unavailable",
            "manifest_list_unavailable",
            "manifest_unavailable",
        ] {
            assert_eq!(
                health_issue_status(code),
                VerifyStatus::Incomplete,
                "{code}"
            );
        }
    }

    #[test]
    fn corrupt_catalog_evidence_is_invalid() {
        for code in [
            "catalog_head_missing",
            "publication_chain_corrupt",
            "publication_change_invalid",
            "table_metadata_invalid",
            "table_uuid_mismatch",
        ] {
            assert_eq!(health_issue_status(code), VerifyStatus::Invalid, "{code}");
        }
    }
}
