mod common;

use _ugoite_core::entry;
use _ugoite_core::sample_data::{
    create_sample_space, create_sample_space_job, get_sample_space_job, list_sample_scenarios,
    SampleDataJob, SampleDataOptions, SampleJobStatus,
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
        owner_user_id: None,
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
        owner_user_id: None,
    };

    let summary = create_sample_space(&op, &root_uri, &options).await?;
    assert_eq!(summary.space_id, space_id);
    assert_eq!(summary.entry_count, 6);

    let entries = entry::list_entries(&op, &format!("spaces/{}", options.space_id)).await?;
    assert_eq!(entries.len(), 6);

    Ok(())
}

/// REQ-OPS-016
#[tokio::test]
async fn test_sample_data_req_ops_016_bootstraps_trimmed_owner_membership() -> anyhow::Result<()> {
    let tempdir = tempfile::tempdir()?;
    let root_uri = temp_root_uri(&tempdir);
    let space_id = unique_space_id("sample-space-owner");
    let op = setup_operator()?;
    let options = SampleDataOptions {
        space_id: space_id.clone(),
        scenario: "renewable-ops".to_string(),
        entry_count: 6,
        seed: Some(11),
        owner_user_id: Some("  local-dev-user  ".to_string()),
    };

    create_sample_space(&op, &root_uri, &options).await?;

    let settings_path = format!("spaces/{space_id}/settings.json");
    let settings_bytes = op.read(&settings_path).await?;
    let settings_text = String::from_utf8(settings_bytes.to_bytes().to_vec())?;
    assert!(
        settings_text.contains('\n'),
        "settings.json should stay pretty-printed after owner bootstrap"
    );
    let settings_json: serde_json::Value = serde_json::from_str(&settings_text)?;

    assert_eq!(
        settings_json["membership_version"].as_i64(),
        Some(1),
        "owner bootstrap should initialize membership_version as an integer"
    );
    let owner_member = &settings_json["members"]["local-dev-user"];
    assert_eq!(owner_member["user_id"].as_str(), Some("local-dev-user"));
    assert_eq!(owner_member["role"].as_str(), Some("admin"));
    assert_eq!(owner_member["state"].as_str(), Some("active"));

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
        owner_user_id: None,
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

/// REQ-API-010
#[tokio::test]
async fn test_sample_data_req_api_010_job_status_retries_transient_eof() -> anyhow::Result<()> {
    let op = setup_operator()?;
    let job_id = Uuid::new_v4().to_string();
    let path = format!("sample_jobs/{job_id}.json");
    op.create_dir("sample_jobs/").await?;
    op.write(&path, Vec::<u8>::new()).await?;

    let delayed_job = SampleDataJob {
        job_id: job_id.clone(),
        space_id: unique_space_id("sample-job-race"),
        scenario: "renewable-ops".to_string(),
        entry_count: 6,
        seed: Some(2),
        owner_user_id: None,
        status: SampleJobStatus::Queued,
        status_message: Some("Queued".to_string()),
        processed_entries: 0,
        total_entries: 6,
        started_at: None,
        completed_at: None,
        error: None,
        summary: None,
    };
    let delayed_job_bytes = serde_json::to_vec_pretty(&delayed_job)?;
    let delayed_op = op.clone();
    let delayed_path = path.clone();
    tokio::spawn(async move {
        sleep(Duration::from_millis(20)).await;
        delayed_op
            .write(&delayed_path, delayed_job_bytes)
            .await
            .expect("write delayed sample job");
    });

    let loaded = get_sample_space_job(&op, &job_id).await?;
    assert_eq!(loaded.job_id, job_id);
    assert_eq!(loaded.status, SampleJobStatus::Queued);
    assert_eq!(loaded.total_entries, 6);

    Ok(())
}

fn temp_root_uri(tempdir: &TempDir) -> String {
    format!("file://{}/", tempdir.path().display())
}

fn unique_space_id(prefix: &str) -> String {
    format!("{prefix}-{}", Uuid::new_v4())
}
