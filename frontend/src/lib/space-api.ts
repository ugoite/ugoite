import type {
	Space,
	SpaceMember,
	SpaceMemberAcceptPayload,
	SpaceMemberInvitePayload,
	SpaceMemberInviteResponse,
	SpaceMemberRoleUpdatePayload,
	SpacePatchPayload,
	TestConnectionPayload,
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

	/** List members in a space */
	async listMembers(id: string): Promise<SpaceMember[]> {
		const res = await apiFetch(`/spaces/${id}/members`);
		if (!res.ok) {
			throw new Error(await formatApiError(res, `Failed to list members: ${res.statusText}`));
		}
		return (await res.json()) as SpaceMember[];
	},

	/** Invite a member to a space */
	async inviteMember(
		id: string,
		payload: SpaceMemberInvitePayload,
	): Promise<SpaceMemberInviteResponse> {
		const res = await apiFetch(`/spaces/${id}/members/invitations`, {
			method: "POST",
			headers: { "Content-Type": "application/json" },
			body: JSON.stringify(payload),
		});
		if (!res.ok) {
			throw new Error(await formatApiError(res, `Failed to invite member: ${res.statusText}`));
		}
		return (await res.json()) as SpaceMemberInviteResponse;
	},

	/** Accept invitation token */
	async acceptInvitation(
		id: string,
		payload: SpaceMemberAcceptPayload,
	): Promise<{ member: SpaceMember }> {
		const res = await apiFetch(`/spaces/${id}/members/accept`, {
			method: "POST",
			headers: { "Content-Type": "application/json" },
			body: JSON.stringify(payload),
		});
		if (!res.ok) {
			throw new Error(await formatApiError(res, `Failed to accept invitation: ${res.statusText}`));
		}
		return (await res.json()) as { member: SpaceMember };
	},

	/** Update role for a member */
	async updateMemberRole(
		id: string,
		memberUserId: string,
		payload: SpaceMemberRoleUpdatePayload,
	): Promise<{ member: SpaceMember }> {
		const res = await apiFetch(`/spaces/${id}/members/${memberUserId}/role`, {
			method: "POST",
			headers: { "Content-Type": "application/json" },
			body: JSON.stringify(payload),
		});
		if (!res.ok) {
			throw new Error(await formatApiError(res, `Failed to update role: ${res.statusText}`));
		}
		return (await res.json()) as { member: SpaceMember };
	},

	/** Revoke a member */
	async revokeMember(id: string, memberUserId: string): Promise<{ member: SpaceMember }> {
		const res = await apiFetch(`/spaces/${id}/members/${memberUserId}`, {
			method: "DELETE",
		});
		if (!res.ok) {
			throw new Error(await formatApiError(res, `Failed to revoke member: ${res.statusText}`));
		}
		return (await res.json()) as { member: SpaceMember };
	},
};
