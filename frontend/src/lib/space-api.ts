import type {
  AgentCreatePayload,
  AgentPrincipal,
  Space,
  SpaceMember,
  SpaceMemberInvitePayload,
  SpaceMemberInviteResponse,
  SpaceMemberRoleUpdatePayload,
  SpacePatchPayload,
  TestConnectionPayload,
} from "./types";
import { auditApi } from "./audit-api";
import type { AuditListQuery, SpaceAuditPage } from "./types";
import { protocolFetch } from "./ugoite-client/protocol";

/** Space API client backed by the shared Rust/WASM protocol. */
export const spaceApi = {
  async list(): Promise<Space[]> {
    return await protocolFetch<Space[]>("space.list");
  },

  async create(name: string): Promise<{ id: string; name: string }> {
    return await protocolFetch<{ id: string; name: string }>(
      "space.create",
      {},
      { name },
    );
  },

  async get(id: string): Promise<Space> {
    return await protocolFetch<Space>("space.get", { space_id: id });
  },

  async listAudit(
    id: string,
    query: AuditListQuery,
  ): Promise<SpaceAuditPage> {
    return await auditApi.listSpace(id, query);
  },

  async createPin(
    id: string,
    name: string,
  ): Promise<{
    coordinate: {
      generation: number;
      publication_uri: {
        space_uid: string;
        key: string;
      };
      publication_checksum: string;
    };
    created_at_micros: number;
    created_by_principal_id: string;
  }> {
    return await protocolFetch("pin.create", { space_id: id }, {
      name,
    });
  },

  async diffPins(
    id: string,
    from: string,
    to: string,
  ): Promise<Record<string, unknown>> {
    return await protocolFetch("space.pin_diff", {
      space_id: id,
      from,
      to,
    });
  },

  async patch(id: string, payload: SpacePatchPayload): Promise<Space> {
    return await protocolFetch<Space>("space.patch", { space_id: id }, payload);
  },

  async testConnection(
    id: string,
    payload: TestConnectionPayload,
  ): Promise<{ status: string }> {
    return await protocolFetch<{ status: string }>(
      "space.test_connection",
      { space_id: id },
      payload,
    );
  },

  async listMembers(id: string): Promise<SpaceMember[]> {
    return await protocolFetch<SpaceMember[]>("space.members.list", {
      space_id: id,
    });
  },

  async inviteMember(
    id: string,
    payload: SpaceMemberInvitePayload,
  ): Promise<SpaceMemberInviteResponse> {
    return await protocolFetch<SpaceMemberInviteResponse>(
      "space.members.invite",
      { space_id: id },
      payload,
    );
  },

  async updateMemberRole(
    id: string,
    principalId: string,
    payload: SpaceMemberRoleUpdatePayload,
  ): Promise<{ principal_id: string; role: SpaceMember["role"] }> {
    return await protocolFetch<
      { principal_id: string; role: SpaceMember["role"] }
    >(
      "space.members.update_role",
      { space_id: id, principal_id: principalId },
      payload,
    );
  },

  async revokeMember(
    id: string,
    principalId: string,
  ): Promise<{ principal_id: string; state: "revoked" }> {
    return await protocolFetch<{ principal_id: string; state: "revoked" }>(
      "space.members.revoke",
      {
        space_id: id,
        principal_id: principalId,
      },
    );
  },

  async listAgents(id: string): Promise<AgentPrincipal[]> {
    return await protocolFetch<AgentPrincipal[]>("agent.list", {
      space_id: id,
    });
  },

  async createAgent(
    id: string,
    payload: AgentCreatePayload,
  ): Promise<{ agent: AgentPrincipal; credential: Record<string, unknown> }> {
    return await protocolFetch("agent.create", { space_id: id }, payload);
  },

  async revokeAgent(id: string, agentId: string): Promise<void> {
    await protocolFetch("agent.revoke", {
      space_id: id,
      agent_id: agentId,
    });
  },
};
