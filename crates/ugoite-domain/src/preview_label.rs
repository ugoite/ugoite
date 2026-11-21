/// Returns a control-safe label bounded for a visible preview.
///
/// Labels up to 96 Unicode scalar values are kept in full. Longer labels are
/// shortened to 84 values and marked as omitted.
pub fn safe_preview_label(value: &str) -> String {
    const MAX_CHARS: usize = 96;
    const PREFIX_CHARS: usize = 84;
    const OMITTED: &str = "… [omitted]";

    let chars = value.chars().collect::<Vec<_>>();
    let shortened = chars.len() > MAX_CHARS;
    let mut label = chars
        .into_iter()
        .take(if shortened { PREFIX_CHARS } else { MAX_CHARS })
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect::<String>();
    if shortened {
        label.push_str(OMITTED);
    }
    label
}

#[cfg(test)]
mod tests {
    use super::safe_preview_label;

    #[test]
    fn safe_preview_label_sanitizes_controls_and_observes_length_boundaries() {
        assert_eq!(safe_preview_label("a\n\0\u{7f}\u{85}b"), "a    b");
        assert_eq!(safe_preview_label(&"x".repeat(84)), "x".repeat(84));
        assert_eq!(safe_preview_label(&"x".repeat(85)), "x".repeat(85));
        assert_eq!(safe_preview_label(&"x".repeat(96)), "x".repeat(96));
        assert_eq!(
            safe_preview_label(&"x".repeat(97)),
            format!("{}… [omitted]", "x".repeat(84))
        );
    }
}
