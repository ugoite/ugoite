// REQ-FE-054: Timestamp normalization in query and entry lists
import { describe, expect, it } from "vitest";
import { formatDateLabel, normalizeTimestamp } from "./date-format";

describe("date-format", () => {
	it("REQ-FE-054: normalizeTimestamp converts unix-second timestamps from numbers and numeric strings", () => {
		const expected = new Date(1772960822.056 * 1000).toISOString();

		expect(normalizeTimestamp(1772960822.056)).toBe(expected);
		expect(normalizeTimestamp("1772960822.056")).toBe(expected);
	});

	it("REQ-FE-054: normalizeTimestamp handles millisecond timestamps and trimmed string inputs", () => {
		const expected = new Date(1772960822056).toISOString();

		expect(normalizeTimestamp(1772960822056)).toBe(expected);
		expect(normalizeTimestamp(` ${expected} `)).toBe(expected);
		expect(normalizeTimestamp(" not-a-timestamp ")).toBe("not-a-timestamp");
		expect(normalizeTimestamp("   ")).toBe("   ");
	});

	it("REQ-FE-054: normalizeTimestamp falls back to string output for missing or invalid numeric inputs", () => {
		expect(normalizeTimestamp(undefined)).toBe("");
		expect(normalizeTimestamp(Number.POSITIVE_INFINITY)).toBe("Infinity");
		expect(normalizeTimestamp(Number.MAX_SAFE_INTEGER)).toBe(String(Number.MAX_SAFE_INTEGER));
	});

	it("REQ-FE-054: formatDateLabel renders valid timestamps as locale dates", () => {
		const isoTimestamp = new Date(1772960822.056 * 1000).toISOString();
		const expected = new Date(isoTimestamp).toLocaleDateString();

		expect(formatDateLabel(isoTimestamp)).toBe(expected);
		expect(formatDateLabel(1772960822.056)).toBe(expected);
	});

	it("REQ-FE-054: formatDateLabel falls back to trimmed text or em dash", () => {
		expect(formatDateLabel(" not-a-date ")).toBe("not-a-date");
		expect(formatDateLabel("   ")).toBe("—");
		expect(formatDateLabel(null)).toBe("—");
	});
});
