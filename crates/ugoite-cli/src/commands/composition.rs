use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use ugoite_api_client::{
    CompositionDiagnosticCode as ApiCompositionDiagnosticCode, CompositionLintError,
    CompositionLintResponse, CompositionLintValue,
};
use ugoite_domain::composition::{
    canonicalize_composition_yaml, CompositionDiagnosticCode, MAX_COMPOSITION_YAML_BYTES,
};

#[derive(Args)]
pub struct CompositionCmd {
    #[command(subcommand)]
    pub sub: CompositionSubCmd,
}

#[derive(Subcommand)]
pub enum CompositionSubCmd {
    /// Validate and canonicalize a Composition YAML file without a Space or server
    Lint {
        #[arg(value_name = "FILE")]
        file: PathBuf,
    },
}

/// Composition file commands are local and do not resolve CLI contexts.
pub fn run(cmd: CompositionCmd) -> Result<()> {
    match cmd.sub {
        CompositionSubCmd::Lint { file } => {
            let response = lint_file(&file)?;
            crate::output::print_json(&response);
        }
    }
    Ok(())
}

fn lint_file(path: &Path) -> Result<CompositionLintResponse> {
    let file =
        File::open(path).with_context(|| format!("open Composition file {}", path.display()))?;
    let mut bytes = Vec::with_capacity(MAX_COMPOSITION_YAML_BYTES + 1);
    file.take((MAX_COMPOSITION_YAML_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .with_context(|| format!("read Composition file {}", path.display()))?;
    Ok(lint_yaml_bytes(&bytes))
}

fn lint_yaml_bytes(bytes: &[u8]) -> CompositionLintResponse {
    if bytes.len() > MAX_COMPOSITION_YAML_BYTES {
        return diagnostic_response(CompositionDiagnosticCode::InvalidComposition);
    }
    let Ok(yaml) = std::str::from_utf8(bytes) else {
        return diagnostic_response(CompositionDiagnosticCode::InvalidComposition);
    };

    match canonicalize_composition_yaml(yaml) {
        Ok(canonical) => CompositionLintResponse {
            ok: true,
            value: Some(CompositionLintValue {
                document: serde_json::to_value(canonical.document)
                    .expect("Composition domain document is JSON serializable"),
                canonical_yaml: canonical.yaml,
                fingerprint: canonical.fingerprint,
            }),
            error: None,
        },
        Err(code) => diagnostic_response(code),
    }
}

fn diagnostic_response(code: CompositionDiagnosticCode) -> CompositionLintResponse {
    let api_code = ApiCompositionDiagnosticCode::from_code(code.as_str())
        .expect("Composition domain diagnostics are represented in the portable API");
    CompositionLintResponse {
        ok: false,
        value: None,
        error: Some(CompositionLintError {
            kind: "composition_diagnostic".to_string(),
            code: api_code,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::{lint_file, lint_yaml_bytes};
    use std::io::Write;
    use ugoite_api_client::CompositionDiagnosticCode;
    use ugoite_domain::composition::MAX_COMPOSITION_YAML_BYTES;

    const MONTHLY_EXPENSE: &str = include_str!(
        "../../../../crates/ugoite-domain/tests/fixtures/composition/monthly-expense.ugcomp.yaml"
    );
    const MONTHLY_EXPENSE_CANONICAL: &str = include_str!(
        "../../../../crates/ugoite-domain/tests/fixtures/composition/monthly-expense.canonical.ugcomp.yaml"
    );

    #[test]
    fn lint_file_returns_the_shared_canonical_document_response() {
        let mut file = tempfile::NamedTempFile::new().expect("temporary YAML file");
        file.write_all(MONTHLY_EXPENSE.as_bytes())
            .expect("write YAML fixture");

        let response = lint_file(file.path()).expect("lint succeeds");

        assert!(response.ok);
        assert!(response.error.is_none());
        let value = response.value.expect("canonical value");
        assert_eq!(value.canonical_yaml, MONTHLY_EXPENSE_CANONICAL);
        assert_eq!(value.fingerprint.len(), 64);
        assert_eq!(value.document["format_version"], 1);
        assert_eq!(value.document["name"], "Monthly expenses");
    }

    #[test]
    fn lint_returns_the_domain_diagnostic_code_for_unsupported_versions() {
        let response =
            lint_yaml_bytes(b"format_version: 22\nname: Future\nkind: dashboard\nspec: {}\n");

        assert!(!response.ok);
        assert!(response.value.is_none());
        assert_eq!(
            response.error.expect("diagnostic").code,
            CompositionDiagnosticCode::UnsupportedFormatVersion
        );
    }

    #[test]
    fn lint_rejects_oversized_and_non_utf8_files_as_invalid_composition() {
        let oversized = vec![b' '; MAX_COMPOSITION_YAML_BYTES + 1];
        let oversized_response = lint_yaml_bytes(&oversized);
        let invalid_utf8_response = lint_yaml_bytes(&[0xff]);

        for response in [oversized_response, invalid_utf8_response] {
            assert!(!response.ok);
            assert_eq!(
                response.error.expect("diagnostic").code,
                CompositionDiagnosticCode::InvalidComposition
            );
        }
    }

    #[test]
    fn lint_reads_at_most_one_byte_over_the_yaml_limit() {
        let mut file = tempfile::NamedTempFile::new().expect("temporary YAML file");
        file.write_all(&vec![b' '; MAX_COMPOSITION_YAML_BYTES + 100])
            .expect("write oversized file");

        let response = lint_file(file.path()).expect("file read succeeds");

        assert!(!response.ok);
        assert_eq!(
            response.error.expect("diagnostic").code,
            CompositionDiagnosticCode::InvalidComposition
        );
    }
}
