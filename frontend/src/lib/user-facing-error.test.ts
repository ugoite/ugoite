import { afterEach, describe, expect, it } from "vitest";
import { setLocale } from "./i18n";
import { formatUserFacingError } from "./user-facing-error";
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
});
