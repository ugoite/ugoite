import { afterEach, describe, expect, it } from "vitest";
import { setLocale } from "./i18n";
import {
  formatMarkdownConversionDiagnostic,
  formatUserFacingError,
} from "./user-facing-error";
import { UgoiteApiError } from "./ugoite-client/protocol";

describe("formatUserFacingError", () => {
  afterEach(() => setLocale("en"));

  it("maps a known API code before appending server detail", () => {
    setLocale("ja");
    const error = new UgoiteApiError({
      kind: "api",
      code: "SPACE_NOT_FOUND",
      operation: "space.get",
      message: "Failed to load space: Space not found",
      detail: { code: "SPACE_NOT_FOUND", message: "Space not found: demo" },
    });

    const message = formatUserFacingError(error, "settings.unknownError");
    expect(message).toContain("スペースが見つかりません。");
    expect(message).toContain("Space not found: demo");
  });

  it("uses a known operation when a response has no code", () => {
    setLocale("ja");
    const error = new UgoiteApiError({
      kind: "api",
      operation: "search.keyword",
      message: "Failed to search entries: backend detail",
      detail: "backend detail",
    });

    expect(formatUserFacingError(error, "searchPage.error.searchFailed"))
      .toContain("検索に失敗しました。");
  });

  it("keeps an unknown string detail next to the localized fallback", () => {
    setLocale("ja");
    expect(
      formatUserFacingError(
        "Unknown backend detail",
        "searchPage.error.searchFailed",
      ),
    ).toContain("Unknown backend detail");
  });

  it("uses the typed status when code and operation are absent", () => {
    setLocale("ja");
    const error = new UgoiteApiError({
      kind: "api",
      status: 503,
      message: "Service unavailable",
    });

    expect(formatUserFacingError(error, "settings.unknownError"))
      .toContain("必要なサービスを利用できません。");
  });

  it("localizes the Rust Markdown conversion diagnostic codes", () => {
    setLocale("ja");
    expect(formatMarkdownConversionDiagnostic({
      code: "markdown_frontmatter_invalid",
      message: "backend detail",
    })).toBe("Markdownのfrontmatterが正しくありません。");
    expect(formatMarkdownConversionDiagnostic({
      code: "markdown_frontmatter_unclosed",
      message: "backend detail",
    })).toBe("Markdownのfrontmatterが閉じられていません。");
    expect(formatMarkdownConversionDiagnostic({
      code: "markdown_duplicate_field_section",
      message: "backend detail",
    })).toBe("Markdownに同じフィールドのセクションが重複しています。");
    expect(formatMarkdownConversionDiagnostic({
      code: "markdown_unassigned_preamble",
      message: "backend detail",
    })).toBe("最初のフィールドより前のMarkdown本文を割り当てられません。");
  });
});
