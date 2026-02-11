mod common;

use _ieapp_core::entry;
use _ieapp_core::sample_data::{create_sample_space, SampleDataOptions};
use common::setup_operator;

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
