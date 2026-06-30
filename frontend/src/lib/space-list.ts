import type { Space } from "./types";

export const DEFAULT_SPACE_ID = "default";

function compareSpaces(a: Space, b: Space): number {
  const priority = (space: Space): number => {
    if (space.id === DEFAULT_SPACE_ID) return 0;
    return 1;
  };
  const priorityDiff = priority(a) - priority(b);
  if (priorityDiff !== 0) {
    return priorityDiff;
  }
  const aLabel = (a.name || a.id).toLocaleLowerCase();
  const bLabel = (b.name || b.id).toLocaleLowerCase();
  return aLabel.localeCompare(bLabel);
}

export function sortSpaces(spaces: readonly Space[]): Space[] {
  return [...spaces].sort(compareSpaces);
}
