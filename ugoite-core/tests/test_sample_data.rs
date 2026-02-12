mod common;

use _ugoite_core::entry;
use _ugoite_core::sample_data::{
    create_sample_space, create_sample_space_job, get_sample_space_job, list_sample_scenarios,
    SampleDataOptions, SampleJobStatus,
};
use common::setup_operator;
use tokio::time::{sleep, Duration};

#[tokio::test]
/// REQ-API-009
async fn test_sample_data_req_api_009_create_sample_space() -> anyhow::Result<()> {
    let op = setup_operator()?;
    let options = SampleDataOptions {
        space_id: "sample-space".to_string(),
        scenario: "renewable-ops".to_string(),
        entry_count: 120,
        seed: Some(7),
    };

    let summary = create_sample_space(&op, "/tmp", &options).await?;
    assert_eq!(summary.space_id, "sample-space");
    assert_eq!(summary.entry_count, 120);
    assert!(summary.form_count >= 3 && summary.form_count <= 6);
    assert_eq!(summary.forms.len(), summary.form_count);

    let entries = entry::list_entries(&op, "spaces/sample-space").await?;
    assert_eq!(entries.len(), 120);

    Ok(())
}

#[test]
/// REQ-API-010
fn test_sample_data_req_api_010_list_scenarios() -> anyhow::Result<()> {
    let scenarios = list_sample_scenarios();
    assert!(scenarios.len() >= 6);
    let ids: Vec<String> = scenarios.into_iter().map(|s| s.id).collect();
    assert!(ids.contains(&"renewable-ops".to_string()));
    assert!(ids.contains(&"supply-chain".to_string()));
    Ok(())
}

#[tokio::test]
/// REQ-API-010
async fn test_sample_data_req_api_010_job_lifecycle() -> anyhow::Result<()> {
    let op = setup_operator()?;
    let options = SampleDataOptions {
        space_id: "sample-job".to_string(),
        scenario: "renewable-ops".to_string(),
        entry_count: 100,
        seed: Some(10),
    };

    let job = create_sample_space_job(&op, "/tmp", &options).await?;
    assert_eq!(job.status, SampleJobStatus::Queued);

    let mut attempts = 0;
    loop {
        let latest = get_sample_space_job(&op, &job.job_id).await?;
        match latest.status {
            SampleJobStatus::Completed => {
                let summary = latest.summary.expect("summary missing");
                assert_eq!(summary.space_id, "sample-job");
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
