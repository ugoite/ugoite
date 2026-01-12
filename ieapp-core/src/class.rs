use anyhow::Result;
use opendal::Operator;

pub async fn list_classes(_op: &Operator, _ws_path: &str) -> Result<Vec<String>> {
    Ok(vec![])
}

pub async fn list_column_types() -> Result<Vec<String>> {
    Ok(vec!["string".to_string(), "number".to_string()])
}

pub async fn get_class(_op: &Operator, _ws_path: &str, _class_name: &str) -> Result<String> {
    Ok("".to_string())
}

pub async fn upsert_class(_op: &Operator, _ws_path: &str, _class_def: &str) -> Result<()> {
    Ok(())
}
