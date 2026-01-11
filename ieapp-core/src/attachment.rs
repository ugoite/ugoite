use pyo3::prelude::*;

#[pyfunction]
pub fn save_attachment() -> PyResult<String> {
    Ok("mock: save_attachment".to_string())
}

#[pyfunction]
pub fn list_attachments() -> PyResult<Vec<String>> {
    Ok(vec!["mock_attachment".to_string()])
}

#[pyfunction]
pub fn delete_attachment() -> PyResult<String> {
    Ok("mock: delete_attachment".to_string())
}
