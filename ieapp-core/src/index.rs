use pyo3::prelude::*;

#[pyfunction]
pub fn query_index() -> PyResult<String> {
    Ok("mock: query_index".to_string())
}
