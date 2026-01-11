use pyo3::prelude::*;

#[pyfunction]
pub fn list_workspaces() -> PyResult<Vec<String>> {
    Ok(vec!["mock_workspace".to_string()])
}

#[pyfunction]
pub fn create_workspace() -> PyResult<String> {
    Ok("mock: create_workspace".to_string())
}

#[pyfunction]
pub fn get_workspace() -> PyResult<String> {
    Ok("mock: get_workspace".to_string())
}

#[pyfunction]
pub fn patch_workspace() -> PyResult<String> {
    Ok("mock: patch_workspace".to_string())
}

#[pyfunction]
pub fn test_storage_connection() -> PyResult<bool> {
    Ok(true)
}
