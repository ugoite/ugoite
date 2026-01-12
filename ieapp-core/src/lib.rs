#![deny(warnings)]
#![deny(clippy::all)]

use pyo3::prelude::*;

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

/// Formats the sum of two numbers as string.
#[pyfunction]
fn sum_as_string(a: usize, b: usize) -> PyResult<String> {
    Ok((a + b).to_string())
}

/// A Python module implemented in Rust.
#[pymodule]
fn _ieapp_core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(sum_as_string, m)?)?;

    // Workspace
    m.add_function(wrap_pyfunction!(workspace::list_workspaces, m)?)?;
    m.add_function(wrap_pyfunction!(workspace::create_workspace, m)?)?;
    m.add_function(wrap_pyfunction!(workspace::get_workspace, m)?)?;
    m.add_function(wrap_pyfunction!(workspace::patch_workspace, m)?)?;
    m.add_function(wrap_pyfunction!(workspace::test_storage_connection, m)?)?;

    // Note
    m.add_function(wrap_pyfunction!(note::create_note, m)?)?;
    m.add_function(wrap_pyfunction!(note::list_notes, m)?)?;
    m.add_function(wrap_pyfunction!(note::get_note, m)?)?;
    m.add_function(wrap_pyfunction!(note::update_note, m)?)?;
    m.add_function(wrap_pyfunction!(note::delete_note, m)?)?;
    m.add_function(wrap_pyfunction!(note::get_note_history, m)?)?;
    m.add_function(wrap_pyfunction!(note::get_note_revision, m)?)?;
    m.add_function(wrap_pyfunction!(note::restore_note, m)?)?;

    // Class
    m.add_function(wrap_pyfunction!(class::list_classes, m)?)?;
    m.add_function(wrap_pyfunction!(class::list_column_types, m)?)?;
    m.add_function(wrap_pyfunction!(class::get_class, m)?)?;
    m.add_function(wrap_pyfunction!(class::upsert_class, m)?)?;

    // Attachment
    m.add_function(wrap_pyfunction!(attachment::save_attachment, m)?)?;
    m.add_function(wrap_pyfunction!(attachment::list_attachments, m)?)?;
    m.add_function(wrap_pyfunction!(attachment::delete_attachment, m)?)?;

    // Link
    m.add_function(wrap_pyfunction!(link::create_link, m)?)?;
    m.add_function(wrap_pyfunction!(link::list_links, m)?)?;
    m.add_function(wrap_pyfunction!(link::delete_link, m)?)?;

    // Index & Search
    m.add_function(wrap_pyfunction!(index::query_index, m)?)?;
    m.add_function(wrap_pyfunction!(search::search_notes, m)?)?;

    Ok(())
}
