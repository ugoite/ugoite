//! Portable, inert text templates used by Composition Text components.

use serde::{Deserialize, Serialize};

/// One parsed piece of a Composition Text template.
///
/// Literal text is rendered as text. Variable pieces resolve to a scalar value
/// during Composition evaluation; this format has no HTML, expression, or
/// code-evaluation semantics.
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CompositionTextTemplatePart {
    Literal { text: String },
    Variable { variable: String },
}

/// Stable syntax errors for a Text template.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum CompositionTextTemplateError {
    UnclosedVariable,
    EmptyVariable,
    InvalidVariable,
    NestedVariable,
}

impl CompositionTextTemplateError {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UnclosedVariable => "unclosed_variable",
            Self::EmptyVariable => "empty_variable",
            Self::InvalidVariable => "invalid_variable",
            Self::NestedVariable => "nested_variable",
        }
    }
}

/// Parse `{{variable_id}}` references from a Text template.
///
/// The parser preserves literal text exactly, except that `\\{{` is the
/// literal spelling for `{{`. A variable reference is an exact, case-sensitive
/// graph ID. It may contain Unicode and spaces but cannot contain braces. A
/// lone closing `}}` is ordinary text.
pub fn parse_composition_text_template(
    template: &str,
) -> Result<Vec<CompositionTextTemplatePart>, CompositionTextTemplateError> {
    let mut parts = Vec::new();
    let mut literal = String::new();
    let mut cursor = 0;

    while cursor < template.len() {
        let remaining = &template[cursor..];
        if remaining.starts_with(r"\{{") {
            literal.push_str("{{");
            cursor += 3;
            continue;
        }

        if remaining.starts_with("{{") {
            if !literal.is_empty() {
                parts.push(CompositionTextTemplatePart::Literal {
                    text: std::mem::take(&mut literal),
                });
            }
            let variable_start = cursor + 2;
            let Some(relative_end) = template[variable_start..].find("}}") else {
                return Err(CompositionTextTemplateError::UnclosedVariable);
            };
            let variable_end = variable_start + relative_end;
            let variable = &template[variable_start..variable_end];
            if variable.contains("{{") {
                return Err(CompositionTextTemplateError::NestedVariable);
            }
            if variable.trim().is_empty() {
                return Err(CompositionTextTemplateError::EmptyVariable);
            }
            if variable
                .chars()
                .any(|character| matches!(character, '{' | '}'))
            {
                return Err(CompositionTextTemplateError::InvalidVariable);
            }
            parts.push(CompositionTextTemplatePart::Variable {
                variable: variable.to_owned(),
            });
            cursor = variable_end + 2;
            continue;
        }

        let character = remaining
            .chars()
            .next()
            .expect("cursor is before the end of the template");
        literal.push(character);
        cursor += character.len_utf8();
    }

    if !literal.is_empty() {
        parts.push(CompositionTextTemplatePart::Literal { text: literal });
    }
    Ok(parts)
}

#[cfg(test)]
mod tests {
    use super::{
        parse_composition_text_template, CompositionTextTemplateError as Error,
        CompositionTextTemplatePart as Part,
    };

    #[test]
    fn parses_ordered_literals_variables_and_repeated_references() {
        assert_eq!(
            parse_composition_text_template("合計：{{total}} 円 / {{total}}!").unwrap(),
            [
                Part::Literal {
                    text: "合計：".to_owned()
                },
                Part::Variable {
                    variable: "total".to_owned()
                },
                Part::Literal {
                    text: " 円 / ".to_owned()
                },
                Part::Variable {
                    variable: "total".to_owned()
                },
                Part::Literal {
                    text: "!".to_owned()
                },
            ]
        );
    }

    #[test]
    fn escaped_opening_braces_are_literal_and_do_not_form_a_reference() {
        assert_eq!(
            parse_composition_text_template(r"show \{{literal}} and {{value}}").unwrap(),
            [
                Part::Literal {
                    text: "show {{literal}} and ".to_owned()
                },
                Part::Variable {
                    variable: "value".to_owned()
                },
            ]
        );
    }

    #[test]
    fn empty_template_and_unmatched_closing_braces_are_literal() {
        assert!(parse_composition_text_template("").unwrap().is_empty());
        assert_eq!(
            parse_composition_text_template("literal }}").unwrap(),
            [Part::Literal {
                text: "literal }}".to_owned()
            }]
        );
    }

    #[test]
    fn malformed_references_have_stable_syntax_errors() {
        for (template, expected) in [
            ("{{total", Error::UnclosedVariable),
            ("{{}}", Error::EmptyVariable),
            ("{{bad{part}}}", Error::InvalidVariable),
            ("{{outer{{inner}}}}", Error::NestedVariable),
        ] {
            assert_eq!(parse_composition_text_template(template), Err(expected));
        }
    }

    #[test]
    fn graph_ids_are_exact_and_may_contain_unicode_or_spaces() {
        assert_eq!(
            parse_composition_text_template("{{今月の合計}} / {{monthly total}}").unwrap(),
            [
                Part::Variable {
                    variable: "今月の合計".to_owned()
                },
                Part::Literal {
                    text: " / ".to_owned()
                },
                Part::Variable {
                    variable: "monthly total".to_owned()
                },
            ]
        );
    }
}
