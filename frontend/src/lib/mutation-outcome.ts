/** Shared result vocabulary for operations that can change durable state. */
export type MutationOutcome =
  | "confirmed"
  | "rejected"
  | "unknown"
  | "receipt_invalid";

export const MUTATION_ERROR_CODES = {
  rejected: "MUTATION_REJECTED",
  unknown: "MUTATION_OUTCOME_UNKNOWN",
  receipt_invalid: "MUTATION_RECEIPT_INVALID",
} as const;

const MUTATION_OPERATIONS = new Set([
  "space.create",
  "change.revert",
  "run.undo",
  "ugoite.apply",
  "pin.create",
  "pin.delete",
  "space.patch",
  "space.members.invite",
  "space.members.update_role",
  "space.members.revoke",
  "space.recovery.force_reset",
  "form.upsert",
  "entry.create",
  "entry.update",
  "entry.delete",
  "entry.restore",
  "sql.create",
  "sql.update",
  "sql.delete",
  "composition.save",
  "composition.restore",
  "agent.create",
  "agent.revoke",
  "approval.issue",
  "access.put",
  "asset.upload",
  "asset.delete",
]);

type ErrorEvidence = {
  operation?: unknown;
  status?: unknown;
  kind?: unknown;
  code?: unknown;
};

/** Classify only write failures; reads and preflight operations return null. */
export const classifyMutationOutcome = (
  error: ErrorEvidence,
): MutationOutcome | null => {
  if (
    typeof error.operation !== "string" ||
    !MUTATION_OPERATIONS.has(error.operation)
  ) return null;

  if (
    error.kind === "invalid_response" && typeof error.status === "number" &&
    error.status >= 200 && error.status < 300
  ) return "receipt_invalid";

  if (
    error.kind === "invalid_arguments" || error.kind === "invalid_operation" ||
    error.kind === "invalid_input"
  ) return "rejected";

  if (
    typeof error.status === "number" && error.status >= 400 &&
    error.status < 500 && error.status !== 408 && error.status !== 425
  ) {
    return "rejected";
  }

  // A typed validation error is produced before a write can be accepted.
  if (
    typeof error.code === "string" && [
      "INVALID_IDENTIFIER",
      "INVALID_INPUT",
      "FORM_VALIDATION_FAILED",
      "UNKNOWN_FORM_FIELDS",
    ].includes(error.code)
  ) return "rejected";

  return "unknown";
};
