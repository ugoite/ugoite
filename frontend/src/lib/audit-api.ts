import type { AuditListQuery, NodeAuditEvent, SpaceAuditPage } from "./types";
import { protocolFetch } from "./ugoite-client/protocol";

export const auditApi = {
  async listNode(): Promise<NodeAuditEvent[]> {
    return await protocolFetch<NodeAuditEvent[]>("auth.audit");
  },

  async listSpace(
    spaceId: string,
    query: AuditListQuery,
  ): Promise<SpaceAuditPage> {
    return await protocolFetch<SpaceAuditPage>("space.audit", {
      space_id: spaceId,
      offset: query.offset,
      limit: query.limit,
      action: query.action,
      actor_principal_id: query.actorId,
      outcome: query.outcome,
    });
  },
};
