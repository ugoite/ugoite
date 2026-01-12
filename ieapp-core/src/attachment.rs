use anyhow::Result;
use opendal::Operator;

pub async fn save_attachment(
    _op: &Operator,
    _ws_path: &str,
    _filename: &str,
    _content: &[u8],
) -> Result<()> {
    Ok(())
}

pub async fn list_attachments(_op: &Operator, _ws_path: &str) -> Result<Vec<String>> {
    Ok(vec![])
}

pub async fn delete_attachment(_op: &Operator, _ws_path: &str, _filename: &str) -> Result<()> {
    Ok(())
}
