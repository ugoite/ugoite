import { describe, expect, it } from "vitest";

import { buildEntryMarkdownByMode } from "~/lib/entry-input";
import type { Form } from "~/lib/types";

describe("buildEntryMarkdownByMode", () => {
  it("REQ-ENTRY-1872: adds the browser offset only for timezone-aware fields", () => {
    const formDef: Form = {
      name: "Event",
      version: 1,
      template: "# Event\n",
      fields: {
        Local: { type: "timestamp", required: false },
        Instant: { type: "timestamp_tz", required: false },
      },
    };
    const result = buildEntryMarkdownByMode(
      formDef,
      "Event",
      {
        Local: "2026-08-21T10:48",
        Instant: "2026-08-21T10:48",
      },
      "webform",
    );

    expect(result).toContain("## Local\n2026-08-21T10:48");
    expect(result).toMatch(
      /## Instant\n2026-08-21T10:48:00[+-]\d{2}:\d{2}/,
    );
  });

  it("REQ-ENTRY-1872: preserves an explicit timezone value", () => {
    const formDef: Form = {
      name: "Event",
      version: 1,
      template: "# Event\n",
      fields: { Instant: { type: "timestamp_tz", required: false } },
    };
    const result = buildEntryMarkdownByMode(
      formDef,
      "Event",
      { Instant: "2026-08-21T10:48:00+09:00" },
      "webform",
    );

    expect(result).toContain("## Instant\n2026-08-21T10:48:00+09:00");
  });

  it("REQ-FE-037: preserves user markdown whitespace in markdown mode", () => {
    const formDef: Form = {
      name: "Meeting",
      version: 1,
      template: "# Meeting\n\n## Date\n",
      fields: {
        Date: { type: "date", required: true },
      },
    };

    const markdown =
      "# Entry\n\n---\nform: Meeting\n---\n\n## Date\n2026-02-14\n";
    const result = buildEntryMarkdownByMode(formDef, "Entry", {
      __markdown: markdown,
    }, "markdown");

    expect(result).toBe(markdown);
  });

  it("REQ-FE-037: builds from fields when __markdown is empty in markdown mode", () => {
    const formDef: Form = {
      name: "Task",
      version: 1,
      template: "# Task\n\n## Status\n",
      fields: { Status: { type: "text" } },
    };
    const result = buildEntryMarkdownByMode(
      formDef as Form,
      "My Task",
      { __markdown: "" },
      "markdown",
    );
    expect(result).toContain("My Task");
  });
});
