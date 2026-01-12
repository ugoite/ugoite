use anyhow::Result;
use opendal::Operator;

pub async fn query_index(_op: &Operator, _ws_path: &str, _query: &str) -> Result<String> {
    Ok("".to_string())
}

pub async fn reindex_all(_op: &Operator, _ws_path: &str) -> Result<()> {
    Ok(())
}
