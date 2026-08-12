import type { AssetReference } from "./types";

export type AssetPreviewKind =
  | "image"
  | "pdf"
  | "text"
  | "json"
  | "markdown"
  | "csv"
  | "audio"
  | "video"
  | "unsupported";

export const MAX_PREVIEW_BYTES = 512 * 1024;
export const MAX_PREVIEW_ROWS = 200;
export const MAX_PREVIEW_COLUMNS = 50;

const IMAGE_EXTENSIONS = new Set([
  "avif",
  "bmp",
  "gif",
  "jpeg",
  "jpg",
  "png",
  "webp",
]);
const AUDIO_EXTENSIONS = new Set([
  "aac",
  "flac",
  "m4a",
  "mp3",
  "oga",
  "ogg",
  "wav",
  "weba",
]);
const VIDEO_EXTENSIONS = new Set([
  "avi",
  "m4v",
  "mkv",
  "mov",
  "mp4",
  "mpeg",
  "ogv",
  "webm",
]);
const PREVIEW_MEDIA_TYPES: Record<string, string> = {
  avif: "image/avif",
  bmp: "image/bmp",
  gif: "image/gif",
  jpeg: "image/jpeg",
  jpg: "image/jpeg",
  png: "image/png",
  webp: "image/webp",
  aac: "audio/aac",
  flac: "audio/flac",
  m4a: "audio/mp4",
  mp3: "audio/mpeg",
  oga: "audio/ogg",
  ogg: "audio/ogg",
  wav: "audio/wav",
  weba: "audio/webm",
  avi: "video/x-msvideo",
  m4v: "video/x-m4v",
  mkv: "video/x-matroska",
  mov: "video/quicktime",
  mp4: "video/mp4",
  mpeg: "video/mpeg",
  ogv: "video/ogg",
  webm: "video/webm",
};
const SOURCE_EXTENSIONS = new Set([
  "c",
  "cc",
  "cpp",
  "css",
  "go",
  "h",
  "hpp",
  "ini",
  "java",
  "js",
  "jsx",
  "kt",
  "lock",
  "log",
  "lua",
  "py",
  "rs",
  "sql",
  "svelte",
  "toml",
  "ts",
  "tsx",
  "txt",
  "vue",
  "xml",
  "yaml",
  "yml",
]);

const normalizedMediaType = (mediaType: string) =>
  mediaType.split(";", 1)[0]?.trim().toLowerCase() ?? "";

const extensionOf = (name: string) => {
  const lastSegment = name.split(/[\\/]/).pop() ?? "";
  const dot = lastSegment.lastIndexOf(".");
  return dot < 0 ? "" : lastSegment.slice(dot + 1).toLowerCase();
};

const isTextMediaType = (mediaType: string) =>
  mediaType.startsWith("text/") ||
  mediaType === "application/octet-stream" ||
  mediaType === "application/javascript" ||
  mediaType === "application/json" ||
  mediaType === "application/ld+json" ||
  mediaType === "application/xml" ||
  mediaType === "application/toml" ||
  mediaType === "application/typescript" ||
  mediaType === "application/x-javascript" ||
  mediaType === "application/x-yaml" ||
  mediaType === "application/yaml" ||
  mediaType.endsWith("+json");

const isGenericMediaType = (mediaType: string) =>
  mediaType === "application/octet-stream";

/**
 * Select only formats that can be rendered safely with browser primitives.
 * The extension gate prevents a misleading MIME type from turning HTML or
 * SVG into active content; all text and Markdown output is rendered as text
 * or through the escaping Markdown helper.
 */
