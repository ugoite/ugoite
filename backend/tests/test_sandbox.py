import pytest
import os
from app.core.sandbox import run_in_sandbox


@pytest.mark.xfail(
    reason="Sandbox isolation requires bwrap/nsjail which is not available in this dev env"
)
def test_sandbox_denies_access_outside_tmp(tmp_path):
    # Create a secret file outside the allowed area
    secret_file = tmp_path / "secret.txt"
    secret_file.write_text("secret data")

    # Code that tries to read the secret file
    code = f"""
with open('{secret_file}', 'r') as f:
    print(f.read())
"""

    # Run the code in the sandbox
    result = run_in_sandbox(code)

    # Expect failure or empty output, definitely not the secret data
    assert "secret data" not in result.stdout
    assert (
        "Permission denied" in result.stderr
        or "FileNotFoundError" in result.stderr
        or result.returncode != 0
    )


def test_sandbox_allows_tmp_access():
    code = """
import os
with open('/tmp/test_sandbox.txt', 'w') as f:
    f.write('hello world')
with open('/tmp/test_sandbox.txt', 'r') as f:
    print(f.read())
"""
    result = run_in_sandbox(code)
    assert "hello world" in result.stdout
