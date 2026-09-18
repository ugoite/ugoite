import type { EntryRecord } from "~/lib/types";

/**
 * Human label for an Entry row. Title-less Entry: the stable entry_id is
 * the canonical identity; a legacy compatibility title is display-only and
 * never synthesized (no "Untitled" canonical label).
 */
export const entryDisplayLabel = (
  entry: Pick<EntryRecord, "id" | "title">,
): string => entry.title?.trim() || entry.id;
