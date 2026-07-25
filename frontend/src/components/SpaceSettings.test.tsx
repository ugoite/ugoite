import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { SpaceSettings } from "./SpaceSettings";
import { setLocale } from "~/lib/i18n";
import type { Space } from "~/lib/types";

const space: Space = {
  id: "demo",
  name: "Demo",
  created_at: "2026-01-01T00:00:00Z",
  storage: { type: "local", root: "/var/lib/ugoite/demo" },
  storage_config: {
    uri: "s3://bucket/demo",
    endpoint: "https://s3.example.com",
  },
  settings: { default_form: "Notes" },
};
describe("v5 SpaceSettings", () => {
  beforeEach(() => setLocale("en"));
  it("renders and saves General independently", async () => {
    const save = vi.fn().mockResolvedValue(undefined);
    render(() => (
      <SpaceSettings
        space={space}
        section="general"
        onSave={save}
      />
    ));
    fireEvent.input(screen.getByLabelText("Space Name"), {
      target: { value: "Renamed" },
    });
    fireEvent.input(screen.getByLabelText("Default Form"), {
      target: { value: "Meeting" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() =>
      expect(save).toHaveBeenCalledWith({
        name: "Renamed",
        settings: { default_form: "Meeting" },
      })
    );
    expect(screen.queryByLabelText("Timezone")).not.toBeInTheDocument();
  });
  it("renders storage values and saves only storage configuration", async () => {
    const save = vi.fn().mockResolvedValue(undefined);
    render(() => (
      <SpaceSettings
        space={space}
        section="storage"
        onSave={save}
      />
    ));
    expect(screen.getByText("Storage topology")).toBeInTheDocument();
    expect(screen.getByText("Local filesystem")).toBeInTheDocument();
    expect(screen.getByText("file:///var/lib/ugoite/demo")).toBeInTheDocument();
    expect(screen.getByText(/migration metadata only/i)).toBeInTheDocument();
    expect(screen.getByDisplayValue("s3://bucket/demo")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() =>
      expect(save).toHaveBeenCalledWith({
        storage_config: {
          uri: "s3://bucket/demo",
          endpoint: "https://s3.example.com",
        },
      })
    );
  });
  it("tests the edited storage connection", async () => {
    const test = vi.fn().mockResolvedValue({ status: "ok" });
    render(() => (
      <SpaceSettings
        space={space}
        section="storage"
        onSave={vi.fn()}
        onTestConnection={test}
      />
    ));
    fireEvent.click(screen.getByRole("button", { name: "Test Connection" }));
    await waitFor(() =>
      expect(screen.getByText("Connection successful (ok)")).toBeInTheDocument()
    );
  });
  it("clears an S3 endpoint when changing to local storage metadata", async () => {
    const save = vi.fn().mockResolvedValue(undefined);
    render(() => (
      <SpaceSettings
        space={space}
        section="storage"
        onSave={save}
      />
    ));

    fireEvent.input(screen.getByDisplayValue("s3://bucket/demo"), {
      target: { value: "file:///data/demo" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() =>
      expect(save).toHaveBeenCalledWith({
        storage_config: { uri: "file:///data/demo" },
      })
    );
  });
});
