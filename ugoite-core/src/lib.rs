#![warn(warnings)]
#![deny(clippy::all)]

use opendal::Operator;
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyList, PyTuple};
use pyo3::IntoPyObjectExt;
use serde_json::Value;

pub mod asset;
pub mod entry;
pub mod form;
pub mod iceberg_store;
pub mod index;
pub mod integrity;
pub mod link;
pub mod materialized_view;
pub mod metadata;
pub mod sample_data;
pub mod saved_sql;
pub mod search;
pub mod space;
pub mod sql;
pub mod sql_session;
pub mod storage;

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

// Space

#[pyfunction]
fn list_spaces<'a>(
    py: Python<'a>,
    storage_config: Bound<'a, PyDict>,
) -> PyResult<Bound<'a, PyAny>> {
    let op = get_operator(py, &storage_config)?;
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let spaces = space::list_spaces(&op)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        Ok(spaces)
    })
}

#[pyfunction]
fn create_space<'a>(
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
        space::create_space(&op, &name, &uri)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        Ok(())
    })
}

#[pyfunction]
#[pyo3(signature = (storage_config, space_id, scenario=None, entry_count=None, seed=None))]
fn create_sample_space<'a>(
    py: Python<'a>,
    storage_config: Bound<'a, PyDict>,
    space_id: String,
    scenario: Option<String>,
    entry_count: Option<usize>,
    seed: Option<u64>,
) -> PyResult<Bound<'a, PyAny>> {
    let uri: String = storage_config
        .get_item("uri")?
        .ok_or_else(|| PyValueError::new_err("Missing 'uri'"))?
        .extract()?;
    let op = get_operator(py, &storage_config)?;
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let options = sample_data::SampleDataOptions {
            space_id,
            scenario: scenario.unwrap_or_else(|| sample_data::DEFAULT_SCENARIO.to_string()),
            entry_count: entry_count.unwrap_or(sample_data::DEFAULT_ENTRY_COUNT),
            seed,
        };
        let summary = sample_data::create_sample_space(&op, &uri, &options)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        let val =
            serde_json::to_value(summary).map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        Python::with_gil(|py| json_to_py(py, val))
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

// Entry

#[pyfunction]
#[pyo3(signature = (storage_config, space_id, entry_id, content, author=None))]
fn create_entry<'a>(
    py: Python<'a>,
    storage_config: Bound<'a, PyDict>,
    space_id: String,
    entry_id: String,
    content: String,
    author: Option<String>,
) -> PyResult<Bound<'a, PyAny>> {
    let op = get_operator(py, &storage_config)?;
    let ws_path = format!("spaces/{}", space_id);
    let author = author.unwrap_or_else(|| "unknown".to_string());

    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let integrity = RealIntegrityProvider::from_space(&op, &space_id)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        let meta = entry::create_entry(&op, &ws_path, &entry_id, &content, &author, &integrity)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;

        let val = serde_json::to_value(meta).map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        Python::with_gil(|py| json_to_py(py, val))
    })
}

// Saved SQL

#[pyfunction]
fn list_sql<'a>(
    py: Python<'a>,
    storage_config: Bound<'a, PyDict>,
    space_id: String,
) -> PyResult<Bound<'a, PyAny>> {
    let op = get_operator(py, &storage_config)?;
    let ws_path = format!("spaces/{}", space_id);
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let entries = saved_sql::list_sql(&op, &ws_path)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        let val =
            serde_json::to_value(entries).map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        Python::with_gil(|py| json_to_py(py, val))
    })
}

#[pyfunction]
fn get_sql<'a>(
    py: Python<'a>,
    storage_config: Bound<'a, PyDict>,
    space_id: String,
    sql_id: String,
) -> PyResult<Bound<'a, PyAny>> {
    let op = get_operator(py, &storage_config)?;
    let ws_path = format!("spaces/{}", space_id);
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let entry = saved_sql::get_sql(&op, &ws_path, &sql_id)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        Python::with_gil(|py| json_to_py(py, entry))
    })
}

