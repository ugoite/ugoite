/**
 * Central Space URL-segment encoding helper (PR-07, #2849).
 *
 * All `/spaces/:space_id/...` navigation targets must route the Space ID
 * through {@link encodeSpaceSegment} so identifiers containing `/`, spaces,
 * Unicode, or `%` survive as a single path segment. API calls keep using the
 * logical (decoded) Space ID; only the URL segment is encoded.
 *
 * Existing URLs/APIs/CLI are unchanged: for plain IDs
 * `encodeSpaceSegment("default") === "default"`, so deep links keep working.
 */

/** Encode a logical Space ID as a single URL path segment. */
export function encodeSpaceSegment(spaceId: string): string {
  return encodeURIComponent(spaceId);
}

/**
 * Decode a `:space_id` route param back to the logical Space ID.
 * Solid's router typically decodes params already; this stays idempotent:
 * plain IDs pass through, single-encoded segments decode once, and malformed
 * `%` sequences return as-is instead of throwing.
 */
export function decodeSpaceSegment(segment: string | undefined | null): string {
  if (segment == null) return "";
  const value = String(segment);
  if (!value.includes("%")) return value;
  try {
    return decodeURIComponent(value);
  } catch {
    return value;
  }
}

/** `/spaces/:space_id` base for a logical Space ID. */
export function spaceBase(spaceId: string): string {
  return `/spaces/${encodeSpaceSegment(spaceId)}`;
}

/**
 * Join a subpath onto a Space base without double slashes.
 * `sub` may include query/hash (e.g. `entries?form=X`); only the Space
 * segment is encoded here, query values stay the caller's responsibility.
 */
export function spacePath(spaceId: string, sub: string = ""): string {
  const base = spaceBase(spaceId);
  if (!sub) return base;
  return `${base}/${sub.replace(/^\//, "")}`;
}

export const spaceDashboardPath = (spaceId: string) =>
  spacePath(spaceId, "dashboard");
export const spaceEntriesPath = (spaceId: string, query = "") =>
  spacePath(spaceId, query ? `entries${query}` : "entries");
export const spaceEntryPath = (spaceId: string, entryId: string) =>
  `${spaceBase(spaceId)}/entries/${encodeURIComponent(entryId)}`;
export const spaceFormsPath = (spaceId: string, query = "") =>
  spacePath(spaceId, query ? `forms${query}` : "forms");
export const spaceAssetsPath = (spaceId: string) =>
  spacePath(spaceId, "assets");
export const spaceSearchPath = (spaceId: string) =>
  spacePath(spaceId, "search");
export const spaceHistoryPath = (spaceId: string) =>
  spacePath(spaceId, "history");
export const spaceSettingsPath = (spaceId: string, query = "") =>
  spacePath(spaceId, query ? `settings${query}` : "settings");
export const spaceSqlPath = (spaceId: string) => spacePath(spaceId, "sql");
