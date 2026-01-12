use pyo3::prelude::*;

#[pyfunction]
pub fn list_classes() -> PyResult<Vec<String>> {
    Ok(vec!["mock_class".to_string()])
}

#[pyfunction]
pub fn list_column_types() -> PyResult<Vec<String>> {
    Ok(vec!["string".to_string(), "number".to_string()])
}

#[pyfunction]
pub fn get_class() -> PyResult<String> {
    Ok("mock: get_class".to_string())
}

#[pyfunction]
pub fn upsert_class() -> PyResult<String> {
    Ok("mock: upsert_class".to_string())
}
