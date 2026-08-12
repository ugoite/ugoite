import { apiFetch } from "./api";
import { UgoiteApiError } from "./ugoite-client/protocol";
import type { AuditListQuery, NodeAuditEvent, SpaceAuditPage } from "./types";

const responsePayload = async (response: Response): Promise<unknown> => {
  const body = await response.text();
  if (!body) return {};
  try {
    return JSON.parse(body) as unknown;
  } catch {
    return body;
  }
};

const errorMessage = (payload: unknown, status: number): string => {
  if (typeof payload === "string" && payload.trim()) return payload.trim();
  if (!payload || typeof payload !== "object") {
    return `Audit request failed (${status})`;
  }
  const body = payload as Record<string, unknown>;
  if (typeof body.message === "string") return body.message;
  if (typeof body.detail === "string") return body.detail;
  return `Audit request failed (${status})`;
};

const errorKind = (payload: unknown, status: number): string => {
  if (payload && typeof payload === "object") {
    const kind = (payload as Record<string, unknown>).kind;
    if (typeof kind === "string") return kind;
  }
  if (status === 401 || status === 403) return "forbidden";
  if (status === 404) return "not_found";
  if (status === 409) return "conflict";
  if (status === 410) return "expired";
  if (status === 422) return "invalid_arguments";
  if (status === 501) return "unimplemented";
  if (status === 502 || status === 503) return "dependency_unavailable";
  return "internal";
};

const request = async <T>(
  path: string,
  query: Record<string, string | number | undefined> = {},
): Promise<T> => {
  const params = new URLSearchParams();
  for (const [key, value] of Object.entries(query)) {
    if (value !== undefined && value !== "") params.set(key, String(value));
  }
  const queryString = params.toString();
  const response = await apiFetch(
    queryString ? `${path}?${queryString}` : path,
    { method: "GET" },
  );
  const payload = await responsePayload(response);
  if (!response.ok) {
    throw new UgoiteApiError({
      kind: errorKind(payload, response.status),
      message: errorMessage(payload, response.status),
      code: typeof payload === "object" && payload &&
          typeof (payload as Record<string, unknown>).code === "string"
        ? (payload as Record<string, string>).code
        : undefined,
      operation: path.includes("/spaces/") ? "space.audit" : "auth.audit",
      status: response.status,
      detail: typeof payload === "object" && payload
        ? (payload as Record<string, unknown>).detail
        : payload,
      payload,
    });
  }
  return payload as T;
};

export const auditApi = {
  async listNode(): Promise<NodeAuditEvent[]> {
    return await request<NodeAuditEvent[]>("/auth/audit");
  },

  async listSpace(
    spaceId: string,
    query: AuditListQuery,
  ): Promise<SpaceAuditPage> {
    return await request<SpaceAuditPage>(
      `/spaces/${encodeURIComponent(spaceId)}/audit`,
      {
        offset: query.offset,
        limit: query.limit,
        action: query.action,
        actor_principal_id: query.actorId,
        outcome: query.outcome,
      },
    );
  },
};
