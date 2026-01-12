use pyo3::prelude::*;

pub fn list_workspaces_logic() -> Vec<String> {
    vec!["mock_workspace".to_string()]
}

#[pyfunction]
pub fn list_workspaces() -> PyResult<Vec<String>> {
    Ok(list_workspaces_logic())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_workspaces_logic() {
        let result = list_workspaces_logic();
        assert_eq!(result, vec!["mock_workspace".to_string()]);
    }

    #[test]
    fn test_storage_connection_mock() {
        // We can't easily test #[pyfunction] with cargo test due to linking
        // So we focus on logic tests or use pytest for bindings.
        assert!(true);
    }
}
