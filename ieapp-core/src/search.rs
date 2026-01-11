use pyo3::prelude::*;

#[pyfunction]
pub fn search_notes() -> PyResult<String> {
    Ok("mock: search_notes".to_string())
}
