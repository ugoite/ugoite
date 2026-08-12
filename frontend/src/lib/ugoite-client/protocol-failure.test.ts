import { afterEach, describe, expect, it, vi } from "vitest";

const wasmModule = "../generated/ugoite_wasm.wasm?init";

const fakeInstance = (
  protocolVersion: number,
  invocationStatus: number,
) => {
  const memory = new WebAssembly.Memory({ initial: 1 });
  const exports = {
    memory,
    ugoite_protocol_version: () => protocolVersion,
    ugoite_alloc: () => 0,
    ugoite_dealloc: vi.fn(),
    ugoite_protocol_invoke: () => invocationStatus,
    ugoite_protocol_result_pointer: () => 0,
    ugoite_protocol_result_length: () => 0,
    ugoite_protocol_clear_result: vi.fn(),
  };
  return { exports };
};

afterEach(() => {
  vi.doUnmock(wasmModule);
  vi.resetModules();
});

describe("portable protocol WASM failure handling", () => {
  it("rejects an incompatible WASM protocol version", async () => {
    vi.doMock(wasmModule, () => ({
      default: () => Promise.resolve(fakeInstance(99, 0)),
    }));
    const { getWasmSupportedOperations } = await import("./protocol");

    await expect(getWasmSupportedOperations()).rejects.toThrow(
      "Unsupported Ugoite WASM protocol version: 99",
    );
  });

  it("rejects a non-zero WASM protocol invocation status and cleans up", async () => {
    const instance = fakeInstance(1, 7);
    vi.doMock(wasmModule, () => ({
      default: () => Promise.resolve(instance),
    }));
    const { getWasmSupportedOperations } = await import("./protocol");

    await expect(getWasmSupportedOperations()).rejects.toThrow(
      "Ugoite WASM protocol invocation failed: 7",
    );
    expect(instance.exports.ugoite_dealloc).toHaveBeenCalledOnce();
    expect(instance.exports.ugoite_protocol_clear_result)
      .toHaveBeenCalledOnce();
  });
});
