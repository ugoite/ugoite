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

/** Minimal directory entry for actor display-name resolution. */
export type ActorDirectoryEntry = {
  principal_id: string;
  display_name: string;
};

const UUID_PATTERN =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const SHORT_ACTOR_MAX_LENGTH = 16;

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

/**
 * Raw actor identity for advanced/detail disclosure. History rows never show
 * this: they show `resolveActorDisplayName`. The revision detail and Info
 * routes own the copyable raw value (POL-UI-005 advanced-only identifiers).
 */
export const revisionActorId = (
  revision: RevisionMetadata,
): string | null => {
  const raw = revision.actor?.trim() || revision.updated_by?.trim() ||
    revision.author?.trim() || "";
  return raw ? raw : null;
};

/**
 * Stable short fallback for rows when an actor ID cannot be resolved to a
 * display name. UUIDs collapse to their first 8 characters; short
 * human-meaningful values pass through unchanged; anything else truncates
 * deterministically. Empty stays the shared unknown-actor copy.
 */
export const shortActorFallback = (actorId: string): string => {
  const raw = actorId.trim();
  if (!raw) return t("entryHistory.unknownActor");
  if (UUID_PATTERN.test(raw)) return raw.slice(0, 8);
  return raw.length > SHORT_ACTOR_MAX_LENGTH
    ? `${raw.slice(0, SHORT_ACTOR_MAX_LENGTH - 1)}…`
    : raw;
};

/**
 * Actor display name for history rows. The history API carries only opaque
 * actor identity strings (actor/updated_by/author principal IDs); display
 * names resolve through the Space member directory when available. Rows show
 * the display name, the stable short fallback, or the unknown-actor copy —
 * never a raw UUID (UX-HIST actor UUID=0 in normal rows).
 */
export const resolveActorDisplayName = (
  revision: RevisionMetadata,
  lookup?: (actorId: string) => string | undefined,
): string => {
  const raw = revisionActorId(revision);
  if (!raw) return t("entryHistory.unknownActor");
  return lookup?.(raw)?.trim() || shortActorFallback(raw);
};

/** Build the member-directory lookup for `resolveActorDisplayName`. */
export const actorDisplayNameLookup = (
  members: ActorDirectoryEntry[],
): ((actorId: string) => string | undefined) => {
  const names = new Map<string, string>();
  for (const member of members) {
    const id = member.principal_id?.trim();
    const name = member.display_name?.trim();
    if (id && name && !names.has(id)) names.set(id, name);
  }
  return (actorId: string) => names.get(actorId.trim());
};

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
