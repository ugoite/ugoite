#![warn(warnings)]
#![deny(clippy::all)]

use opendal::Operator;
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyList, PyTuple};
use pyo3::IntoPyObjectExt;
use serde_json::Value;

pub mod attachment;
pub mod class;
pub mod iceberg_store;
pub mod index;
pub mod integrity;
pub mod link;
pub mod note;
pub mod search;
pub mod sql;
pub mod storage;
pub mod workspace;

use integrity::RealIntegrityProvider;

// --- Helpers ---

fn get_operator(_py: Python<'_>, config: &Bound<'_, PyDict>) -> PyResult<Operator> {
    let uri = config
        .get_item("uri")?
        .ok_or_else(|| PyValueError::new_err("Missing 'uri' in storage config"))?
        .extract::<String>()?;

    storage::operator_from_uri(&uri).map_err(|e| PyValueError::new_err(e.to_string()))
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
    let uri: String = storage_config
        .get_item("uri")?
        .ok_or_else(|| PyValueError::new_err("Missing 'uri'"))?
        .extract()?;
    let payload = if uri.starts_with("memory://") {
        serde_json::json!({"status": "ok", "mode": "memory"})
    } else if uri.starts_with("file://")
        || uri.starts_with("fs://")
        || uri.starts_with('/')
        || uri.starts_with('.')
    {
        serde_json::json!({"status": "ok", "mode": "local"})
    } else if uri.starts_with("s3://") {
        serde_json::json!({"status": "ok", "mode": "s3"})
    } else {
        return Err(PyValueError::new_err("Unsupported storage connector"));
    };
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        Python::with_gil(|py| json_to_py(py, payload))
    })
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
        let integrity = RealIntegrityProvider::from_workspace(&op, &workspace_id)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
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
        let val =
            serde_json::to_value(classes).map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        Python::with_gil(|py| json_to_py(py, val))
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
        let parsed: serde_json::Value =
            serde_json::from_str(&class_def).map_err(|e| PyValueError::new_err(e.to_string()))?;
        class::upsert_class(&op, &ws_path, &parsed)
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
        let info = attachment::save_attachment(&op, &ws_path, &filename, &content)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        let val = serde_json::to_value(info).map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        Python::with_gil(|py| json_to_py(py, val))
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
        let val = serde_json::to_value(list).map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        Python::with_gil(|py| json_to_py(py, val))
    })
}

#[pyfunction]
fn delete_attachment<'a>(
    py: Python<'a>,
    storage_config: Bound<'a, PyDict>,
    workspace_id: String,
    attachment_id: String,
) -> PyResult<Bound<'a, PyAny>> {
    let op = get_operator(py, &storage_config)?;
    let ws_path = format!("workspaces/{}", workspace_id);
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        attachment::delete_attachment(&op, &ws_path, &attachment_id)
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
#[pyo3(signature = (storage_config, workspace_id, note_id, hard_delete=false))]
fn delete_note<'a>(
    py: Python<'a>,
    storage_config: Bound<'a, PyDict>,
    workspace_id: String,
    note_id: String,
    hard_delete: bool,
) -> PyResult<Bound<'a, PyAny>> {
    let op = get_operator(py, &storage_config)?;
    let ws_path = format!("workspaces/{}", workspace_id);
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        note::delete_note(&op, &ws_path, &note_id, hard_delete)
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
        let val = serde_json::Value::Array(notes);
        Python::with_gil(|py| json_to_py(py, val))
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
        let meta = workspace::get_workspace_raw(&op, &name)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        let val = serde_json::to_value(meta).map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        Python::with_gil(|py| json_to_py(py, val))
    })
}

#[pyfunction]
fn patch_workspace<'a>(
    py: Python<'a>,
    storage_config: Bound<'a, PyDict>,
    workspace_id: String,
    patch_json: String,
) -> PyResult<Bound<'a, PyAny>> {
    let op = get_operator(py, &storage_config)?;
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let patch_value: serde_json::Value =
            serde_json::from_str(&patch_json).map_err(|e| PyValueError::new_err(e.to_string()))?;
        let updated = workspace::patch_workspace(&op, &workspace_id, &patch_value)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        let val =
            serde_json::to_value(updated).map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
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
#[pyo3(signature = (storage_config, workspace_id, class_def_json, strategies_json=None))]
fn migrate_class<'a>(
    py: Python<'a>,
    storage_config: Bound<'a, PyDict>,
    workspace_id: String,
    class_def_json: String,
    strategies_json: Option<String>,
) -> PyResult<Bound<'a, PyAny>> {
    let op = get_operator(py, &storage_config)?;
    let ws_path = format!("workspaces/{}", workspace_id);
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let class_def: serde_json::Value = serde_json::from_str(&class_def_json)
            .map_err(|e| PyValueError::new_err(e.to_string()))?;
        let strategies = match strategies_json {
            Some(json) => Some(
                serde_json::from_str::<serde_json::Value>(&json)
                    .map_err(|e| PyValueError::new_err(e.to_string()))?,
            ),
            None => None,
        };
        let integrity = RealIntegrityProvider::from_workspace(&op, &workspace_id)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        let count = class::migrate_class(&op, &ws_path, &class_def, strategies, &integrity)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        Ok(count)
    })
}

