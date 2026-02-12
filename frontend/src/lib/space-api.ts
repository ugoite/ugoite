import type {
	SampleSpaceCreatePayload,
	SampleSpaceJob,
	SampleSpaceScenario,
	SampleSpaceSummary,
	TestConnectionPayload,
	Space,
	SpacePatchPayload,
} from "./types";
import { apiFetch } from "./api";

const parseErrorDetail = (detail: unknown): string => {
	if (typeof detail === "string" && detail.trim()) return detail;
	if (Array.isArray(detail)) {
		const messages = detail
			.map((item) => {
				if (typeof item === "string") return item;
				if (item && typeof item === "object") {
					const maybeMsg = (item as { msg?: string }).msg;
					if (typeof maybeMsg === "string") return maybeMsg;
					return JSON.stringify(item);
				}
				return "";
			})
			.filter(Boolean);
		return messages.join("\n");
	}
	if (detail && typeof detail === "object") return JSON.stringify(detail);
	return "";
};

const formatApiError = async (res: Response, fallback: string): Promise<string> => {
	try {
		const payload = (await res.json()) as { detail?: unknown };
		const message = parseErrorDetail(payload?.detail);
		return message || fallback;
	} catch {
		return fallback;
	}
};

/**
 * Space API client
 */
export const spaceApi = {
	/** List all spaces */
	async list(): Promise<Space[]> {
		const res = await apiFetch("/spaces");
		if (!res.ok) {
			throw new Error(`Failed to list spaces: ${res.statusText}`);
		}
		return (await res.json()) as Space[];
	},

	/** Create a new space */
	async create(name: string): Promise<{ id: string; name: string }> {
		const res = await apiFetch("/spaces", {
			method: "POST",
			headers: { "Content-Type": "application/json" },
			body: JSON.stringify({ name }),
		});
		if (!res.ok) {
			throw new Error(await formatApiError(res, `Failed to create space: ${res.statusText}`));
		}
		return (await res.json()) as { id: string; name: string };
	},

	/** Create a sample-data space */
	async createSampleSpace(payload: SampleSpaceCreatePayload): Promise<SampleSpaceSummary> {
		const res = await apiFetch("/spaces/sample-data", {
			method: "POST",
			headers: { "Content-Type": "application/json" },
			body: JSON.stringify(payload),
		});
		if (!res.ok) {
			throw new Error(
				await formatApiError(res, `Failed to create sample space: ${res.statusText}`),
			);
		}
		return (await res.json()) as SampleSpaceSummary;
	},

	/** List sample-data scenarios */
	async listSampleScenarios(): Promise<SampleSpaceScenario[]> {
		const res = await apiFetch("/spaces/sample-data/scenarios");
		if (!res.ok) {
			throw new Error(
				await formatApiError(res, `Failed to list sample scenarios: ${res.statusText}`),
			);
		}
		return (await res.json()) as SampleSpaceScenario[];
	},

	/** Create a sample-data generation job */
	async createSampleSpaceJob(payload: SampleSpaceCreatePayload): Promise<SampleSpaceJob> {
		const res = await apiFetch("/spaces/sample-data/jobs", {
			method: "POST",
			headers: { "Content-Type": "application/json" },
			body: JSON.stringify(payload),
		});
		if (!res.ok) {
			throw new Error(await formatApiError(res, `Failed to create sample job: ${res.statusText}`));
		}
		return (await res.json()) as SampleSpaceJob;
	},

	/** Get a sample-data generation job */
	async getSampleSpaceJob(jobId: string): Promise<SampleSpaceJob> {
		const res = await apiFetch(`/spaces/sample-data/jobs/${jobId}`);
		if (!res.ok) {
			throw new Error(await formatApiError(res, `Failed to get sample job: ${res.statusText}`));
		}
		return (await res.json()) as SampleSpaceJob;
	},

	/** Get space by ID */
	async get(id: string): Promise<Space> {
		const res = await apiFetch(`/spaces/${id}`);
		if (!res.ok) {
			throw new Error(`Failed to get space: ${res.statusText}`);
		}
		return (await res.json()) as Space;
	},

	/** Patch space metadata/settings */
	async patch(id: string, payload: SpacePatchPayload): Promise<Space> {
		const res = await apiFetch(`/spaces/${id}`, {
			method: "PATCH",
			headers: { "Content-Type": "application/json" },
			body: JSON.stringify(payload),
		});
		if (!res.ok) {
			throw new Error(await formatApiError(res, `Failed to patch space: ${res.statusText}`));
		}
		return (await res.json()) as Space;
	},

	/** Test storage connection */
	async testConnection(id: string, payload: TestConnectionPayload): Promise<{ status: string }> {
		const res = await apiFetch(`/spaces/${id}/test-connection`, {
			method: "POST",
			headers: { "Content-Type": "application/json" },
			body: JSON.stringify(payload),
		});
		if (!res.ok) {
			throw new Error(await formatApiError(res, `Failed to test connection: ${res.statusText}`));
		}
		return (await res.json()) as { status: string };
	},
};
