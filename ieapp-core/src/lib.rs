#![warn(warnings)]
#![deny(clippy::all)]

use opendal::Operator;
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};
use pyo3::ToPyObject;
use serde_json::Value;

pub mod attachment;
pub mod class;
pub mod index;
pub mod integrity;
pub mod link;
pub mod note;
pub mod sandbox;
pub mod search;
pub mod storage;
pub mod workspace;

use integrity::FakeIntegrityProvider;

// --- Helpers ---

fn get_operator(_py: Python<'_>, config: &Bound<'_, PyDict>) -> PyResult<Operator> {
    let uri = config
        .get_item("uri")?
        .ok_or_else(|| PyValueError::new_err("Missing 'uri' in storage config"))?
        .extract::<String>()?;

    storage::opendal::create_operator_from_uri(&uri)
        .map_err(|e: anyhow::Error| PyValueError::new_err(e.to_string()))
}

fn json_to_py(py: Python<'_>, value: Value) -> PyResult<PyObject> {
    match value {
        Value::Null => Ok(py.None()),
        Value::Bool(b) => Ok(b.to_object(py)),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(i.to_object(py))
            } else if let Some(f) = n.as_f64() {
                Ok(f.to_object(py))
            } else {
                Ok(n.to_string().to_object(py))
            }
        }
        Value::String(s) => Ok(s.to_object(py)),
        Value::Array(arr) => {
            let list = PyList::empty(py);
            for item in arr {
                list.append(json_to_py(py, item)?)?;
            }
            Ok(list.into())
        }
        Value::Object(map) => {
            let dict = PyDict::new(py);
            for (k, v) in map {
                dict.set_item(k, json_to_py(py, v)?)?;
            }
            Ok(dict.into())
        }
    }
}

// --- Bindings ---

// Workspace

#[pyfunction]
fn list_workspaces<'a>(
    py: Python<'a>,
    storage_config: Bound<'a, PyDict>,
) -> PyResult<Bound<'a, PyAny>> {
    let op = get_operator(py, &storage_config)?;
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let workspaces = workspace::list_workspaces(&op)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        Ok(workspaces)
    })
}

#[pyfunction]
fn create_workspace<'a>(
    py: Python<'a>,
    storage_config: Bound<'a, PyDict>,
    name: String,
) -> PyResult<Bound<'a, PyAny>> {
    let op = get_operator(py, &storage_config)?;
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        workspace::create_workspace(&op, &name)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        Ok(())
    })
}

#[pyfunction]
#[pyo3(name = "test_storage_connection")]
fn test_storage_connection_py<'a>(
    py: Python<'a>,
    storage_config: Bound<'a, PyDict>,
) -> PyResult<Bound<'a, PyAny>> {
    let _ = get_operator(py, &storage_config)?;
    pyo3_async_runtimes::tokio::future_into_py(py, async move { Ok(true) })
}

// Note

#[pyfunction]
fn create_note<'a>(
    py: Python<'a>,
    storage_config: Bound<'a, PyDict>,
    workspace_id: String,
    note_id: String,
    content: String,
    author: Option<String>,
) -> PyResult<Bound<'a, PyAny>> {
    let op = get_operator(py, &storage_config)?;
    let ws_path = format!("workspaces/{}", workspace_id);
    let author = author.unwrap_or_else(|| "unknown".to_string());

    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let integrity = FakeIntegrityProvider;
        let meta = note::create_note(&op, &ws_path, &note_id, &content, &author, &integrity)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;

        let val = serde_json::to_value(meta).map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        Python::with_gil(|py| json_to_py(py, val))
    })
}

// Class

#[pyfunction]
fn list_classes<'a>(
    py: Python<'a>,
    storage_config: Bound<'a, PyDict>,
    workspace_id: String,
) -> PyResult<Bound<'a, PyAny>> {
    let op = get_operator(py, &storage_config)?;
    let ws_path = format!("workspaces/{}", workspace_id);
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let classes = class::list_classes(&op, &ws_path)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        Ok(classes)
    })
}

#[pyfunction]
fn upsert_class<'a>(
    py: Python<'a>,
    storage_config: Bound<'a, PyDict>,
    workspace_id: String,
    class_def: String,
) -> PyResult<Bound<'a, PyAny>> {
    let op = get_operator(py, &storage_config)?;
    let ws_path = format!("workspaces/{}", workspace_id);
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        class::upsert_class(&op, &ws_path, &class_def)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        Ok(())
    })
}

