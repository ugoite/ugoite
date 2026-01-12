#![warn(warnings)]
#![deny(clippy::all)]

use opendal::Operator;
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};
use pyo3::IntoPyObjectExt;
use serde_json::Value;

pub mod attachment;
pub mod class;
pub mod index;
pub mod integrity;
pub mod link;
pub mod note;
pub mod sandbox;
pub mod search;
pub mod workspace;

use integrity::FakeIntegrityProvider;

// --- Helpers ---

fn get_operator(_py: Python<'_>, config: &Bound<'_, PyDict>) -> PyResult<Operator> {
    let uri = config
        .get_item("uri")?
        .ok_or_else(|| PyValueError::new_err("Missing 'uri' in storage config"))?
        .extract::<String>()?;

    Operator::from_uri(uri).map_err(|e| PyValueError::new_err(e.to_string()))
}

fn json_to_py(py: Python<'_>, value: Value) -> PyResult<PyObject> {
    match value {
        Value::Null => Ok(py.None()),
        Value::Bool(b) => b.into_py_any(py),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i.into_py_any(py)
            } else if let Some(f) = n.as_f64() {
                f.into_py_any(py)
            } else {
                n.to_string().into_py_any(py)
            }
        }
        Value::String(s) => s.into_py_any(py),
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
    let uri: String = storage_config
        .get_item("uri")?
        .ok_or_else(|| PyValueError::new_err("Missing 'uri'"))?
        .extract()?;
    let op = get_operator(py, &storage_config)?;
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        workspace::create_workspace(&op, &name, &uri)
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
#[pyo3(signature = (storage_config, workspace_id, note_id, content, author=None))]
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

// Search

#[pyfunction]
fn search_notes<'a>(
    py: Python<'a>,
    storage_config: Bound<'a, PyDict>,
    workspace_id: String,
    query: String,
) -> PyResult<Bound<'a, PyAny>> {
    let op = get_operator(py, &storage_config)?;
    let ws_path = format!("workspaces/{}", workspace_id);
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let results = search::search_notes(&op, &ws_path, &query)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;

        // Return list of dicts
        let val =
            serde_json::to_value(results).map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        Python::with_gil(|py| json_to_py(py, val))
    })
}

// Links

#[pyfunction]
fn create_link<'a>(
    py: Python<'a>,
    storage_config: Bound<'a, PyDict>,
    workspace_id: String,
    source: String,
    target: String,
    kind: String,
    link_id: String,
) -> PyResult<Bound<'a, PyAny>> {
    let op = get_operator(py, &storage_config)?;
    let ws_path = format!("workspaces/{}", workspace_id);
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let link = link::create_link(&op, &ws_path, &source, &target, &kind, &link_id)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        let val = serde_json::to_value(link).map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        Python::with_gil(|py| json_to_py(py, val))
    })
}

#[pyfunction]
fn list_links<'a>(
    py: Python<'a>,
    storage_config: Bound<'a, PyDict>,
    workspace_id: String,
) -> PyResult<Bound<'a, PyAny>> {
    let op = get_operator(py, &storage_config)?;
    let ws_path = format!("workspaces/{}", workspace_id);
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let links = link::list_links(&op, &ws_path)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        let val =
            serde_json::to_value(links).map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        Python::with_gil(|py| json_to_py(py, val))
    })
}

#[pyfunction]
fn delete_link<'a>(
    py: Python<'a>,
    storage_config: Bound<'a, PyDict>,
    workspace_id: String,
    link_id: String,
) -> PyResult<Bound<'a, PyAny>> {
    let op = get_operator(py, &storage_config)?;
    let ws_path = format!("workspaces/{}", workspace_id);
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        link::delete_link(&op, &ws_path, &link_id)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        Ok(())
    })
}

#[pyfunction]
fn delete_note<'a>(
    py: Python<'a>,
    storage_config: Bound<'a, PyDict>,
    workspace_id: String,
    note_id: String,
) -> PyResult<Bound<'a, PyAny>> {
    let op = get_operator(py, &storage_config)?;
    let ws_path = format!("workspaces/{}", workspace_id);
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        note::delete_note(&op, &ws_path, &note_id)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        Ok(())
    })
}

