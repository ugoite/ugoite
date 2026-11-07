import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  publishCompositionAccessChange,
  subscribeToCompositionAccessChanges,
} from "./composition-access-changes";

class TestBroadcastChannel {
  static instances: TestBroadcastChannel[] = [];
  onmessage: ((event: MessageEvent<unknown>) => void) | null = null;
  posted: unknown[] = [];
  closed = false;

  constructor(readonly name: string) {
    TestBroadcastChannel.instances.push(this);
  }

  postMessage(message: unknown): void {
    this.posted.push(message);
    for (const receiver of TestBroadcastChannel.instances) {
      if (
        receiver !== this && receiver.name === this.name && !receiver.closed
      ) {
        receiver.receive(structuredClone(message));
      }
    }
  }

  close(): void {
    this.closed = true;
  }

  receive(data: unknown): void {
    this.onmessage?.({ data } as MessageEvent<unknown>);
  }
}

describe("Composition access change broadcasts", () => {
  const originalBroadcastChannel = globalThis.BroadcastChannel;

  beforeEach(() => {
    TestBroadcastChannel.instances = [];
    vi.stubGlobal("BroadcastChannel", TestBroadcastChannel);
  });

  afterEach(() => {
    vi.stubGlobal("BroadcastChannel", originalBroadcastChannel);
  });

  it("publishes only a Space identifier and closes the sender channel", () => {
    publishCompositionAccessChange("space-1");

    expect(TestBroadcastChannel.instances).toHaveLength(1);
    expect(TestBroadcastChannel.instances[0].name).toBe(
      "ugoite:composition-access-changes",
    );
    expect(TestBroadcastChannel.instances[0].posted).toEqual([{
      kind: "space-access-changed",
      space_id: "space-1",
    }]);
    expect(TestBroadcastChannel.instances[0].closed).toBe(true);
  });

  it("delivers a published change to another same-origin tab channel", () => {
    const changed = vi.fn();
    const unsubscribe = subscribeToCompositionAccessChanges(changed);

    publishCompositionAccessChange("space-1");

    expect(changed).toHaveBeenCalledOnce();
    expect(changed).toHaveBeenCalledWith("space-1");
    unsubscribe();
  });

  it("delivers valid same-origin Space signals and ignores malformed messages", () => {
    const changed = vi.fn();
    const unsubscribe = subscribeToCompositionAccessChanges(changed);
    const receiver = TestBroadcastChannel.instances[0];

    receiver.receive({ kind: "space-access-changed", space_id: "space-2" });
    receiver.receive({ kind: "other", space_id: "space-3" });
    receiver.receive({ kind: "space-access-changed", space_id: "" });
    receiver.receive(null);

    expect(changed).toHaveBeenCalledOnce();
    expect(changed).toHaveBeenCalledWith("space-2");
    unsubscribe();
    expect(receiver.closed).toBe(true);
  });
});
