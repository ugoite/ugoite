//! Portable spreadsheet export encoding.
//!
//! Spreadsheet exports are derived representations of Knowledge. They must
//! not change the durable value, but formula-like values need a text marker in
//! the exported representation so opening a CSV in a spreadsheet cannot turn
//! them into formulas.

/// Encode rows as a spreadsheet-safe RFC 4180-style CSV document.
///
/// Every cell is quoted, quotes inside cells are doubled, and records use the
/// CRLF line ending specified by RFC 4180. Formula-like leading characters
/// receive an apostrophe in the export representation only; the caller's
/// durable value is never modified.
pub fn encode_spreadsheet_csv(rows: &[Vec<String>]) -> String {
    rows.iter()
        .map(|row| {
            row.iter()
                .map(|cell| encode_spreadsheet_csv_cell(cell))
                .collect::<Vec<_>>()
                .join(",")
        })
        .collect::<Vec<_>>()
        .join("\r\n")
}

/// Encode one cell for a spreadsheet-safe CSV document.
pub fn encode_spreadsheet_csv_cell(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len() + 2);
    encoded.push('"');
    if value
        .chars()
        .next()
        .map(is_formula_interpretation_prefix)
        .unwrap_or(false)
    {
        // An apostrophe is recognized by spreadsheet applications as a text
        // marker and is not shown as part of the displayed cell value.
        encoded.push('\'');
    }
    for character in value.chars() {
        if character == '"' {
            encoded.push('"');
        }
        encoded.push(character);
    }
    encoded.push('"');
    encoded
}

/// Return whether the first character can make a CSV cell be interpreted as
/// a spreadsheet formula or as a control-prefixed formula.
fn is_formula_interpretation_prefix(character: char) -> bool {
    matches!(character, '=' | '+' | '-' | '@') || character.is_control()
}

#[cfg(test)]
mod tests {
    use super::{encode_spreadsheet_csv, encode_spreadsheet_csv_cell};

    #[test]
    fn preserves_rfc4180_special_values_and_unicode() {
        let rows = vec![
            vec![
                "plain".to_string(),
                "a,b".to_string(),
                "a\"b".to_string(),
                "line\nbreak".to_string(),
                "日本語".to_string(),
            ],
            vec!["second row".to_string()],
        ];

        assert_eq!(
            encode_spreadsheet_csv(&rows),
            "\"plain\",\"a,b\",\"a\"\"b\",\"line\nbreak\",\"日本語\"\r\n\"second row\""
        );
    }

    #[test]
    fn neutralizes_formula_and_control_prefixes_only_in_export() {
        let values = [
            "=SUM(A1:A2)",
            "+1",
            "-1",
            "@user",
            "\t=SUM(A1:A2)",
            "\r=SUM(A1:A2)",
            "\n=SUM(A1:A2)",
            "\u{0001}=SUM(A1:A2)",
        ];

        for value in values {
            assert_eq!(
                encode_spreadsheet_csv_cell(value),
                format!("\"'{}\"", value),
                "formula-like value should be exported as literal text: {value:?}"
            );
        }

        let durable_value = "=SUM(A1:A2)";
        let _ = encode_spreadsheet_csv_cell(durable_value);
        assert_eq!(durable_value, "=SUM(A1:A2)");
    }

    #[test]
    fn keeps_non_formula_values_unchanged_before_csv_escaping() {
        for value in ["42", "hello", " leading", "quote'", "🦀"] {
            assert_eq!(
                encode_spreadsheet_csv_cell(value),
                format!("\"{}\"", value.replace('"', "\"\""))
            );
        }
    }
}
