import os
import json
import struct
from wasmtime import Store, Module, Linker, WasiConfig, Engine, Config, Func
from pathlib import Path
from typing import Any, Dict, Callable, Optional
import tempfile
import shutil
import threading
import select

SANDBOX_WASM_PATH = Path(__file__).parent / "sandbox.wasm"


class SandboxError(Exception):
    pass


class SandboxTimeout(SandboxError):
    pass


class SandboxExecutionError(SandboxError):
    pass


def run_script(
    code: str,
    host_call_handler: Callable[[str, str, Optional[Dict]], Any],
    fuel_limit: int = 100_000_000,
) -> Any:
    # Configure Engine
    config = Config()
    config.consume_fuel = True
    engine = Engine(config)
    store = Store(engine)
    store.set_fuel(fuel_limit)

    linker = Linker(engine)
    linker.define_wasi()

    tmp_dir = tempfile.mkdtemp()
    try:
        stdin_path = os.path.join(tmp_dir, "stdin.fifo")
        stdout_path = os.path.join(tmp_dir, "stdout.fifo")
        stderr_path = os.path.join(tmp_dir, "stderr.fifo")

        os.mkfifo(stdin_path)
        os.mkfifo(stdout_path)
        os.mkfifo(stderr_path)

        # Open pipes (Host side) BEFORE configuring WASI
        # This is crucial if Wasmtime opens the files immediately upon configuration.
        fd_in = os.open(stdin_path, os.O_RDWR)
        fd_out = os.open(stdout_path, os.O_RDWR)
        fd_err = os.open(stderr_path, os.O_RDWR)

        f_in = os.fdopen(fd_in, "wb", buffering=0)
        f_out = os.fdopen(fd_out, "rb", buffering=0)
        f_err = os.fdopen(fd_err, "rb", buffering=0)

        wasi = WasiConfig()
        wasi.stdin_file = stdin_path
        wasi.stdout_file = stdout_path
        wasi.stderr_file = stderr_path

        store.set_wasi(wasi)

        module = Module.from_file(engine, str(SANDBOX_WASM_PATH))

        result_container = {}

        def run_wasm():
            try:
                # Instantiate in the thread
                instance = linker.instantiate(store, module)
                exports = instance.exports(store)
                start_func = exports["_start"]
                if isinstance(start_func, Func):
                    start_func(store)
                else:
                    raise SandboxError("_start is not a function")
            except Exception as e:
                result_container["runtime_error"] = e

        wasm_thread = threading.Thread(target=run_wasm)
        wasm_thread.start()

        try:
            # 1. Send Code
            code_bytes = code.encode("utf-8")
            f_in.write(struct.pack(">I", len(code_bytes)))
            f_in.write(code_bytes)
            f_in.flush()

            # 2. Loop
            while True:
                # Check if thread is alive
                if not wasm_thread.is_alive():
                    if "runtime_error" in result_container:
                        raise result_container["runtime_error"]
                    # If thread died without error, maybe it finished?
                    # But we expect a result on stdout.
                    break

                # Wait for data on stdout or stderr
                rlist, _, _ = select.select([f_out, f_err], [], [], 0.5)

                if f_err in rlist:
                    err_data = f_err.read(1024)
                    if err_data:
                        # print(f"SANDBOX STDERR: {err_data.decode(errors='replace')}", file=sys.__stderr__)
                        pass

                if f_out in rlist:
                    # Read Magic (6 bytes)
                    magic = f_out.read(6)
                    if not magic:
                        # EOF on stdout
                        break

                    while len(magic) < 6:
                        chunk = f_out.read(6 - len(magic))
                        if not chunk:
                            break
                        magic += chunk

                    if len(magic) < 6:
                        break  # Unexpected EOF

                    if magic == b"\0HOST\0":
                        # Host Call
                        len_bytes = f_out.read(4)
                        while len(len_bytes) < 4:
                            chunk = f_out.read(4 - len(len_bytes))
                            if not chunk:
                                break
                            len_bytes += chunk

                        if len(len_bytes) < 4:
                            break

                        length = struct.unpack(">I", len_bytes)[0]
                        payload_bytes = b""
                        while len(payload_bytes) < length:
                            chunk = f_out.read(length - len(payload_bytes))
                            if not chunk:
                                break
                            payload_bytes += chunk

                        payload = json.loads(payload_bytes.decode("utf-8"))

                        # Execute Host Call
                        try:
                            resp = host_call_handler(
                                payload["method"], payload["path"], payload.get("body")
                            )
                            resp_str = json.dumps(resp)
                        except Exception as e:
                            resp_str = json.dumps({"error": str(e)})

                        resp_bytes = resp_str.encode("utf-8")
                        f_in.write(struct.pack(">I", len(resp_bytes)))
                        f_in.write(resp_bytes)
                        f_in.flush()

                    elif magic == b"\0RSLT\0":
                        # Result
                        len_bytes = f_out.read(4)
                        while len(len_bytes) < 4:
                            chunk = f_out.read(4 - len(len_bytes))
                            if not chunk:
                                break
                            len_bytes += chunk

                        length = struct.unpack(">I", len_bytes)[0]
                        res_bytes = b""
                        while len(res_bytes) < length:
                            chunk = f_out.read(length - len(res_bytes))
                            if not chunk:
                                break
                            res_bytes += chunk

                        result_container["result"] = json.loads(
                            res_bytes.decode("utf-8")
                        )
                        break

                    elif magic == b"\0ERRR\0":
                        # Error
                        len_bytes = f_out.read(4)
                        while len(len_bytes) < 4:
                            chunk = f_out.read(4 - len(len_bytes))
                            if not chunk:
                                break
                            len_bytes += chunk

                        length = struct.unpack(">I", len_bytes)[0]
                        err_bytes = b""
                        while len(err_bytes) < length:
                            chunk = f_out.read(length - len(err_bytes))
                            if not chunk:
                                break
                            err_bytes += chunk

                        result_container["error"] = SandboxExecutionError(
                            err_bytes.decode("utf-8")
                        )
                        break
                    else:
                        # Unknown magic
                        # print(f"Unknown magic: {magic}")
                        raise SandboxError(f"Invalid magic: {magic}")
        finally:
            f_in.close()
            f_out.close()
            f_err.close()

        wasm_thread.join(timeout=1)

        if "runtime_error" in result_container:
            raise result_container["runtime_error"]

        if "error" in result_container:
            raise result_container["error"]

        return result_container.get("result")

    finally:
        shutil.rmtree(tmp_dir)
