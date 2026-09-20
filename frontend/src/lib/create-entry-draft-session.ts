import type { StructuredEntryFields } from "~/lib/types";

/** The in-memory work state for one new Entry authoring session. */
export interface CreateEntryDraftState {
  title: string;
  fields: StructuredEntryFields;
  tags: string[];
  /** Values used by provisional asset fields, including uploaded references. */
  assetFields: StructuredEntryFields;
  dirty: boolean;
}

const clone = <T>(value: T): T => {
  if (typeof structuredClone === "function") return structuredClone(value);
  return JSON.parse(JSON.stringify(value)) as T;
};

/**
 * Work is intentionally kept outside a route component. Route components are
 * disposable UI; this object keeps a new Entry's uncommitted work alive until
 * the author explicitly discards it or it becomes a durable Entry.
 */
export class CreateEntryDraftSession {
  private readonly drafts = new Map<string, CreateEntryDraftState>();

  save(formName: string, state: CreateEntryDraftState): void {
    const key = formName.trim();
    if (!key) return;
    this.drafts.set(key, clone(state));
  }

  restore(formName: string): CreateEntryDraftState | undefined {
    const state = this.drafts.get(formName.trim());
    return state ? clone(state) : undefined;
  }

  hasDirtyWork(): boolean {
    return [...this.drafts.values()].some((draft) => draft.dirty);
  }

  clear(): void {
    this.drafts.clear();
  }
}

const sessions = new Map<string, CreateEntryDraftSession>();

export const createEntryDraftSessionKey = (spaceId: string) =>
  `create-entry:${spaceId}`;

export function getCreateEntryDraftSession(
  key: string,
): CreateEntryDraftSession {
  let session = sessions.get(key);
  if (!session) {
    session = new CreateEntryDraftSession();
    sessions.set(key, session);
  }
  return session;
}

export function clearCreateEntryDraftSession(key: string): void {
  sessions.get(key)?.clear();
  sessions.delete(key);
}
