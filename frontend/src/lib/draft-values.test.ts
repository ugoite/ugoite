import { describe, expect, it } from "vitest";
import {
  draftValueToDisplayString,
  isCanonicalAssetValue,
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
  name: "Project",
  version: 1,
  template: "# Project",
  fields: {
    Title: { type: "string", required: true },
    Done: { type: "boolean", required: false },
    Count: { type: "integer", required: false },
    Tags: { type: "list", required: false },
    Rows: { type: "object_list", required: false },
    Ref: { type: "row_reference", required: false, target_form: "Task" },
    File: { type: "asset_reference", required: false },
    Files: {
      type: "list",
      required: false,
      items: { type: "asset_reference" },
    },
  },
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

  it("rejects invalid AssetReference through the shared validator", async () => {
    const result = await validateEntryDraftViaWasm(typedForm(), {
      title: "T",
      tags: [],
      fields: {
        Title: "T",
        File: { asset_id: "not-a-uuid" } as unknown as Record<string, unknown>,
      },
    });
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
    );
    const reparsed = await parseSourceToDraftViaWasm(source, "Website");
    const first = await validateEntryDraftViaWasm(form, {
      title: "Website",
      tags: [],
      fields,
    });
    const second = await validateEntryDraftViaWasm(form, {
      title: reparsed.title,
      tags: reparsed.tags,
      fields: reparsed.fields,
    });
    expect(first.ok).toBe(true);
    expect(second.ok).toBe(true);
    if (first.ok && second.ok) {
      expect(second.normalized).toEqual(first.normalized);
    }
  });
});