#[pyfunction]
#[pyo3(signature = (storage_config, space_id, sql_id, payload_json, author=None))]
fn create_sql<'a>(
    py: Python<'a>,
    storage_config: Bound<'a, PyDict>,
    space_id: String,
    sql_id: String,
    payload_json: String,
    author: Option<String>,
) -> PyResult<Bound<'a, PyAny>> {
    let op = get_operator(py, &storage_config)?;
    let ws_path = format!("spaces/{}", space_id);
    let author = author.unwrap_or_else(|| "unknown".to_string());
    let payload: saved_sql::SqlPayload =
        serde_json::from_str(&payload_json).map_err(|e| PyValueError::new_err(e.to_string()))?;

    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let integrity = RealIntegrityProvider::from_space(&op, &space_id)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        let entry = saved_sql::create_sql(&op, &ws_path, &sql_id, &payload, &author, &integrity)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        Python::with_gil(|py| json_to_py(py, entry))
    })
}

#[pyfunction]
#[pyo3(signature = (storage_config, space_id, sql_id, payload_json, parent_revision_id=None, author=None))]
fn update_sql<'a>(
    py: Python<'a>,
    storage_config: Bound<'a, PyDict>,
    space_id: String,
    sql_id: String,
    payload_json: String,
    parent_revision_id: Option<String>,
    author: Option<String>,
) -> PyResult<Bound<'a, PyAny>> {
    let op = get_operator(py, &storage_config)?;
    let ws_path = format!("spaces/{}", space_id);
    let author = author.unwrap_or_else(|| "unknown".to_string());
    let payload: saved_sql::SqlPayload =
        serde_json::from_str(&payload_json).map_err(|e| PyValueError::new_err(e.to_string()))?;

    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let integrity = RealIntegrityProvider::from_space(&op, &space_id)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        let entry = saved_sql::update_sql(
            &op,
            &ws_path,
            &sql_id,
            &payload,
            parent_revision_id.as_deref(),
            &author,
            &integrity,
        )
        .await
        .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        Python::with_gil(|py| json_to_py(py, entry))
    })
}

#[pyfunction]
fn delete_sql<'a>(
    py: Python<'a>,
    storage_config: Bound<'a, PyDict>,
    space_id: String,
    sql_id: String,
) -> PyResult<Bound<'a, PyAny>> {
    let op = get_operator(py, &storage_config)?;
    let ws_path = format!("spaces/{}", space_id);
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        saved_sql::delete_sql(&op, &ws_path, &sql_id)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        Ok(())
    })
}

// Search

#[pyfunction]
fn search_entries<'a>(
    py: Python<'a>,
    storage_config: Bound<'a, PyDict>,
    space_id: String,
    query: String,
) -> PyResult<Bound<'a, PyAny>> {
    let op = get_operator(py, &storage_config)?;
    let ws_path = format!("spaces/{}", space_id);
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let results = search::search_entries(&op, &ws_path, &query)
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
    space_id: String,
    source: String,
    target: String,
    kind: String,
    link_id: String,
) -> PyResult<Bound<'a, PyAny>> {
    let op = get_operator(py, &storage_config)?;
    let ws_path = format!("spaces/{}", space_id);
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
    space_id: String,
) -> PyResult<Bound<'a, PyAny>> {
    let op = get_operator(py, &storage_config)?;
    let ws_path = format!("spaces/{}", space_id);
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
    space_id: String,
    link_id: String,
) -> PyResult<Bound<'a, PyAny>> {
    let op = get_operator(py, &storage_config)?;
    let ws_path = format!("spaces/{}", space_id);
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        link::delete_link(&op, &ws_path, &link_id)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        Ok(())
    })
}

#[pyfunction]
#[pyo3(signature = (storage_config, space_id, entry_id, hard_delete=false))]
fn delete_entry<'a>(
    py: Python<'a>,
    storage_config: Bound<'a, PyDict>,
    space_id: String,
    entry_id: String,
    hard_delete: bool,
) -> PyResult<Bound<'a, PyAny>> {
    let op = get_operator(py, &storage_config)?;
    let ws_path = format!("spaces/{}", space_id);
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        entry::delete_entry(&op, &ws_path, &entry_id, hard_delete)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        Ok(())
    })
}

