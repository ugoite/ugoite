import type { EntryRecord } from "~/lib/types";

/** Entry IDs are the only deterministic display identity owned by Ugoite. */
export const entryDisplayLabel = (entry: Pick<EntryRecord, "id">): string =>
  entry.id;
