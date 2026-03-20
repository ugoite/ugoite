mod common;

use _ugoite_core::entry;
use _ugoite_core::sample_data::{
    create_sample_space, create_sample_space_job, get_sample_space_job, list_sample_scenarios,
    SampleDataOptions, SampleJobStatus,
};
use common::setup_operator;
use tempfile::TempDir;
use tokio::time::{sleep, Duration};
use uuid::Uuid;

/// REQ-API-009
#[tokio::test]
async fn test_sample_data_req_api_009_create_sample_space() -> anyhow::Result<()> {
    let tempdir = tempfile::tempdir()?;
    let root_uri = temp_root_uri(&tempdir);
    let space_id = unique_space_id("sample-space");
    let op = setup_operator()?;
    let options = SampleDataOptions {
        space_id: space_id.clone(),
        scenario: "renewable-ops".to_string(),
        entry_count: 120,
        seed: Some(7),
    };

    let summary = create_sample_space(&op, &root_uri, &options).await?;
    assert_eq!(summary.space_id, space_id);
    assert_eq!(summary.entry_count, 120);
    assert!(summary.form_count >= 3 && summary.form_count <= 6);
    assert_eq!(summary.forms.len(), summary.form_count);

    let entries = entry::list_entries(&op, &format!("spaces/{}", options.space_id)).await?;
    assert_eq!(entries.len(), 120);

    Ok(())
}

/// REQ-API-010
#[test]
fn test_sample_data_req_api_010_list_scenarios() -> anyhow::Result<()> {
    let scenarios = list_sample_scenarios();
    assert!(scenarios.len() >= 6);
    let ids: Vec<String> = scenarios.into_iter().map(|s| s.id).collect();
    assert!(ids.contains(&"renewable-ops".to_string()));
    assert!(ids.contains(&"supply-chain".to_string()));
    Ok(())
}

/// REQ-API-009
#[tokio::test]
async fn test_sample_data_req_api_009_respects_requested_small_entry_count() -> anyhow::Result<()> {
    let tempdir = tempfile::tempdir()?;
    let root_uri = temp_root_uri(&tempdir);
    let space_id = unique_space_id("sample-space-small");
    let op = setup_operator()?;
    let options = SampleDataOptions {
        space_id: space_id.clone(),
        scenario: "lab-qa".to_string(),
        entry_count: 6,
        seed: Some(9),
    };

    let summary = create_sample_space(&op, &root_uri, &options).await?;
    assert_eq!(summary.space_id, space_id);
    assert_eq!(summary.entry_count, 6);

    let entries = entry::list_entries(&op, &format!("spaces/{}", options.space_id)).await?;
    assert_eq!(entries.len(), 6);

    Ok(())
}

/// REQ-API-010
#[tokio::test]
async fn test_sample_data_req_api_010_job_lifecycle() -> anyhow::Result<()> {
    let tempdir = tempfile::tempdir()?;
    let root_uri = temp_root_uri(&tempdir);
    let space_id = unique_space_id("sample-job");
    let op = setup_operator()?;
    let options = SampleDataOptions {
        space_id: space_id.clone(),
        scenario: "renewable-ops".to_string(),
        entry_count: 100,
        seed: Some(10),
    };

    let job = create_sample_space_job(&op, &root_uri, &options).await?;
    assert_eq!(job.status, SampleJobStatus::Queued);

    let mut attempts = 0;
    loop {
        let latest = get_sample_space_job(&op, &job.job_id).await?;
        match latest.status {
            SampleJobStatus::Completed => {
                let summary = latest.summary.expect("summary missing");
                assert_eq!(summary.space_id, space_id);
                assert_eq!(summary.entry_count, 100);
                break;
            }
            SampleJobStatus::Failed => {
                panic!("Sample job failed: {}", latest.error.unwrap_or_default());
            }
            _ => {
                if attempts > 600 {
                    panic!("Sample job did not finish in time");
                }
                attempts += 1;
                sleep(Duration::from_millis(100)).await;
            }
        }
    }

    Ok(())
}

fn temp_root_uri(tempdir: &TempDir) -> String {
    format!("file://{}/", tempdir.path().display())
}

fn unique_space_id(prefix: &str) -> String {
    format!("{prefix}-{}", Uuid::new_v4())
}