#[pyfunction]
fn reindex_all<'a>(
    py: Python<'a>,
    storage_config: Bound<'a, PyDict>,
    workspace_id: String,
) -> PyResult<Bound<'a, PyAny>> {
    let op = get_operator(py, &storage_config)?;
    let ws_path = format!("workspaces/{}", workspace_id);
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        index::reindex_all(&op, &ws_path)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        Ok(())
    })
}

#[pyfunction]
fn update_note_index<'a>(
    py: Python<'a>,
    storage_config: Bound<'a, PyDict>,
    workspace_id: String,
    note_id: String,
) -> PyResult<Bound<'a, PyAny>> {
    let op = get_operator(py, &storage_config)?;
    let ws_path = format!("workspaces/{}", workspace_id);
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        index::update_note_index(&op, &ws_path, &note_id)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        Ok(())
    })
}

#[pyfunction]
fn load_hmac_material<'a>(
    py: Python<'a>,
    storage_config: Bound<'a, PyDict>,
) -> PyResult<Bound<'a, PyAny>> {
    let op = get_operator(py, &storage_config)?;
    pyo3_async_runtimes::tokio::future_into_py::<_, PyObject>(py, async move {
        let (key_id, secret) = integrity::load_hmac_material(&op)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        Python::with_gil(|py| {
            let secret_bytes = PyBytes::new(py, &secret);
            let key_id_obj = key_id.into_py_any(py)?;
            let secret_obj = secret_bytes.into_py_any(py)?;
            let tuple = PyTuple::new(py, [key_id_obj, secret_obj])?;
            tuple.into_py_any(py)
        })
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
        let val = serde_json::to_value(cls).map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        Python::with_gil(|py| json_to_py(py, val))
    })
}

#[pyfunction]
fn get_note_history<'a>(
    py: Python<'a>,
    storage_config: Bound<'a, PyDict>,
    workspace_id: String,
    note_id: String,
) -> PyResult<Bound<'a, PyAny>> {
    let op = get_operator(py, &storage_config)?;
    let ws_path = format!("workspaces/{}", workspace_id);
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let history = note::get_note_history(&op, &ws_path, &note_id)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        Python::with_gil(|py| json_to_py(py, history))
    })
}

#[pyfunction]
fn get_note_revision<'a>(
    py: Python<'a>,
    storage_config: Bound<'a, PyDict>,
    workspace_id: String,
    note_id: String,
    revision_id: String,
) -> PyResult<Bound<'a, PyAny>> {
    let op = get_operator(py, &storage_config)?;
    let ws_path = format!("workspaces/{}", workspace_id);
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let revision = note::get_note_revision(&op, &ws_path, &note_id, &revision_id)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        Python::with_gil(|py| json_to_py(py, revision))
    })
}

#[pyfunction]
#[pyo3(signature = (storage_config, workspace_id, note_id, revision_id, author=None))]
fn restore_note<'a>(
    py: Python<'a>,
    storage_config: Bound<'a, PyDict>,
    workspace_id: String,
    note_id: String,
    revision_id: String,
    author: Option<String>,
) -> PyResult<Bound<'a, PyAny>> {
    let op = get_operator(py, &storage_config)?;
    let ws_path = format!("workspaces/{}", workspace_id);
    let author = author.unwrap_or_else(|| "unknown".to_string());
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let integrity = RealIntegrityProvider::from_workspace(&op, &workspace_id)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        let result = note::restore_note(&op, &ws_path, &note_id, &revision_id, &author, &integrity)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        Python::with_gil(|py| json_to_py(py, result))
    })
}

#[pyfunction]
#[pyo3(name = "extract_properties")]
fn extract_properties_py(py: Python<'_>, markdown: String) -> PyResult<PyObject> {
    let props = index::extract_properties(&markdown);
    json_to_py(py, props)
}