#[pyfunction]
fn get_entry<'a>(
    py: Python<'a>,
    storage_config: Bound<'a, PyDict>,
    space_id: String,
    entry_id: String,
) -> PyResult<Bound<'a, PyAny>> {
    let op = get_operator(py, &storage_config)?;
    let ws_path = format!("spaces/{}", space_id);
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let meta = entry::get_entry(&op, &ws_path, &entry_id)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        let val = serde_json::to_value(meta).map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        Python::with_gil(|py| json_to_py(py, val))
    })
}

#[pyfunction]
fn list_entries<'a>(
    py: Python<'a>,
    storage_config: Bound<'a, PyDict>,
    space_id: String,
) -> PyResult<Bound<'a, PyAny>> {
    let op = get_operator(py, &storage_config)?;
    let ws_path = format!("spaces/{}", space_id);
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let entries = entry::list_entries(&op, &ws_path)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        let val = serde_json::Value::Array(entries);
        Python::with_gil(|py| json_to_py(py, val))
    })
}

#[pyfunction]
fn get_space<'a>(
    py: Python<'a>,
    storage_config: Bound<'a, PyDict>,
    name: String,
) -> PyResult<Bound<'a, PyAny>> {
    let op = get_operator(py, &storage_config)?;
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let meta = space::get_space_raw(&op, &name)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        let val = serde_json::to_value(meta).map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        Python::with_gil(|py| json_to_py(py, val))
    })
}

#[pyfunction]
fn patch_space<'a>(
    py: Python<'a>,
    storage_config: Bound<'a, PyDict>,
    space_id: String,
    patch_json: String,
) -> PyResult<Bound<'a, PyAny>> {
    let op = get_operator(py, &storage_config)?;
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let patch_value: serde_json::Value =
            serde_json::from_str(&patch_json).map_err(|e| PyValueError::new_err(e.to_string()))?;
        let updated = space::patch_space(&op, &space_id, &patch_value)
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
        let types = form::list_column_types()
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        Ok(types)
    })
}

#[pyfunction]
#[pyo3(signature = (storage_config, space_id, form_def_json, strategies_json=None))]
fn migrate_form<'a>(
    py: Python<'a>,
    storage_config: Bound<'a, PyDict>,
    space_id: String,
    form_def_json: String,
    strategies_json: Option<String>,
) -> PyResult<Bound<'a, PyAny>> {
    let op = get_operator(py, &storage_config)?;
    let ws_path = format!("spaces/{}", space_id);
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let form_def: serde_json::Value = serde_json::from_str(&form_def_json)
            .map_err(|e| PyValueError::new_err(e.to_string()))?;
        let strategies = match strategies_json {
            Some(json) => Some(
                serde_json::from_str::<serde_json::Value>(&json)
                    .map_err(|e| PyValueError::new_err(e.to_string()))?,
            ),
            None => None,
        };
        let integrity = RealIntegrityProvider::from_space(&op, &space_id)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        let count = form::migrate_form(&op, &ws_path, &form_def, strategies, &integrity)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        Ok(count)
    })
}

#[pyfunction]
fn reindex_all<'a>(
    py: Python<'a>,
    storage_config: Bound<'a, PyDict>,
    space_id: String,
) -> PyResult<Bound<'a, PyAny>> {
    let op = get_operator(py, &storage_config)?;
    let ws_path = format!("spaces/{}", space_id);
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        index::reindex_all(&op, &ws_path)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        Ok(())
    })
}

#[pyfunction]
fn update_entry_index<'a>(
    py: Python<'a>,
    storage_config: Bound<'a, PyDict>,
    space_id: String,
    entry_id: String,
) -> PyResult<Bound<'a, PyAny>> {
    let op = get_operator(py, &storage_config)?;
    let ws_path = format!("spaces/{}", space_id);
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        index::update_entry_index(&op, &ws_path, &entry_id)
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
fn list_forms<'a>(
    py: Python<'a>,
    storage_config: Bound<'a, PyDict>,
    space_id: String,
) -> PyResult<Bound<'a, PyAny>> {
    let op = get_operator(py, &storage_config)?;
    let ws_path = format!("spaces/{}", space_id);
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let forms = form::list_forms(&op, &ws_path)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        let val =
            serde_json::to_value(forms).map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        Python::with_gil(|py| json_to_py(py, val))
    })
}

