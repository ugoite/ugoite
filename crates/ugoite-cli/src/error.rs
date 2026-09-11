use anyhow::Error;

/// Human-only error text kept for compatibility. New code should use
/// `crate::output::project_error` for the shared machine/human contract.
pub fn format_cli_error(error: &Error) -> String {
    crate::output::format_cli_error(error)
}
