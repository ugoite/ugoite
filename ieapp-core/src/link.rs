use pyo3::prelude::*;

#[pyfunction]
pub fn create_link() -> PyResult<String> {
    Ok("mock: create_link".to_string())
}

#[pyfunction]
pub fn list_links() -> PyResult<Vec<String>> {
    Ok(vec!["mock_link".to_string()])
}

#[pyfunction]
pub fn delete_link() -> PyResult<String> {
    Ok("mock: delete_link".to_string())
}
