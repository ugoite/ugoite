import type { CompositionDraft } from "./composition-draft";

/**
 * Studio workspace arrangement. Design edits the finished look, Data
 * confirms and edits what is fetched, and Split pairs the Design canvas
 * with the Data pane for the selected block. Split is a comprehension aid:
 * the mode and pane selection are transient Work, never persisted.
 */
export type StudioMode = "design" | "data" | "split";

export const STUDIO_MODES: readonly StudioMode[] = [
  "design",
  "data",
  "split",
];

const PARAMETER_BLOCK_PREFIX = "param:";

/**
 * Block to source mapping for Design to Data synchronization. Metric and
 * table blocks resolve to their component source; text blocks and parameter
 * controls have no source, so the caller keeps the current Data selection.
 */
export const sourceDraftIdForBlock = (
  draft: CompositionDraft,
  blockId: string | null,
): string | null => {
  if (!blockId || blockId.startsWith(PARAMETER_BLOCK_PREFIX)) return null;
  const display = draft.displays.find((entry) => entry.draftId === blockId);
  if (!display || display.kind === "text") return null;
  return display.sourceDraftId;
};

/**
 * Source to using-block mapping for Data to Design soft highlight. Returns
 * component block identities only; highlight never changes the selection.
 */
export const blockIdsUsingSource = (
  draft: CompositionDraft,
  sourceDraftId: string | null,
): string[] => {
  if (!sourceDraftId) return [];
  return draft.displays
    .filter((display) =>
      "sourceDraftId" in display && display.sourceDraftId === sourceDraftId
    )
    .map((display) => display.draftId);
};

/**
 * Visible-component sources for the bounded preview discipline: draft
 * source IDs referenced by placed non-text components, in layout order.
 * Text blocks emit no source request; unplaced declarations are excluded.
 */
export const visibleComponentSourceIds = (
  draft: CompositionDraft,
): string[] => {
  const placed = new Set<string>();
  for (const row of draft.layoutRows) {
    for (const item of row.items) {
      if (item.kind === "component") placed.add(item.draftId);
    }
  }
  const seen = new Set<string>();
  const sources: string[] = [];
  for (const display of draft.displays) {
    if (display.kind === "text" || !placed.has(display.draftId)) continue;
    if (!seen.has(display.sourceDraftId)) {
      seen.add(display.sourceDraftId);
      sources.push(display.sourceDraftId);
    }
  }
  return sources;
};
