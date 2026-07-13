import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { describe, expect, it, vi } from "vitest";
import { SpaceSettings } from "./SpaceSettings";
import type { Space } from "~/lib/types";

const space: Space = { id: "demo", name: "Demo", created_at: "2026-01-01T00:00:00Z", storage_config: { uri: "s3://bucket/demo", endpoint: "https://s3.example.com" } };
describe("v5 SpaceSettings", () => {
  it("renders and saves General independently", async () => {
    const save = vi.fn().mockResolvedValue(undefined);
    render(() => <SpaceSettings space={space} section="general" onSave={save} />);
    fireEvent.input(screen.getByLabelText("Space Name"), { target: { value: "Renamed" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(save).toHaveBeenCalledWith({ name: "Renamed" }));
  });
  it("renders storage values and saves only storage configuration", async () => {
    const save = vi.fn().mockResolvedValue(undefined);
    render(() => <SpaceSettings space={space} section="storage" onSave={save} />);
    expect(screen.getByDisplayValue("s3://bucket/demo")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(save).toHaveBeenCalledWith({ storage_config: { uri: "s3://bucket/demo", endpoint: "https://s3.example.com" } }));
  });
  it("tests the edited storage connection", async () => {
    const test = vi.fn().mockResolvedValue({ status: "ok" });
    render(() => <SpaceSettings space={space} section="storage" onSave={vi.fn()} onTestConnection={test} />);
    fireEvent.click(screen.getByRole("button", { name: "Test Connection" }));
    await waitFor(() => expect(screen.getByText("Connection successful (ok)")).toBeInTheDocument());
  });
});
