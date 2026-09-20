import type { Space } from "./types";

export const DEFAULT_SPACE_SLUG = "default";

/** Return the immutable remote identity used in protocol requests and URLs. */
export function spaceUid(space: Pick<Space, "space_uid">): string {
  return space.space_uid;
}

function isDefaultSpace(space: Space): boolean {
  return space.slug === DEFAULT_SPACE_SLUG;
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