#[pyfunction]
fn upsert_form<'a>(
    py: Python<'a>,
    storage_config: Bound<'a, PyDict>,
    space_id: String,
    form_def: String,
) -> PyResult<Bound<'a, PyAny>> {
    let op = get_operator(py, &storage_config)?;
    let ws_path = format!("spaces/{}", space_id);
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let parsed: serde_json::Value =
            serde_json::from_str(&form_def).map_err(|e| PyValueError::new_err(e.to_string()))?;
        form::upsert_form(&op, &ws_path, &parsed)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        Ok(())
    })
}

// Asset

#[pyfunction]
fn save_asset<'a>(
    py: Python<'a>,
    storage_config: Bound<'a, PyDict>,
    space_id: String,
    filename: String,
    content: Vec<u8>,
) -> PyResult<Bound<'a, PyAny>> {
    let op = get_operator(py, &storage_config)?;
    let ws_path = format!("spaces/{}", space_id);
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let info = asset::save_asset(&op, &ws_path, &filename, &content)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        let val = serde_json::to_value(info).map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        Python::with_gil(|py| json_to_py(py, val))
    })
}

#[pyfunction]
fn list_assets<'a>(
    py: Python<'a>,
    storage_config: Bound<'a, PyDict>,
    space_id: String,
) -> PyResult<Bound<'a, PyAny>> {
    let op = get_operator(py, &storage_config)?;
    let ws_path = format!("spaces/{}", space_id);
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let list = asset::list_assets(&op, &ws_path)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        let val = serde_json::to_value(list).map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        Python::with_gil(|py| json_to_py(py, val))
    })
}

#[pyfunction]
fn delete_asset<'a>(
    py: Python<'a>,
    storage_config: Bound<'a, PyDict>,
    space_id: String,
    asset_id: String,
) -> PyResult<Bound<'a, PyAny>> {
    let op = get_operator(py, &storage_config)?;
    let ws_path = format!("spaces/{}", space_id);
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        asset::delete_asset(&op, &ws_path, &asset_id)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        Ok(())
    })
}

#[pyfunction]
fn get_form<'a>(
    py: Python<'a>,
    storage_config: Bound<'a, PyDict>,
    space_id: String,
    form_name: String,
) -> PyResult<Bound<'a, PyAny>> {
    let op = get_operator(py, &storage_config)?;
    let ws_path = format!("spaces/{}", space_id);
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let frm = form::get_form(&op, &ws_path, &form_name)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        let val = serde_json::to_value(frm).map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        Python::with_gil(|py| json_to_py(py, val))
    })
}

#[pyfunction]
fn get_entry_history<'a>(
    py: Python<'a>,
    storage_config: Bound<'a, PyDict>,
    space_id: String,
    entry_id: String,
) -> PyResult<Bound<'a, PyAny>> {
    let op = get_operator(py, &storage_config)?;
    let ws_path = format!("spaces/{}", space_id);
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let history = entry::get_entry_history(&op, &ws_path, &entry_id)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        Python::with_gil(|py| json_to_py(py, history))
    })
}

#[pyfunction]
fn get_entry_revision<'a>(
    py: Python<'a>,
    storage_config: Bound<'a, PyDict>,
    space_id: String,
    entry_id: String,
    revision_id: String,
) -> PyResult<Bound<'a, PyAny>> {
    let op = get_operator(py, &storage_config)?;
    let ws_path = format!("spaces/{}", space_id);
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let revision = entry::get_entry_revision(&op, &ws_path, &entry_id, &revision_id)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        Python::with_gil(|py| json_to_py(py, revision))
    })
}

