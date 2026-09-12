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

/** Result of appending a revert Change. The reverted Change is kept. */
export type RevertResult = {
  change_id: string;
  reverts_change_id: string;
  run_id: string | null;
};

/** Result of appending Run undo inverses. The Run itself is untouched. */
export type UndoResult = {
  run_id: string;
  reverted_change_count: number;
};

/**
 * Typed client for Space-level Change history and append-only recovery.
 *
 * Change history is durable Knowledge mutation evidence. It is distinct
 * from the Audit log, which is security/operational evidence.
 *
 * Revert and undo never rewrite the past: each returns a server-confirmed
 * result describing the newly appended Change(s).
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

  async revert(
    spaceId: string,
    changeId: string,
    options: { runId?: string; message?: string } = {},
  ): Promise<RevertResult> {
    const body: Record<string, string> = {};
    if (options.runId) body.run_id = options.runId;
    if (options.message) body.message = options.message;
    const result = await protocolFetch<Record<string, unknown>>(
      "change.revert",
      { space_id: spaceId, change_id: changeId },
      body,
    );
    return {
      change_id: asString(result.change_id) ?? "",
      reverts_change_id: asString(result.reverts_change_id) ?? changeId,
      run_id: asString(result.run_id),
    };
  },

  async undoRun(spaceId: string, runId: string): Promise<UndoResult> {
    const result = await protocolFetch<Record<string, unknown>>("run.undo", {
      space_id: spaceId,
      run_id: runId,
    });
    const count = result.reverted_change_count;
    return {
      run_id: asString(result.run_id) ?? runId,
      reverted_change_count: typeof count === "number" ? count : 0,
    };
  },
};