// Attachment

#[pyfunction]
fn save_attachment<'a>(
    py: Python<'a>,
    storage_config: Bound<'a, PyDict>,
    workspace_id: String,
    filename: String,
    content: Vec<u8>,
) -> PyResult<Bound<'a, PyAny>> {
    let op = get_operator(py, &storage_config)?;
    let ws_path = format!("workspaces/{}", workspace_id);
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        attachment::save_attachment(&op, &ws_path, &filename, &content)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        Ok(())
    })
}

#[pyfunction]
fn list_attachments<'a>(
    py: Python<'a>,
    storage_config: Bound<'a, PyDict>,
    workspace_id: String,
) -> PyResult<Bound<'a, PyAny>> {
    let op = get_operator(py, &storage_config)?;
    let ws_path = format!("workspaces/{}", workspace_id);
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let list = attachment::list_attachments(&op, &ws_path)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        Ok(list)
    })
}

#[pyfunction]
fn delete_attachment<'a>(
    py: Python<'a>,
    storage_config: Bound<'a, PyDict>,
    workspace_id: String,
    filename: String,
) -> PyResult<Bound<'a, PyAny>> {
    let op = get_operator(py, &storage_config)?;
    let ws_path = format!("workspaces/{}", workspace_id);
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        attachment::delete_attachment(&op, &ws_path, &filename)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        Ok(())
    })
}

// Stubs using generic signature
macro_rules! make_stub {
    ($name:ident) => {
        #[pyfunction]
        #[pyo3(signature = (*_args, **_kwargs))]
        fn $name(
            _args: &Bound<'_, pyo3::types::PyTuple>,
            _kwargs: Option<&Bound<'_, PyDict>>,
        ) -> PyResult<()> {
            Err(PyRuntimeError::new_err(format!(
                "Mock: {} not implemented",
                stringify!($name)
            )))
        }
    };
}

make_stub!(delete_note);
make_stub!(get_class);
make_stub!(get_note);
make_stub!(get_note_history);
make_stub!(get_note_revision);
make_stub!(get_workspace);
make_stub!(list_column_types);
make_stub!(list_notes);
make_stub!(patch_workspace);
make_stub!(query_index);
make_stub!(restore_note);
make_stub!(update_note);

/// A Python module implemented in Rust.
#[pymodule]
fn _ieapp_core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(list_workspaces, m)?)?;
    m.add_function(wrap_pyfunction!(create_workspace, m)?)?;
    m.add_function(wrap_pyfunction!(test_storage_connection_py, m)?)?;

    m.add_function(wrap_pyfunction!(create_note, m)?)?;
    m.add_function(wrap_pyfunction!(delete_note, m)?)?;
    m.add_function(wrap_pyfunction!(get_note, m)?)?;
    m.add_function(wrap_pyfunction!(get_note_history, m)?)?;
    m.add_function(wrap_pyfunction!(get_note_revision, m)?)?;
    m.add_function(wrap_pyfunction!(list_notes, m)?)?;
    m.add_function(wrap_pyfunction!(restore_note, m)?)?;
    m.add_function(wrap_pyfunction!(update_note, m)?)?;

    m.add_function(wrap_pyfunction!(list_classes, m)?)?;
    m.add_function(wrap_pyfunction!(upsert_class, m)?)?;
    m.add_function(wrap_pyfunction!(get_class, m)?)?;
    m.add_function(wrap_pyfunction!(list_column_types, m)?)?;

    m.add_function(wrap_pyfunction!(save_attachment, m)?)?;
    m.add_function(wrap_pyfunction!(list_attachments, m)?)?;
    m.add_function(wrap_pyfunction!(delete_attachment, m)?)?;

    m.add_function(wrap_pyfunction!(get_workspace, m)?)?;
    m.add_function(wrap_pyfunction!(patch_workspace, m)?)?;

    m.add_function(wrap_pyfunction!(query_index, m)?)?;

    // Keep mocks for others
    m.add_function(wrap_pyfunction!(link::create_link, m)?)?;
    m.add_function(wrap_pyfunction!(link::list_links, m)?)?;
    m.add_function(wrap_pyfunction!(link::delete_link, m)?)?;
    m.add_function(wrap_pyfunction!(search::search_notes, m)?)?;

    Ok(())
}