#[pyfunction]
#[pyo3(signature = (storage_config, space_id, entry_id, revision_id, author=None))]
fn restore_entry<'a>(
    py: Python<'a>,
    storage_config: Bound<'a, PyDict>,
    space_id: String,
    entry_id: String,
    revision_id: String,
    author: Option<String>,
) -> PyResult<Bound<'a, PyAny>> {
    let op = get_operator(py, &storage_config)?;
    let ws_path = format!("spaces/{}", space_id);
    let author = author.unwrap_or_else(|| "unknown".to_string());
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let integrity = RealIntegrityProvider::from_space(&op, &space_id)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        let result =
            entry::restore_entry(&op, &ws_path, &entry_id, &revision_id, &author, &integrity)
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
    form_json: String,
) -> PyResult<PyObject> {
    let properties: serde_json::Value =
        serde_json::from_str(&properties_json).map_err(|e| PyValueError::new_err(e.to_string()))?;
    let form_def: serde_json::Value =
        serde_json::from_str(&form_json).map_err(|e| PyValueError::new_err(e.to_string()))?;
    let (casted, warnings) = index::validate_properties(&properties, &form_def)
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
#[pyo3(signature = (storage_config, space_id, entry_id, content, parent_revision_id=None, author=None, assets_json=None))]
#[allow(clippy::too_many_arguments)]
fn update_entry<'a>(
    py: Python<'a>,
    storage_config: Bound<'a, PyDict>,
    space_id: String,
    entry_id: String,
    content: String,
    parent_revision_id: Option<String>,
    author: Option<String>,
    assets_json: Option<String>,
) -> PyResult<Bound<'a, PyAny>> {
    let op = get_operator(py, &storage_config)?;
    let ws_path = format!("spaces/{}", space_id);
    let author = author.unwrap_or_else(|| "unknown".to_string());

    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let integrity = RealIntegrityProvider::from_space(&op, &space_id)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        let assets = match assets_json {
            Some(json_str) => serde_json::from_str::<Vec<serde_json::Value>>(&json_str)
                .map(Some)
                .map_err(|e| PyRuntimeError::new_err(e.to_string()))?,
            None => None,
        };
        let meta = entry::update_entry(
            &op,
            &ws_path,
            &entry_id,
            &content,
            parent_revision_id.as_deref(),
            &author,
            assets,
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
    space_id: String,
    query: String,
) -> PyResult<Bound<'a, PyAny>> {
    let op = get_operator(py, &storage_config)?;
    let ws_path = format!("spaces/{}", space_id);
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

#[pyfunction]
fn create_sql_session<'a>(
    py: Python<'a>,
    storage_config: Bound<'a, PyDict>,
    space_id: String,
    sql: String,
) -> PyResult<Bound<'a, PyAny>> {
    let op = get_operator(py, &storage_config)?;
    let ws_path = format!("spaces/{}", space_id);
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let session = sql_session::create_sql_session(&op, &ws_path, &sql)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        Python::with_gil(|py| json_to_py(py, session))
    })
}

#[pyfunction]
fn get_sql_session_status<'a>(
    py: Python<'a>,
    storage_config: Bound<'a, PyDict>,
    space_id: String,
    session_id: String,
) -> PyResult<Bound<'a, PyAny>> {
    let op = get_operator(py, &storage_config)?;
    let ws_path = format!("spaces/{}", space_id);
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let session = sql_session::get_sql_session_status(&op, &ws_path, &session_id)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        Python::with_gil(|py| json_to_py(py, session))
    })
}

#[pyfunction]
fn get_sql_session_count<'a>(
    py: Python<'a>,
    storage_config: Bound<'a, PyDict>,
    space_id: String,
    session_id: String,
) -> PyResult<Bound<'a, PyAny>> {
    let op = get_operator(py, &storage_config)?;
    let ws_path = format!("spaces/{}", space_id);
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let count = sql_session::get_sql_session_count(&op, &ws_path, &session_id)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        let val = Value::Number(count.into());
        Python::with_gil(|py| json_to_py(py, val))
    })
}

#[pyfunction]
fn get_sql_session_rows<'a>(
    py: Python<'a>,
    storage_config: Bound<'a, PyDict>,
    space_id: String,
    session_id: String,
    offset: usize,
    limit: usize,
) -> PyResult<Bound<'a, PyAny>> {
    let op = get_operator(py, &storage_config)?;
    let ws_path = format!("spaces/{}", space_id);
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let rows = sql_session::get_sql_session_rows(&op, &ws_path, &session_id, offset, limit)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        Python::with_gil(|py| json_to_py(py, rows))
    })
}

