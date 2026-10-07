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

/**
 * Localize shared-contract diagnostics from draft canonicalization (the
 * async half of the save-readiness gate). Only allowlisted composition
 * diagnostic codes resolve to the localized summary; transport failures and
 * unknown codes fall back to the generic save failure. Never throws.
 */
export const compositionCanonicalizeErrorMessage = (
  error: unknown,
): string => {
  if (
    error instanceof UgoiteApiError &&
    error.kind === "composition_diagnostic" &&
    typeof error.code === "string" &&
    Object.prototype.hasOwnProperty.call(
      compositionSaveDiagnosticKeys,
      error.code,
    )
  ) {
    return t(
      compositionSaveDiagnosticKeys[
        error.code as CompositionSaveDiagnosticCode
      ],
    );
  }
  return t("composition.saveFailed");
};