#[pyfunction]
fn get_note<'a>(
    py: Python<'a>,
    storage_config: Bound<'a, PyDict>,
    workspace_id: String,
    note_id: String,
) -> PyResult<Bound<'a, PyAny>> {
    let op = get_operator(py, &storage_config)?;
    let ws_path = format!("workspaces/{}", workspace_id);
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let meta = note::get_note(&op, &ws_path, &note_id)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        let val = serde_json::to_value(meta).map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        Python::with_gil(|py| json_to_py(py, val))
    })
}

#[pyfunction]
fn list_notes<'a>(
    py: Python<'a>,
    storage_config: Bound<'a, PyDict>,
    workspace_id: String,
) -> PyResult<Bound<'a, PyAny>> {
    let op = get_operator(py, &storage_config)?;
    let ws_path = format!("workspaces/{}", workspace_id);
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let notes = note::list_notes(&op, &ws_path)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        Ok(notes)
    })
}

#[pyfunction]
fn get_workspace<'a>(
    py: Python<'a>,
    storage_config: Bound<'a, PyDict>,
    name: String,
) -> PyResult<Bound<'a, PyAny>> {
    let op = get_operator(py, &storage_config)?;
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let meta = workspace::get_workspace(&op, &name)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        let val = serde_json::to_value(meta).map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        Python::with_gil(|py| json_to_py(py, val))
    })
}

#[pyfunction]
fn list_column_types<'a>(py: Python<'a>) -> PyResult<Bound<'a, PyAny>> {
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let types = class::list_column_types()
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        Ok(types)
    })
}

#[pyfunction]
fn get_class<'a>(
    py: Python<'a>,
    storage_config: Bound<'a, PyDict>,
    workspace_id: String,
    class_name: String,
) -> PyResult<Bound<'a, PyAny>> {
    let op = get_operator(py, &storage_config)?;
    let ws_path = format!("workspaces/{}", workspace_id);
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let cls = class::get_class(&op, &ws_path, &class_name)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        // Class content is already string (JSON), return as parsed strict dict?
        // Or return as string? Python side might expect dict.
        // class::get_class returns String (legacy).
        // Better to parse it here.
        let val: serde_json::Value =
            serde_json::from_str(&cls).map_err(|e| PyValueError::new_err(e.to_string()))?;
        let val_obj =
            serde_json::to_value(val).map_err(|e| PyRuntimeError::new_err(e.to_string()))?; // Already value, but okay.
        Python::with_gil(|py| json_to_py(py, val_obj))
    })
}

#[pyfunction]
#[pyo3(signature = (storage_config, workspace_id, note_id, content, parent_revision_id=None, author=None))]
fn update_note<'a>(
    py: Python<'a>,
    storage_config: Bound<'a, PyDict>,
    workspace_id: String,
    note_id: String,
    content: String,
    parent_revision_id: Option<String>,
    author: Option<String>,
) -> PyResult<Bound<'a, PyAny>> {
    let op = get_operator(py, &storage_config)?;
    let ws_path = format!("workspaces/{}", workspace_id);
    let author = author.unwrap_or_else(|| "unknown".to_string());

    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let integrity = FakeIntegrityProvider;
        // note::update_note takes generic I: IntegrityProvider.
        // We need to move integrity inside async block?
        // integrity is zero-sized, so it's Copy/Clone.
        let meta = note::update_note(
            &op,
            &ws_path,
            &note_id,
            &content,
            parent_revision_id.as_deref(),
            &author,
            &integrity,
        )
        .await
        .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        let val = serde_json::to_value(meta).map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        Python::with_gil(|py| json_to_py(py, val))
    })
}

#[pyfunction]
fn query_index<'a>(
    py: Python<'a>,
    storage_config: Bound<'a, PyDict>,
    workspace_id: String,
    query: String,
) -> PyResult<Bound<'a, PyAny>> {
    let op = get_operator(py, &storage_config)?;
    let ws_path = format!("workspaces/{}", workspace_id);
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let res = index::query_index(&op, &ws_path, &query)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        let val: serde_json::Value =
            serde_json::from_str(&res).map_err(|e| PyValueError::new_err(e.to_string()))?;
        Python::with_gil(|py| json_to_py(py, val))
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

make_stub!(get_note_history);
make_stub!(get_note_revision);
make_stub!(patch_workspace);
make_stub!(restore_note);

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

    m.add_function(wrap_pyfunction!(create_link, m)?)?;
    m.add_function(wrap_pyfunction!(list_links, m)?)?;
    m.add_function(wrap_pyfunction!(delete_link, m)?)?;
    m.add_function(wrap_pyfunction!(search_notes, m)?)?;

    Ok(())
}
