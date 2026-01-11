use pyo3::prelude::*;

#[pyfunction]
pub fn create_note() -> PyResult<String> {
    Ok("mock: create_note".to_string())
}

#[pyfunction]
pub fn list_notes() -> PyResult<Vec<String>> {
    Ok(vec!["mock_note".to_string()])
}

#[pyfunction]
pub fn get_note() -> PyResult<String> {
    Ok("mock: get_note".to_string())
}

#[pyfunction]
pub fn update_note() -> PyResult<String> {
    Ok("mock: update_note".to_string())
}

#[pyfunction]
pub fn delete_note() -> PyResult<String> {
    Ok("mock: delete_note".to_string())
}

#[pyfunction]
pub fn get_note_history() -> PyResult<Vec<String>> {
    Ok(vec!["mock_history".to_string()])
}

#[pyfunction]
pub fn get_note_revision() -> PyResult<String> {
    Ok("mock: get_note_revision".to_string())
}

#[pyfunction]
pub fn restore_note() -> PyResult<String> {
    Ok("mock: restore_note".to_string())
}
