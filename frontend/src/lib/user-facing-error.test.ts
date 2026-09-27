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
      operation: "entry.query",
      message: "Failed to query entries: backend detail",
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

  it("gives an unknown write outcome a no-blind-retry instruction", () => {
    setLocale("ja");
    const error = new UgoiteApiError({
      kind: "transport",
      operation: "entry.update",
      message: "connection reset",
    });

    expect(error.mutationOutcome).toBe("unknown");
    expect(error.mutationCode).toBe("MUTATION_OUTCOME_UNKNOWN");
    expect(formatUserFacingError(error, "entryDetail.saveFailed"))
      .toContain("再試行する前に履歴または対象の項目を確認してください。");
  });

  it("distinguishes invalid success receipts from rejected writes", () => {
    const error = new UgoiteApiError({
      kind: "invalid_response",
      operation: "form.upsert",
      status: 200,
      message: "invalid receipt",
    });
    expect(error.mutationOutcome).toBe("receipt_invalid");
    expect(error.mutationCode).toBe("MUTATION_RECEIPT_INVALID");
    expect(formatUserFacingError(error, "entryDetail.saveFailed"))
      .toContain("invalid receipt");
  });

  it("uses the same error boundary for Konase outcome errors", () => {
    setLocale("ja");
    const error = Object.assign(new Error("unconfirmed"), {
      mutationOutcome: "unknown" as const,
      mutationCode: "MUTATION_OUTCOME_UNKNOWN",
    });
    expect(formatUserFacingError(error, "konase.error"))
      .toContain("再試行する前に履歴または対象の項目を確認してください。");
  });
});
