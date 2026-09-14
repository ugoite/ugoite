import { describe, expect, it } from "vitest";
import {
  draftValueToDisplayString,
  isCanonicalAssetValue,
  readAssetReferences,
  toTransportFields,
} from "~/lib/draft-values";
import { validateEntryDraftViaWasm } from "~/lib/entry-validation";
import type { Form } from "~/lib/types";

const assetRef = {
  asset_id: "01900000-0000-7000-8000-000000000001",
  name: "report.pdf",
  media_type: "application/pdf",
  size_bytes: 10,
  sha256: "a".repeat(64),
};

const typedForm = (): Form => ({
  id: "00000000-0000-0000-0000-000000000001",
  name: "Project",
  version: 1,
  template: "# Project",
  fields: {
    Title: { id: 100, type: "string", required: true },
    Done: { id: 101, type: "boolean", required: false },
    Count: { id: 102, type: "integer", required: false },
    Tags: { id: 103, type: "list", required: false },
    Rows: { id: 104, type: "object_list", required: false },
    Ref: {
      id: 105,
      type: "row_reference",
      required: false,
      target_form: "Task",
    },
    File: { id: 106, type: "asset_reference", required: false },
    Files: {
      id: 107,
      type: "list",
      required: false,
      items: { type: "asset_reference" },
    },
  },
});

const taskForm = (): Form => ({
  id: "00000000-0000-0000-0000-000000000002",
  name: "Task",
  version: 1,
  template: "# Task",
  fields: {},
});

describe("draft-values", () => {
  it("displays scalars and collections without deciding semantics", () => {
    expect(draftValueToDisplayString("hello")).toBe("hello");
    expect(draftValueToDisplayString(true)).toBe("true");
    expect(draftValueToDisplayString(3)).toBe("3");
    expect(draftValueToDisplayString(["alpha", "beta"])).toBe(
      "- alpha\n- beta",
    );
    expect(draftValueToDisplayString(null)).toBe("");
    expect(draftValueToDisplayString(undefined)).toBe("");
  });

  it("keeps asset and reference values typed at the transport boundary", () => {
    const transported = toTransportFields(typedForm(), {
      Title: "Website",
      Done: true,
      Count: 3,
      Tags: ["alpha", "beta"],
      Rows: [{ step: "one" }],
      Ref: "task-01",
      File: { ...assetRef },
      Files: [{ ...assetRef }],
      __control: "drop me",
      Blank: "   ",
    });
    expect(transported.Title).toBe("Website");
    expect(transported.Done).toBe(true);
    expect(transported.Count).toBe(3);
    expect(transported.Tags).toEqual(["alpha", "beta"]);
    expect(transported.Rows).toEqual([{ step: "one" }]);
    // Row references stay stable IDs; display titles never leak in.
    expect(transported.Ref).toBe("task-01");
    expect(transported.File).toEqual(assetRef);
    expect(transported.Files).toEqual([assetRef]);
    expect(transported.__control).toBeUndefined();
    expect(transported.Blank).toBeUndefined();
    expect(isCanonicalAssetValue({ ...assetRef })).toBe(true);
    expect(isCanonicalAssetValue("not-an-asset")).toBe(false);
  });

  it("parses legacy asset JSON strings only at the bridge, not in components", () => {
    const transported = toTransportFields(typedForm(), {
      Title: "T",
      File: JSON.stringify(assetRef),
      Files: JSON.stringify([assetRef]),
    });
    expect(transported.File).toEqual(assetRef);
    expect(transported.Files).toEqual([assetRef]);
  });

  it("shares asset reference presence, invalid, and duplicate semantics", () => {
    expect(readAssetReferences(undefined, false)).toEqual({ references: [] });
    expect(readAssetReferences("   ", true)).toEqual({ references: [] });
    expect(readAssetReferences([], true)).toEqual({ references: [] });
    expect(readAssetReferences([assetRef], true)).toEqual({
      references: [assetRef],
    });
    expect(readAssetReferences("not-json", false).issue).toBe("invalid");
    expect(readAssetReferences([{}], true).issue).toBe("invalid");
    expect(readAssetReferences([assetRef, assetRef], true).issue).toBe(
      "duplicate",
    );

    const transported = toTransportFields(typedForm(), {
      Tags: [],
      Files: [],
    });
    // Empty generic lists are meaningful typed values; an empty asset list is
    // omitted so the Rust boundary sees the same absence as an empty control.
    expect(transported.Tags).toEqual([]);
    expect(transported.Files).toBeUndefined();
  });

  it("rejects invalid AssetReference through the shared validator", async () => {
    const result = await validateEntryDraftViaWasm(typedForm(), {
      title: "T",
      tags: [],
      fields: {
        Title: "T",
        File: { asset_id: "not-a-uuid" } as unknown as Record<string, unknown>,
      },
    }, [taskForm()]);
    expect(result.ok).toBe(false);
    if (!result.ok) {
      expect(result.code).toBe("FORM_VALIDATION_FAILED");
      expect(result.invalidFields).toEqual(["File"]);
    }
  });

  it("round-trips typed asset/reference/list values losslessly", async () => {
    const { renderDraftToSourceViaWasm, parseSourceToDraftViaWasm } =
      await import("~/lib/entry-compat");
    const { toTransportFields } = await import("~/lib/draft-values");
    const form = typedForm();
    const knownForms = [taskForm()];
    const fields = {
      Title: "Website",
      Ref: "task-01",
      Tags: ["alpha", "beta"],
      Rows: [{ step: "one" }],
      File: { ...assetRef },
      Files: [{ ...assetRef }],
    };
    // Wire keeps kinds: transport once here so the round-trip starts from
    // the same typed normalized baseline the save path uses.
    const transported = toTransportFields(
      form,
      fields as unknown as Parameters<typeof toTransportFields>[1],
    );
    expect(transported.File).toEqual(assetRef);
    const source = await renderDraftToSourceViaWasm(
      form,
      "Website",
      [],
      fields,
      knownForms,
    );
    const reparsed = await parseSourceToDraftViaWasm(source, "Website");
    const first = await validateEntryDraftViaWasm(form, {
      title: "Website",
      tags: [],
      fields,
    }, knownForms);
    const second = await validateEntryDraftViaWasm(form, {
      title: reparsed.title,
      tags: reparsed.tags,
      fields: reparsed.fields,
    }, knownForms);
    expect(first.ok).toBe(true);
    expect(second.ok).toBe(true);
    if (first.ok && second.ok) {
      expect(second.normalized).toEqual(first.normalized);
    }
  });
});
