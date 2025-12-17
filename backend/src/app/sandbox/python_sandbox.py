"""WebAssembly sandbox for executing JavaScript code securely."""

import json
import os
import select
import shutil
import struct
import tempfile
import threading
from collections.abc import Callable
from pathlib import Path
from typing import BinaryIO

from wasmtime import Config, Engine, Func, Linker, Module, Store, WasiConfig

SANDBOX_WASM_PATH = Path(__file__).parent / "sandbox.wasm"

# Protocol magic bytes
_MAGIC_HOST = b"\0HOST\0"
_MAGIC_RESULT = b"\0RSLT\0"
_MAGIC_ERROR = b"\0ERRR\0"
_MAGIC_SIZE = 6
_LENGTH_SIZE = 4


class SandboxError(Exception):
    """Base exception for sandbox errors."""


class SandboxTimeoutError(SandboxError):
    """Raised when sandbox execution times out."""


class SandboxExecutionError(SandboxError):
    """Raised when script execution fails."""


HostCallHandler = Callable[[str, str, dict | None], object]


def _read_exact(f: BinaryIO, size: int) -> bytes | None:
    """Read exact number of bytes from a file, returning None on EOF."""
    data = f.read(size)
    if not data:
        return None
    while len(data) < size:
        chunk = f.read(size - len(data))
        if not chunk:
            return None
        data += chunk
    return data


def _handle_host_call(
    f_out: BinaryIO,
    f_in: BinaryIO,
    host_call_handler: HostCallHandler,
) -> bool:
    """Handle a host call from the sandbox, returning True if successful."""
    len_bytes = _read_exact(f_out, _LENGTH_SIZE)
    if len_bytes is None:
        return False

    length = struct.unpack(">I", len_bytes)[0]
    payload_bytes = _read_exact(f_out, length)
    if payload_bytes is None:
        return False

    payload = json.loads(payload_bytes.decode("utf-8"))

    try:
        resp = host_call_handler(
            payload["method"],
            payload["path"],
            payload.get("body"),
        )
        resp_str = json.dumps(resp)
    except (KeyError, TypeError, ValueError) as e:
        resp_str = json.dumps({"error": str(e)})

    resp_bytes = resp_str.encode("utf-8")
    f_in.write(struct.pack(">I", len(resp_bytes)))
    f_in.write(resp_bytes)
    f_in.flush()
    return True


def _handle_result(f_out: BinaryIO) -> object | None:
    """Handle a result message from the sandbox."""
    len_bytes = _read_exact(f_out, _LENGTH_SIZE)
    if len_bytes is None:
        return None

    length = struct.unpack(">I", len_bytes)[0]
    res_bytes = _read_exact(f_out, length)
    if res_bytes is None:
        return None

    return json.loads(res_bytes.decode("utf-8"))


def _handle_error(f_out: BinaryIO) -> SandboxExecutionError | None:
    """Handle an error message from the sandbox."""
    len_bytes = _read_exact(f_out, _LENGTH_SIZE)
    if len_bytes is None:
        return None

    length = struct.unpack(">I", len_bytes)[0]
    err_bytes = _read_exact(f_out, length)
    if err_bytes is None:
        return None

    return SandboxExecutionError(err_bytes.decode("utf-8"))


def _process_output(
    f_out: BinaryIO,
    f_in: BinaryIO,
    host_call_handler: HostCallHandler,
    result_container: dict[str, object],
) -> bool:
    """Process output from sandbox, returning True to continue processing."""
    magic = _read_exact(f_out, _MAGIC_SIZE)
    if magic is None:
        return False

    if magic == _MAGIC_HOST:
        return _handle_host_call(f_out, f_in, host_call_handler)

    if magic == _MAGIC_RESULT:
        result_container["result"] = _handle_result(f_out)
        return False

    if magic == _MAGIC_ERROR:
        result_container["error"] = _handle_error(f_out)
        return False

    msg = f"Invalid magic: {magic!r}"
    raise SandboxError(msg)