export function resolvePreviewKind(
  reference: Pick<AssetReference, "name" | "media_type">,
): AssetPreviewKind {
  const extension = extensionOf(reference.name);
  const mediaType = normalizedMediaType(reference.media_type);

  if (
    extension === "svg" ||
    mediaType === "image/svg+xml" ||
    extension === "html" ||
    extension === "htm" ||
    mediaType === "text/html" ||
    mediaType === "application/xhtml+xml"
  ) {
    return "unsupported";
  }
  if (
    extension === "pdf" &&
    (mediaType === "application/pdf" || isGenericMediaType(mediaType))
  ) return "pdf";
  if (
    IMAGE_EXTENSIONS.has(extension) &&
    (mediaType.startsWith("image/") || isGenericMediaType(mediaType))
  ) {
    return "image";
  }
  if (
    extension === "json" &&
    (mediaType === "application/json" ||
      mediaType === "text/json" ||
      mediaType.endsWith("+json") ||
      mediaType === "application/octet-stream" ||
      mediaType === "text/plain")
  ) {
    return "json";
  }
  if (
    (extension === "md" || extension === "markdown" || extension === "mdown") &&
    isTextMediaType(mediaType)
  ) {
    return "markdown";
  }
  if (
    (extension === "csv" || extension === "tsv") &&
    isTextMediaType(mediaType)
  ) {
    return "csv";
  }
  if (SOURCE_EXTENSIONS.has(extension) && isTextMediaType(mediaType)) {
    return "text";
  }
  if (
    AUDIO_EXTENSIONS.has(extension) &&
    (mediaType.startsWith("audio/") || isGenericMediaType(mediaType))
  ) {
    return "audio";
  }
  if (
    VIDEO_EXTENSIONS.has(extension) &&
    (mediaType.startsWith("video/") || isGenericMediaType(mediaType))
  ) {
    return "video";
  }
  return "unsupported";
}

/** Return a browser-renderable type for generic-MIME references when safe. */
export function previewMediaType(
  reference: Pick<AssetReference, "name" | "media_type">,
) {
  const mediaType = normalizedMediaType(reference.media_type);
  if (mediaType !== "application/octet-stream") return reference.media_type;

  const extension = extensionOf(reference.name);
  const kind = resolvePreviewKind(reference);
  if (kind === "pdf") return "application/pdf";
  if (kind === "json") return "application/json";
  if (kind === "markdown") return "text/markdown";
  if (kind === "csv") {
    return extension === "tsv" ? "text/tab-separated-values" : "text/csv";
  }
  if (kind === "text") return "text/plain";
  if (kind === "image" || kind === "audio" || kind === "video") {
    return PREVIEW_MEDIA_TYPES[extension] ?? mediaType;
  }
  return mediaType;
}

export async function readPreviewText(blob: Blob) {
  return await blob.slice(0, MAX_PREVIEW_BYTES).text();
}

const capPreviewText = (text: string) => text.slice(0, MAX_PREVIEW_BYTES);

export function formatJsonPreview(text: string) {
  try {
    return capPreviewText(JSON.stringify(JSON.parse(text), null, 2));
  } catch {
    return capPreviewText(text);
  }
}

export type PreviewTable = {
  rows: string[][];
  truncatedRows: boolean;
  truncatedColumns: boolean;
};

/** Parse a bounded CSV/TSV preview without introducing a parser dependency. */
export function parseDelimitedPreview(
  text: string,
  delimiter: "," | "\t",
): PreviewTable {
  const rows: string[][] = [];
  let row: string[] = [];
  let field = "";
  let quoted = false;

  const pushField = () => {
    row.push(field);
    field = "";
  };
  const pushRow = () => {
    // A final newline is a record terminator, not an extra empty row.
    if (row.length > 0 || field !== "" || rows.length === 0) {
      pushField();
      rows.push(row);
    }
    row = [];
  };

  for (let index = 0; index < text.length; index += 1) {
    const character = text[index];
    if (quoted) {
      if (character === '"') {
        if (text[index + 1] === '"') {
          field += '"';
          index += 1;
        } else {
          quoted = false;
        }
      } else {
        field += character;
      }
      continue;
    }
    if (character === '"' && field === "") {
      quoted = true;
    } else if (character === delimiter) {
      pushField();
    } else if (character === "\n") {
      pushRow();
    } else if (character !== "\r") {
      field += character;
    }
  }
  if (quoted || field !== "" || row.length > 0) pushRow();

  const truncatedRows = rows.length > MAX_PREVIEW_ROWS;
  const visibleRows = rows.slice(0, MAX_PREVIEW_ROWS);
  const truncatedColumns = visibleRows.some((candidate) =>
    candidate.length > MAX_PREVIEW_COLUMNS
  );
  return {
    rows: visibleRows.map((candidate) =>
      candidate.slice(0, MAX_PREVIEW_COLUMNS)
    ),
    truncatedRows,
    truncatedColumns,
  };
}
