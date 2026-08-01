import { t, type TranslationKey } from "./i18n";
import {
  UgoiteApiError,
  type UgoiteApiOperation,
} from "./ugoite-client/protocol";

const CODE_KEYS: Record<string, TranslationKey> = {
  INVALID_IDENTIFIER: "errors.code.invalidIdentifier",
  FORBIDDEN: "errors.code.forbidden",
  SPACE_ALREADY_EXISTS: "errors.code.spaceAlreadyExists",
  SPACE_NOT_FOUND: "errors.code.spaceNotFound",
  FORM_NOT_FOUND: "errors.code.formNotFound",
  ENTRY_NOT_FOUND: "errors.code.entryNotFound",
  REVISION_NOT_FOUND: "errors.code.revisionNotFound",
  REVISION_CONFLICT: "errors.code.revisionConflict",
  FORM_VALIDATION_FAILED: "errors.code.formValidationFailed",
  UNKNOWN_FORM_FIELDS: "errors.code.unknownFormFields",
  ASSET_NOT_FOUND: "errors.code.assetNotFound",
  INVITATION_EXPIRED: "errors.code.invitationExpired",
  INVITATION_NOT_FOUND: "errors.code.invitationNotFound",
  INVITATION_NOT_PENDING: "errors.code.invitationNotPending",
  MEMBER_ALREADY_ACTIVE: "errors.code.memberAlreadyActive",
  MEMBER_NOT_FOUND: "errors.code.memberNotFound",
  LAST_ADMIN_REQUIRED: "errors.code.lastAdminRequired",
  ASSET_REFERENCED: "errors.code.assetReferenced",
  SQL_SESSION_EXPIRED: "errors.code.sqlSessionExpired",
  REINDEX_NOT_IMPLEMENTED: "errors.code.reindexNotImplemented",
  STORAGE_CONNECTION_FAILED: "errors.code.storageConnectionFailed",
  INVALID_INPUT: "errors.code.invalidInput",
  INTERNAL_ERROR: "errors.code.internal",
  ORIGIN_MISMATCH: "errors.code.originMismatch",
  NODE_UNINITIALIZED: "errors.code.nodeUninitialized",
  RECENT_PASSKEY_REQUIRED: "errors.code.recentPasskeyRequired",
  INVALID_TOTP: "errors.code.invalidTotp",
  TOTP_ENROLLMENT_FAILED: "errors.code.totpEnrollmentFailed",
  AUTHENTICATION_FAILED: "errors.code.authenticationFailed",
  PASSKEY_CANCELLED: "securityPage.passkeyCancelled",
};

const OPERATION_KEYS: Partial<
  Record<UgoiteApiOperation | string, TranslationKey>
> = {
  "search.keyword": "errors.operation.search",
  "search.query": "errors.operation.search",
  "space.list": "errors.operation.settings",
  "sql.list": "errors.operation.savedSql",
  "sql.get": "errors.operation.savedSql",
  "sql.create": "errors.operation.savedSql",
  "sql.update": "errors.operation.savedSql",
  "sql.delete": "errors.operation.savedSql",
  "sql_session.create": "errors.operation.querySession",
  "sql_session.get": "errors.operation.querySession",
  "sql_session.count": "errors.operation.querySession",
  "sql_session.rows": "errors.operation.querySession",
  "space.get": "errors.operation.settings",
  "space.patch": "errors.operation.settings",
  "space.test_connection": "errors.operation.settings",
  "space.members.list": "errors.operation.settings",
  "space.members.invite": "errors.operation.settings",
  "space.members.update_role": "errors.operation.settings",
  "space.members.revoke": "errors.operation.settings",
  "agent.list": "errors.operation.settings",
  "agent.create": "errors.operation.settings",
  "agent.revoke": "errors.operation.settings",
  "access.get": "errors.operation.accessPolicy",
  "access.put": "errors.operation.accessPolicy",
};

const KIND_KEYS: Record<string, TranslationKey> = {
  forbidden: "errors.forbidden",
  not_found: "errors.notFound",
  invalid_arguments: "errors.invalidInput",
  invalid_response: "errors.invalidResponse",
  conflict: "errors.conflict",
  expired: "errors.expired",
  dependency_unavailable: "errors.unavailable",
  unimplemented: "errors.unimplemented",
  internal: "errors.internal",
};

const STATUS_KEYS: Record<number, TranslationKey> = {
  400: "errors.invalidInput",
  401: "errors.forbidden",
  403: "errors.forbidden",
  404: "errors.notFound",
  409: "errors.conflict",
  410: "errors.expired",
  422: "errors.invalidInput",
  500: "errors.internal",
  501: "errors.unimplemented",
  502: "errors.unavailable",
  503: "errors.unavailable",
};

const detailText = (value: unknown): string | null => {
  if (typeof value === "string" && value.trim()) return value.trim();
  if (!value || typeof value !== "object") return null;
  const object = value as Record<string, unknown>;
  if (typeof object.message === "string" && object.message.trim()) {
    return object.message.trim();
  }
  try {
    const serialized = JSON.stringify(value);
    return serialized && serialized !== "{}" ? serialized : null;
  } catch {
    return null;
  }
};

const apiDetails = (error: UgoiteApiError): string | null =>
  detailText(error.detail) ?? detailText(error.payload) ??
    detailText(error.message);

/** Convert transport/domain errors into a localized summary plus optional detail. */
export const formatUserFacingError = (
  error: unknown,
  fallbackKey: TranslationKey,
  operation?: string,
): string => {
  const apiError = error instanceof UgoiteApiError ? error : null;
  const stringOperationKey = typeof error === "string" && operation
    ? OPERATION_KEYS[operation]
    : undefined;
  const summaryKey = (apiError?.code && CODE_KEYS[apiError.code]) ??
    (apiError?.operation && OPERATION_KEYS[apiError.operation]) ??
    stringOperationKey ??
    (apiError?.kind && KIND_KEYS[apiError.kind]) ??
    (apiError?.status && STATUS_KEYS[apiError.status]);
  const summary = t(summaryKey ?? fallbackKey);
  const detail = apiError ? apiDetails(apiError) : detailText(error);

  if (!detail || detail === summary || detail === t(fallbackKey)) {
    return summary;
  }
  return t("errors.withDetail", { summary, detail });
};