#[pyfunction]
fn get_sql_session_rows_all<'a>(
    py: Python<'a>,
    storage_config: Bound<'a, PyDict>,
    space_id: String,
    session_id: String,
) -> PyResult<Bound<'a, PyAny>> {
    let op = get_operator(py, &storage_config)?;
    let ws_path = format!("spaces/{}", space_id);
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let rows = sql_session::get_sql_session_rows_all(&op, &ws_path, &session_id)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        let val = Value::Array(rows);
        Python::with_gil(|py| json_to_py(py, val))
    })
}

// Stubs using generic signature removed; all bindings are implemented.

/// A Python module implemented in Rust.
#[pymodule]
fn _ugoite_core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(list_spaces, m)?)?;
    m.add_function(wrap_pyfunction!(create_space, m)?)?;
    m.add_function(wrap_pyfunction!(create_sample_space, m)?)?;
    m.add_function(wrap_pyfunction!(test_storage_connection_py, m)?)?;

    m.add_function(wrap_pyfunction!(create_entry, m)?)?;
    m.add_function(wrap_pyfunction!(delete_entry, m)?)?;
    m.add_function(wrap_pyfunction!(get_entry, m)?)?;
    m.add_function(wrap_pyfunction!(get_entry_history, m)?)?;
    m.add_function(wrap_pyfunction!(get_entry_revision, m)?)?;
    m.add_function(wrap_pyfunction!(list_entries, m)?)?;
    m.add_function(wrap_pyfunction!(restore_entry, m)?)?;
    m.add_function(wrap_pyfunction!(update_entry, m)?)?;
    m.add_function(wrap_pyfunction!(list_sql, m)?)?;
    m.add_function(wrap_pyfunction!(get_sql, m)?)?;
    m.add_function(wrap_pyfunction!(create_sql, m)?)?;
    m.add_function(wrap_pyfunction!(update_sql, m)?)?;
    m.add_function(wrap_pyfunction!(delete_sql, m)?)?;
    m.add_function(wrap_pyfunction!(extract_properties_py, m)?)?;
    m.add_function(wrap_pyfunction!(validate_properties_py, m)?)?;

    m.add_function(wrap_pyfunction!(list_forms, m)?)?;
    m.add_function(wrap_pyfunction!(upsert_form, m)?)?;
    m.add_function(wrap_pyfunction!(get_form, m)?)?;
    m.add_function(wrap_pyfunction!(list_column_types, m)?)?;
    m.add_function(wrap_pyfunction!(migrate_form, m)?)?;

    m.add_function(wrap_pyfunction!(save_asset, m)?)?;
    m.add_function(wrap_pyfunction!(list_assets, m)?)?;
    m.add_function(wrap_pyfunction!(delete_asset, m)?)?;

    m.add_function(wrap_pyfunction!(get_space, m)?)?;
    m.add_function(wrap_pyfunction!(patch_space, m)?)?;

    m.add_function(wrap_pyfunction!(query_index, m)?)?;
    m.add_function(wrap_pyfunction!(create_sql_session, m)?)?;
    m.add_function(wrap_pyfunction!(get_sql_session_status, m)?)?;
    m.add_function(wrap_pyfunction!(get_sql_session_count, m)?)?;
    m.add_function(wrap_pyfunction!(get_sql_session_rows, m)?)?;
    m.add_function(wrap_pyfunction!(get_sql_session_rows_all, m)?)?;
    m.add_function(wrap_pyfunction!(reindex_all, m)?)?;
    m.add_function(wrap_pyfunction!(update_entry_index, m)?)?;

    m.add_function(wrap_pyfunction!(create_link, m)?)?;
    m.add_function(wrap_pyfunction!(list_links, m)?)?;
    m.add_function(wrap_pyfunction!(delete_link, m)?)?;
    m.add_function(wrap_pyfunction!(search_entries, m)?)?;
    m.add_function(wrap_pyfunction!(build_response_signature, m)?)?;
    m.add_function(wrap_pyfunction!(load_hmac_material, m)?)?;

    Ok(())
}
