//! Compact, role-aware table rendering for human-facing CLI output.

use super::style::{stdout_style, Role, StylePolicy};

/// Print a list of string IDs as a single-column table.
pub fn print_list_table(header: &str, items: &[impl std::fmt::Display]) {
    println!("{}", render_list_table(header, items, &stdout_style()));
}

/// Print a list of JSON objects as a table, selecting the given columns.
/// Columns is a slice of `(header, json_key)` pairs.
pub fn print_json_table(rows: &[serde_json::Value], columns: &[(&str, &str)]) {
    println!("{}", render_json_table(rows, columns, &stdout_style()));
}

fn render_list_table(
    header: &str,
    items: &[impl std::fmt::Display],
    style: &StylePolicy,
) -> String {
    let mut lines = Vec::with_capacity(items.len() + 1);
    lines.push(style.paint(Role::Muted, header));
    lines.extend(items.iter().map(|item| style.paint(Role::Primary, item)));
    lines.join("\n")
}

fn render_json_table(
    rows: &[serde_json::Value],
    columns: &[(&str, &str)],
    style: &StylePolicy,
) -> String {
    let mut widths: Vec<usize> = columns.iter().map(|(header, _)| header.len()).collect();
    let cell_matrix: Vec<Vec<String>> = rows
        .iter()
        .map(|row| {
            columns
                .iter()
                .enumerate()
                .map(|(index, (_, key))| {
                    let cell = match &row[key] {
                        serde_json::Value::String(value) => value.clone(),
                        serde_json::Value::Null => String::new(),
                        other => other.to_string(),
                    };
                    widths[index] = widths[index].max(cell.len());
                    cell
                })
                .collect()
        })
        .collect();

    let header = columns
        .iter()
        .enumerate()
        .map(|(index, (value, _))| {
            let padded = pad_cell(value, widths[index], index + 1 == columns.len());
            style.paint(Role::Muted, padded)
        })
        .collect::<Vec<_>>()
        .join("  ");
    let rows = cell_matrix
        .iter()
        .map(|cells| {
            cells
                .iter()
                .enumerate()
                .map(|(index, value)| {
                    let padded = pad_cell(value, widths[index], index + 1 == columns.len());
                    if index == 0 {
                        style.paint(Role::Primary, padded)
                    } else {
                        padded
                    }
                })
                .collect::<Vec<_>>()
                .join("  ")
        })
        .collect::<Vec<_>>();

    std::iter::once(header)
        .chain(rows)
        .collect::<Vec<_>>()
        .join("\n")
}

fn pad_cell(value: &str, width: usize, last: bool) -> String {
    if last {
        value.to_string()
    } else {
        format!("{value:<width$}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strip_ansi(text: &str) -> String {
        let mut stripped = String::with_capacity(text.len());
        let mut escape = false;
        for character in text.chars() {
            if escape {
                if character.is_ascii_alphabetic() {
                    escape = false;
                }
            } else if character == '\u{1b}' {
                escape = true;
            } else {
                stripped.push(character);
            }
        }
        stripped
    }

    #[test]
    fn list_tables_are_borderless_and_role_aware() {
        let items = ["team-notes", "research", "scratch"];
        let plain = render_list_table("SPACE_UID", &items, &StylePolicy::new(false));
        assert_eq!(plain, "SPACE_UID\nteam-notes\nresearch\nscratch");
        assert!(!plain
            .lines()
            .any(|line| line.chars().all(|character| character == '-')));

        let styled = render_list_table("SPACE_UID", &items, &StylePolicy::new(true));
        assert_eq!(strip_ansi(&styled), plain);
        assert!(styled.contains("\u{1b}[2mSPACE_UID\u{1b}[0m"));
        assert!(styled.contains("\u{1b}[36mteam-notes\u{1b}[0m"));
    }

    #[test]
    fn styles_are_applied_after_raw_width_calculation() {
        let rows = [
            serde_json::json!({"id": "note-1", "title": "Planning"}),
            serde_json::json!({"id": "note-2", "title": "Decisions"}),
        ];
        let columns = [("ID", "id"), ("TITLE", "title")];
        let plain = render_json_table(&rows, &columns, &StylePolicy::new(false));
        let styled = render_json_table(&rows, &columns, &StylePolicy::new(true));

        assert_eq!(plain, "ID      TITLE\nnote-1  Planning\nnote-2  Decisions");
        assert_eq!(strip_ansi(&styled), plain);
        assert!(styled.contains("\u{1b}[2mID    \u{1b}[0m"));
        assert!(styled.contains("\u{1b}[36mnote-1\u{1b}[0m"));
        assert!(styled.contains("  Planning"));
        assert!(!styled.contains('-'.to_string().repeat(2).as_str()));
    }
}
