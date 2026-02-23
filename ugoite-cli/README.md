# ugoite-cli

CLI tool for interacting with Ugoite core and backend modes.

## Backend mode unreachable hint

If backend mode commands fail with connection errors:

1. Verify backend server is running.
   - `curl -sS http://localhost:8000/health`
2. Verify endpoint config.
   - default: `${HOME}/.ugoite/cli-endpoints.json`
   - overrides: `UGOITE_CLI_CONFIG_PATH` or `UGOITE_CONFIG_HOME`
3. Reset endpoint config if needed.
   - `bash ugoite-cli/scripts/reset-endpoint-config.sh`
4. Re-run command with explicit auth env.

This workflow avoids ambiguous "connection failed" failures and provides direct remediation.
