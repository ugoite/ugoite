import type { Space } from "./types";

export const DEFAULT_SPACE_ID = "default";
const RESERVED_ADMIN_SPACE_ID = "admin-space";

export function isReservedAdminSpace(space: Pick<Space, "id" | "is_admin_space">): boolean {
	return space.is_admin_space === true || space.id === RESERVED_ADMIN_SPACE_ID;
}

function compareSpaces(a: Space, b: Space): number {
	const priority = (space: Space): number => {
		if (space.id === DEFAULT_SPACE_ID) return 0;
		if (isReservedAdminSpace(space)) return 2;
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

export function partitionSpaces(spaces: readonly Space[]): {
	userSpaces: Space[];
	adminSpaces: Space[];
} {
	const sortedSpaces = sortSpaces(spaces);
	const userSpaces = sortedSpaces.filter((space) => !isReservedAdminSpace(space));
	const adminSpaces = sortedSpaces.filter((space) => isReservedAdminSpace(space));
	return { userSpaces, adminSpaces };
}
