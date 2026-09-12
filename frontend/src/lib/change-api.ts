import { protocolFetch } from "./ugoite-client/protocol";

/** One Space-level append-only Change: a durable Knowledge mutation. */
export type SpaceChange = {
  change_id: string;
  generation: number;
  actor_principal_id: string;
  message: string | null;
  reverts_change_id: string | null;
  run_id: string | null;
  created_at_micros: number;
};

const asRecord = (value: unknown): Record<string, unknown> =>
  typeof value === "object" && value !== null
    ? (value as Record<string, unknown>)
    : {};

const asString = (value: unknown): string | null =>
  typeof value === "string" ? value : null;

/**
 * Typed client for Space-level Change history.
 *
 * Change history is durable Knowledge mutation evidence. It is distinct
 * from the Audit log, which is security/operational evidence.
 */
export const changeApi = {
  async list(spaceId: string): Promise<SpaceChange[]> {
    const changes = await protocolFetch<unknown[]>("change.list", {
      space_id: spaceId,
    });
    return changes.map((item) => {
      const row = asRecord(item);
      const change = asRecord(row.change);
      return {
        change_id: asString(row.change_id) ?? "",
        generation: typeof row.generation === "number" ? row.generation : 0,
        actor_principal_id: asString(change.actor_principal_id) ?? "",
        message: asString(change.message),
        reverts_change_id: asString(change.reverts_change_id),
        run_id: asString(change.run_id),
        created_at_micros: typeof change.created_at_micros === "number"
          ? change.created_at_micros
          : 0,
      };
    });
  },
};
