import { t, type TranslationKey } from "./i18n";
import { UgoiteApiError } from "./ugoite-client/protocol";

const compositionSaveDiagnosticKeys = {
  invalid_composition: "composition.diagnostic.invalid_composition",
  unsupported_format_version:
    "composition.diagnostic.unsupported_format_version",
} as const satisfies Record<string, TranslationKey>;

type CompositionSaveDiagnosticCode = keyof typeof compositionSaveDiagnosticKeys;

/** Localize only allowlisted Composition validation codes from composition.save. */
export const compositionSaveErrorMessage = (error: unknown): string => {
  if (
    !(error instanceof UgoiteApiError) ||
    error.operation !== "composition.save" ||
    error.kind !== "composition_diagnostic" ||
    error.status !== 422 ||
    typeof error.code !== "string" ||
    !Object.prototype.hasOwnProperty.call(
      compositionSaveDiagnosticKeys,
      error.code,
    )
  ) {
    return t("composition.saveFailed");
  }

  return t(
    compositionSaveDiagnosticKeys[
      error.code as CompositionSaveDiagnosticCode
    ],
  );
};
