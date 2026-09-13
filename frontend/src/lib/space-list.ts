import type { Space } from "./types";

export const DEFAULT_SPACE_SLUG = "default";

/**
 * Return the immutable remote identity used in protocol requests and URLs.
 * `id` is retained only as a compatibility fallback for older fixtures and
 * responses; new server responses always provide `space_uid`.
 */
export function spaceUid(space: Pick<Space, "id" | "space_uid">): string {
  return space.space_uid?.trim() || space.id;
}

function isDefaultSpace(space: Space): boolean {
  return space.slug === DEFAULT_SPACE_SLUG ||
    (!space.slug && space.id === DEFAULT_SPACE_SLUG);
}

function compareSpaces(a: Space, b: Space): number {
  const priority = (space: Space): number => {
    if (isDefaultSpace(space)) return 0;
    return 1;
  };
  const priorityDiff = priority(a) - priority(b);
  if (priorityDiff !== 0) {
    return priorityDiff;
  }
  const aLabel = (a.name || a.slug || spaceUid(a)).toLocaleLowerCase();
  const bLabel = (b.name || b.slug || spaceUid(b)).toLocaleLowerCase();
  return aLabel.localeCompare(bLabel);
}

export function sortSpaces(spaces: readonly Space[]): Space[] {
  return [...spaces].sort(compareSpaces);
}