def run_script(  # noqa: PLR0915
    code: str,
    host_call_handler: HostCallHandler,
    fuel_limit: int = 100_000_000,
) -> object:
    """Execute JavaScript code in a sandboxed WebAssembly environment.

    Args:
        code: The JavaScript code to execute.
        host_call_handler: Callback for handling host calls from the script.
        fuel_limit: Maximum fuel (instructions) to consume.

    Returns:
        The result of the script execution.

    Raises:
        SandboxError: If execution fails due to sandbox issues.
        SandboxExecutionError: If the script throws an error.

    """
    # Require the real Wasm-based sandbox artifact to be present.
    # Previously we fell back to a minimal runner when `sandbox.wasm`
    # didn't exist to make unit tests runnable without building the
    # Wasm artifact. Change the behavior to *require* the Wasm
    # artifact so tests and runtime use the real sandbox.
    if not SANDBOX_WASM_PATH.exists():
        msg = (
            "sandbox.wasm is missing. Build the Wasm artifact before running "
            "(e.g. `mise run sandbox:build` or run "
            "`bash backend/src/app/sandbox/build_sandbox_wasm.sh`)."
        )
        raise SandboxError(msg)

    config = Config()
    config.consume_fuel = True
    engine = Engine(config)
    store = Store(engine)
    store.set_fuel(fuel_limit)

    linker = Linker(engine)
    linker.define_wasi()

    tmp_dir = tempfile.mkdtemp()
    try:
        tmp_path = Path(tmp_dir)
        stdin_path = tmp_path / "stdin.fifo"
        stdout_path = tmp_path / "stdout.fifo"
        stderr_path = tmp_path / "stderr.fifo"

        os.mkfifo(stdin_path)
        os.mkfifo(stdout_path)
        os.mkfifo(stderr_path)

        # Open pipes (Host side) BEFORE configuring WASI
        fd_in = os.open(stdin_path, os.O_RDWR)
        fd_out = os.open(stdout_path, os.O_RDWR)
        fd_err = os.open(stderr_path, os.O_RDWR)

        f_in = os.fdopen(fd_in, "wb", buffering=0)
        f_out = os.fdopen(fd_out, "rb", buffering=0)
        f_err = os.fdopen(fd_err, "rb", buffering=0)

        wasi = WasiConfig()
        wasi.stdin_file = str(stdin_path)
        wasi.stdout_file = str(stdout_path)
        wasi.stderr_file = str(stderr_path)

        store.set_wasi(wasi)
        module = Module.from_file(engine, str(SANDBOX_WASM_PATH))
        result_container: dict[str, object] = {}

        def run_wasm() -> None:
            try:
                instance = linker.instantiate(store, module)
                exports = instance.exports(store)
                start_func = exports["_start"]
                if isinstance(start_func, Func):
                    start_func(store)
                else:
                    msg = "_start is not a function"
                    raise SandboxError(msg)  # noqa: TRY301
            except SandboxError:
                raise
            except Exception as e:  # noqa: BLE001
                result_container["runtime_error"] = e

        wasm_thread = threading.Thread(target=run_wasm)
        wasm_thread.start()

        try:
            _send_code(f_in, code)
            _process_loop(
                f_out,
                f_err,
                f_in,
                wasm_thread,
                host_call_handler,
                result_container,
            )
        finally:
            f_in.close()
            f_out.close()
            f_err.close()

        wasm_thread.join(timeout=1)
        _check_errors(result_container)

        return result_container.get("result")
    finally:
        shutil.rmtree(tmp_dir)


# Note: the minimal JS fallback runner previously present here was removed.
# The sandbox now strictly requires the real WebAssembly artifact to be
# available; attempting to run without `sandbox.wasm` raises `SandboxError`.


def _send_code(f_in: BinaryIO, code: str) -> None:
    """Send JavaScript code to the sandbox."""
    code_bytes = code.encode("utf-8")
    f_in.write(struct.pack(">I", len(code_bytes)))
    f_in.write(code_bytes)
    f_in.flush()


def _process_loop(  # noqa: PLR0913
    f_out: BinaryIO,
    f_err: BinaryIO,
    f_in: BinaryIO,
    wasm_thread: threading.Thread,
    host_call_handler: HostCallHandler,
    result_container: dict[str, object],
) -> None:
    """Process the main communication loop with the sandbox."""
    select_timeout = 0.5
    stderr_buffer_size = 1024

    while True:
        if not wasm_thread.is_alive():
            if "runtime_error" in result_container:
                err = result_container["runtime_error"]
                if isinstance(err, BaseException):
                    raise err
            break

        rlist, _, _ = select.select([f_out, f_err], [], [], select_timeout)

        if f_err in rlist:
            _ = f_err.read(stderr_buffer_size)

        if f_out in rlist and not _process_output(
            f_out,
            f_in,
            host_call_handler,
            result_container,
        ):
            break


def _check_errors(result_container: dict[str, object]) -> None:
    """Check for errors in result container and raise if found."""
    if "runtime_error" in result_container:
        err = result_container["runtime_error"]
        if isinstance(err, BaseException):
            raise err

    if "error" in result_container:
        err = result_container["error"]
        if isinstance(err, SandboxExecutionError):
            raise err
