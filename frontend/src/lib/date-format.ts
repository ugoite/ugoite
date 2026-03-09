const numericTimestampPattern = /^-?\d+(?:\.\d+)?$/;

const numericTimestampToDate = (value: number): Date | null => {
	if (!Number.isFinite(value)) return null;
	const millis = Math.abs(value) < 1_000_000_000_000 ? value * 1000 : value;
	const date = new Date(millis);
	return Number.isNaN(date.getTime()) ? null : date;
};

const timestampToDate = (value: string | number | null | undefined): Date | null => {
	if (typeof value === "number") return numericTimestampToDate(value);
	if (typeof value !== "string") return null;
	const trimmed = value.trim();
	if (!trimmed) return null;
	if (numericTimestampPattern.test(trimmed)) {
		return numericTimestampToDate(Number(trimmed));
	}
	const date = new Date(trimmed);
	return Number.isNaN(date.getTime()) ? null : date;
};

export const normalizeTimestamp = (value: string | number | null | undefined): string => {
	if (typeof value === "string" && value.trim() && !numericTimestampPattern.test(value.trim())) {
		return value.trim();
	}
	return timestampToDate(value)?.toISOString() ?? String(value ?? "");
};

export const formatDateLabel = (value: string | number | null | undefined): string => {
	const date = timestampToDate(value);
	if (date) return date.toLocaleDateString();
	if (typeof value === "string" && value.trim()) return value.trim();
	return "—";
};
