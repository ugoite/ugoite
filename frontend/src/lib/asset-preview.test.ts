import { describe, expect, it } from "vitest";
import {
  formatJsonPreview,
  MAX_PREVIEW_COLUMNS,
  MAX_PREVIEW_ROWS,
  parseDelimitedPreview,
  previewMediaType,
  readPreviewText,
  resolvePreviewKind,
} from "./asset-preview";

const reference = (name: string, media_type: string) => ({
  name,
  media_type,
});

describe("asset preview resolution", () => {
  it("selects browser-native formats using the MIME type and extension", () => {
    expect(resolvePreviewKind(reference("photo.webp", "image/webp")))
      .toBe("image");
    expect(resolvePreviewKind(reference("report.pdf", "application/pdf")))
      .toBe("pdf");
    expect(resolvePreviewKind(reference("notes.md", "text/markdown")))
      .toBe("markdown");
    expect(resolvePreviewKind(reference("data.json", "application/json")))
      .toBe("json");
    expect(
      resolvePreviewKind(reference("data.tsv", "text/tab-separated-values")),
    )
      .toBe("csv");
    expect(resolvePreviewKind(reference("main.rs", "text/plain"))).toBe("text");
    expect(resolvePreviewKind(reference("config.xml", "application/xml")))
      .toBe("text");
    expect(resolvePreviewKind(reference("recording.mp3", "audio/mpeg")))
      .toBe("audio");
    expect(resolvePreviewKind(reference("clip.webm", "video/webm")))
      .toBe("video");
    expect(
      resolvePreviewKind(reference("logo.png", "application/octet-stream")),
    )
      .toBe("image");
    expect(
      resolvePreviewKind(reference("report.pdf", "application/octet-stream")),
    )
      .toBe("pdf");
    expect(
      resolvePreviewKind(
        reference("recording.mp3", "application/octet-stream"),
      ),
    )
      .toBe("audio");
    expect(
      resolvePreviewKind(reference("clip.mp4", "application/octet-stream")),
    )
      .toBe("video");
    expect(previewMediaType(reference("logo.png", "application/octet-stream")))
      .toBe("image/png");
    expect(
      previewMediaType(reference("report.pdf", "application/octet-stream")),
    )
      .toBe("application/pdf");
    expect(previewMediaType(reference("data.tsv", "application/octet-stream")))
      .toBe("text/tab-separated-values");
  });

  it("rejects active markup and mismatched formats", () => {
    expect(resolvePreviewKind(reference("diagram.svg", "image/svg+xml")))
      .toBe("unsupported");
    expect(resolvePreviewKind(reference("payload.html", "text/html")))
      .toBe("unsupported");
    expect(resolvePreviewKind(reference("payload.txt", "text/html")))
      .toBe("unsupported");
    expect(resolvePreviewKind(reference("photo.png", "application/pdf")))
      .toBe("unsupported");
    expect(
      resolvePreviewKind(reference("book.docx", "application/octet-stream")),
    )
      .toBe("unsupported");
  });
});

describe("bounded asset preview helpers", () => {
  it("formats valid JSON and leaves invalid JSON readable", () => {
    expect(formatJsonPreview('{"name":"Ugoite","count":2}')).toContain(
      '\n  "name": "Ugoite",',
    );
    expect(formatJsonPreview("not json")).toBe("not json");
    expect(formatJsonPreview(`{"value":"${"x".repeat(512 * 1024)}"}`))
      .toHaveLength(512 * 1024);
  });

  it("parses quoted CSV fields and bounds rows and columns", () => {
    const rows = [
      "Name,Note",
      '"A, Inc.","Line one',
      'line two"',
      ...Array.from(
        { length: MAX_PREVIEW_ROWS + 5 },
        (_, index) =>
          `${index},${
            Array.from({ length: MAX_PREVIEW_COLUMNS + 2 }, () => "x").join(",")
          }`,
      ),
    ].join("\n");
    const preview = parseDelimitedPreview(rows, ",");

    expect(preview.rows[1]).toEqual(["A, Inc.", "Line one\nline two"]);
    expect(preview.rows).toHaveLength(MAX_PREVIEW_ROWS);
    expect(preview.rows.every((row) => row.length <= MAX_PREVIEW_COLUMNS))
      .toBe(true);
    expect(preview.truncatedRows).toBe(true);
    expect(preview.truncatedColumns).toBe(true);
  });

  it("reads only the bounded text prefix", async () => {
    const blob = new Blob(["x".repeat(600_000)], { type: "text/plain" });
    const text = await readPreviewText(blob);
    expect(text).toHaveLength(512 * 1024);
  });
});
