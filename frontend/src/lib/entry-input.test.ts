import { describe, expect, it } from "vitest";

import {
  buildStructuredEntryFields,
} from "~/lib/entry-input";
import type { AssetReference } from "~/lib/types";
import type { Form } from "~/lib/types";

const assetRef: AssetReference = {
  asset_id: "01900000-0000-7000-8000-000000000001",
  name: "report.pdf",
  media_type: "application/pdf",
  size_bytes: 10,
  sha256: "a".repeat(64),
};

describe("structured entry draft", () => {
  it("builds structured fields without Markdown rendering", () => {
    const formDef: Form = {
      name: "Note",
      version: 1,
      template: "# Note\n",
      fields: {
        Body: { type: "string", required: true },
        Done: { type: "boolean", required: false },
      },
    };
    const fields = buildStructuredEntryFields(formDef, {
      Body: "hello",
      Done: "yes",
      __control: "ignored",
      Empty: "   ",
    });
    expect(fields).toEqual({ Body: "hello", Done: "yes" });
  });

  it("keeps typed values and shared control normalization for webform inputs", () => {
    const formDef: Form = {
      name: "Entry",
      version: 1,
      template: "# Entry\n",
      fields: {
        Body: { type: "string", required: false },
        Zoned: { type: "timestamp_tz", required: false },
        Row: { type: "row_reference", required: false },
        File: { type: "asset_reference", required: false },
        Tags: { type: "list", required: false },
        Rows: { type: "object_list", required: false },
      },
    };
    const fields = buildStructuredEntryFields(formDef, {
      Body: "  hello  ",
      Zoned: "2026-08-21T10:48",
      Row: "entry-01",
      File: assetRef,
      Tags: ["alpha", "beta"],
      Rows: [{ label: "one" }],
      __control: "drop",
      Blank: "   ",
    });

    expect(fields.Body).toBe("hello");
    expect(fields.Zoned).toMatch(
      /^2026-08-21T10:48:00[+-]\d{2}:\d{2}$/,
    );
    expect(fields.Row).toBe("entry-01");
    expect(fields.File).toEqual(assetRef);
    expect(fields.Tags).toEqual(["alpha", "beta"]);
    expect(fields.Rows).toEqual([{ label: "one" }]);
    expect(fields.__control).toBeUndefined();
    expect(fields.Blank).toBeUndefined();
  });

});