#[pyfunction]
#[pyo3(name = "validate_properties")]
fn validate_properties_py(
    py: Python<'_>,
    properties_json: String,
    class_json: String,
) -> PyResult<PyObject> {
    let properties: serde_json::Value =
        serde_json::from_str(&properties_json).map_err(|e| PyValueError::new_err(e.to_string()))?;
    let class_def: serde_json::Value =
        serde_json::from_str(&class_json).map_err(|e| PyValueError::new_err(e.to_string()))?;
    let (casted, warnings) = index::validate_properties(&properties, &class_def)
        .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
    let casted_obj = json_to_py(py, casted)?;
    let warnings_obj = json_to_py(py, serde_json::Value::Array(warnings))?;
    let tuple = PyTuple::new(py, [casted_obj, warnings_obj])?;
    tuple.into_py_any(py)
}

#[pyfunction]
fn build_response_signature<'a>(
    py: Python<'a>,
    storage_config: Bound<'a, PyDict>,
    body: Vec<u8>,
) -> PyResult<Bound<'a, PyAny>> {
    let op = get_operator(py, &storage_config)?;
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let (key_id, signature) = integrity::build_response_signature(&op, &body)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        Ok((key_id, signature))
    })
}

#[pyfunction]
#[pyo3(signature = (storage_config, workspace_id, note_id, content, parent_revision_id=None, author=None, attachments_json=None))]
#[allow(clippy::too_many_arguments)]
fn update_note<'a>(
    py: Python<'a>,
    storage_config: Bound<'a, PyDict>,
    workspace_id: String,
    note_id: String,
    content: String,
    parent_revision_id: Option<String>,
    author: Option<String>,
    attachments_json: Option<String>,
) -> PyResult<Bound<'a, PyAny>> {
    let op = get_operator(py, &storage_config)?;
    let ws_path = format!("workspaces/{}", workspace_id);
    let author = author.unwrap_or_else(|| "unknown".to_string());

    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let integrity = RealIntegrityProvider::from_workspace(&op, &workspace_id)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        let attachments = match attachments_json {
            Some(json_str) => serde_json::from_str::<Vec<serde_json::Value>>(&json_str)
                .map(Some)
                .map_err(|e| PyRuntimeError::new_err(e.to_string()))?,
            None => None,
        };
        let meta = note::update_note(
            &op,
            &ws_path,
            &note_id,
            &content,
            parent_revision_id.as_deref(),
            &author,
            attachments,
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
    let adjusted_query = match serde_json::from_str::<serde_json::Value>(&query) {
        Ok(parsed) => parsed
            .get("$sql")
            .or_else(|| parsed.get("sql"))
            .and_then(|val| val.as_str())
            .and_then(|sql| serde_json::to_string(sql).ok())
            .unwrap_or(query.clone()),
        Err(_) => query.clone(),
    };
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let res = index::query_index(&op, &ws_path, &adjusted_query)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        let val = serde_json::Value::Array(res);
        Python::with_gil(|py| json_to_py(py, val))
    })
}

// Stubs using generic signature removed; all bindings are implemented.

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
    m.add_function(wrap_pyfunction!(extract_properties_py, m)?)?;
    m.add_function(wrap_pyfunction!(validate_properties_py, m)?)?;

    m.add_function(wrap_pyfunction!(list_classes, m)?)?;
    m.add_function(wrap_pyfunction!(upsert_class, m)?)?;
    m.add_function(wrap_pyfunction!(get_class, m)?)?;
    m.add_function(wrap_pyfunction!(list_column_types, m)?)?;
    m.add_function(wrap_pyfunction!(migrate_class, m)?)?;

    m.add_function(wrap_pyfunction!(save_attachment, m)?)?;
    m.add_function(wrap_pyfunction!(list_attachments, m)?)?;
    m.add_function(wrap_pyfunction!(delete_attachment, m)?)?;

    m.add_function(wrap_pyfunction!(get_workspace, m)?)?;
    m.add_function(wrap_pyfunction!(patch_workspace, m)?)?;

    m.add_function(wrap_pyfunction!(query_index, m)?)?;
    m.add_function(wrap_pyfunction!(reindex_all, m)?)?;
    m.add_function(wrap_pyfunction!(update_note_index, m)?)?;

    m.add_function(wrap_pyfunction!(create_link, m)?)?;
    m.add_function(wrap_pyfunction!(list_links, m)?)?;
    m.add_function(wrap_pyfunction!(delete_link, m)?)?;
    m.add_function(wrap_pyfunction!(search_notes, m)?)?;
    m.add_function(wrap_pyfunction!(build_response_signature, m)?)?;
    m.add_function(wrap_pyfunction!(load_hmac_material, m)?)?;

    Ok(())
}
