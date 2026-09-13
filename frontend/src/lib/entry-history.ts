import { t, type TranslationKey } from "./i18n";

export type RevisionMetadata = {
  operation?: string;
  entry_version?: number;
  restored_from?: string | null;
  actor?: string;
  updated_by?: string;
  author?: string;
  title?: string;
  form?: string;
};

const operationLabels: Record<string, TranslationKey> = {
  upsert: "entryHistory.operation.updated",
  delete: "entryHistory.operation.deleted",
  restore: "entryHistory.operation.restored",
};

const operationSummaryLabels: Record<string, TranslationKey> = {
  delete: "entryHistory.summary.deleted",
  restore: "entryHistory.summary.restored",
};

export const revisionActor = (revision: RevisionMetadata): string =>
  revision.actor?.trim() || revision.updated_by?.trim() ||
  revision.author?.trim() || t("entryHistory.unknownActor");

export const revisionOperationLabel = (revision: RevisionMetadata): string => {
  const operation = revision.operation?.trim().toLowerCase() ?? "";
  if (operation === "upsert") {
    return t(
      revision.entry_version === 1
        ? "entryHistory.operation.created"
        : "entryHistory.operation.updated",
    );
  }
  return t(operationLabels[operation] ?? "entryHistory.operation.unknown");
};

export const revisionSummary = (revision: RevisionMetadata): string => {
  const operation = revision.operation?.trim().toLowerCase() ?? "";
  const summaryKey = operationSummaryLabels[operation];
  if (summaryKey) {
    return t(
      summaryKey,
      revision.restored_from ? { value: revision.restored_from } : undefined,
    );
  }
  if (operation === "upsert") {
    return t(
      revision.entry_version === 1
        ? "entryHistory.summary.created"
        : "entryHistory.summary.updated",
    );
  }
  return t("entryHistory.summary.unknown");
};

export const revisionTitle = (revision: RevisionMetadata): string =>
  revision.title?.trim() || t("common.untitled");

export const revisionForm = (revision: RevisionMetadata): string =>
  revision.form?.trim() || t("entryHistory.unknownValue");
