import { apiFetch } from "./api";
import type { EntryRecord, SqlSession, SqlSessionRows } from "./types";

export const sqlSessionApi = {
	async create(spaceId: string, sql: string): Promise<SqlSession> {
		const res = await apiFetch(`/spaces/${encodeURIComponent(spaceId)}/sql-sessions`, {
			method: "POST",
			headers: { "Content-Type": "application/json" },
			body: JSON.stringify({ sql }),
		});
		if (!res.ok) {
			throw new Error(`Failed to create SQL session: ${res.statusText}`);
		}
		return (await res.json()) as SqlSession;
	},

	async get(spaceId: string, sessionId: string): Promise<SqlSession> {
		const res = await apiFetch(
			`/spaces/${encodeURIComponent(spaceId)}/sql-sessions/${encodeURIComponent(sessionId)}`,
		);
		if (!res.ok) {
			throw new Error(`Failed to load SQL session: ${res.statusText}`);
		}
		return (await res.json()) as SqlSession;
	},

	async count(spaceId: string, sessionId: string): Promise<number> {
		const res = await apiFetch(
			`/spaces/${encodeURIComponent(spaceId)}/sql-sessions/${encodeURIComponent(sessionId)}/count`,
		);
		if (!res.ok) {
			throw new Error(`Failed to load SQL session count: ${res.statusText}`);
		}
		const payload = (await res.json()) as { count: number };
		return payload.count;
	},

	async rows(
		spaceId: string,
		sessionId: string,
		offset: number,
		limit: number,
	): Promise<SqlSessionRows> {
		const params = new URLSearchParams({
			offset: String(offset),
			limit: String(limit),
		});
		const res = await apiFetch(
			`/spaces/${encodeURIComponent(spaceId)}/sql-sessions/${encodeURIComponent(sessionId)}/rows?${params.toString()}`,
		);
		if (!res.ok) {
			throw new Error(`Failed to load SQL session rows: ${res.statusText}`);
		}
		const payload = (await res.json()) as Record<string, unknown>;
		const rows = (payload.rows ?? []) as EntryRecord[];
		const offsetValue = Number(payload.offset ?? 0);
		const limitValue = Number(payload.limit ?? 0);
		const totalCount = Number(payload.total_count ?? payload.totalCount ?? 0);
		return {
			rows,
			offset: offsetValue,
			limit: limitValue,
			totalCount,
		};
	},
};
