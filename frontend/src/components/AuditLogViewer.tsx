import { createEffect, createMemo, createSignal, For, Show } from "solid-js";
import { authApi, spaceApi } from "~/lib/ugoite-client";
import { formatDateTimeLabel } from "~/lib/date-format";
import { t, type TranslationKey } from "~/lib/i18n";
import { formatUserFacingError } from "~/lib/user-facing-error";
import type {
  AuditFilters,
  AuditListQuery,
  AuditOutcome,
  NodeAuditEvent,
  SpaceAuditEvent,
  SpaceAuditPage,
} from "~/lib/types";

const PAGE_SIZE = 25;
const outcomes: Array<AuditOutcome | ""> = ["", "success", "deny", "error"];

type AuditEvent = NodeAuditEvent | SpaceAuditEvent;

type AuditPage = {
  items: AuditEvent[];
  total: number;
  offset: number;
  limit: number;
  bounded?: boolean;
};

type AuditQuery = {
  offset: number;
  limit: number;
  filters: AuditFilters;
};

type AuditLoader = (query: AuditQuery) => Promise<AuditPage>;

type AuditLogViewerProps = {
  source: "node" | "space";
  load: AuditLoader;
};

const isNodeEvent = (event: AuditEvent): event is NodeAuditEvent =>
  "safe_metadata" in event;

const eventActor = (event: AuditEvent): string | null =>
  isNodeEvent(event) ? event.actor_account_id : event.actor_principal_id;

const eventSubject = (event: AuditEvent): string | null =>
  isNodeEvent(event) ? event.subject_account_id : event.subject_principal_id;

const eventMetadata = (event: AuditEvent): Record<string, unknown> =>
  isNodeEvent(event) ? event.safe_metadata : event.metadata;

const eventScopeId = (event: AuditEvent): string =>
  isNodeEvent(event) ? event.node_id : event.space_id;

const eventHash = (event: AuditEvent): string | null =>
  isNodeEvent(event) ? null : event.event_hash;

const serializeMetadata = (value: Record<string, unknown>): string => {
  try {
    return JSON.stringify(value, null, 2) ?? "{}";
  } catch {
    return "{}";
  }
};

const normalizeFilterValue = (value: string): string => value.trim();

const filterNodeEvents = (
  events: NodeAuditEvent[],
  filters: AuditFilters,
): NodeAuditEvent[] => {
  const action = normalizeFilterValue(filters.action);
  const actorId = normalizeFilterValue(filters.actorId);
  return events.filter((event) =>
    (!action || event.action === action) &&
    (!actorId || event.actor_account_id === actorId) &&
    (!filters.outcome || event.outcome === filters.outcome)
  );
};

const loadNodePage: AuditLoader = async ({ offset, limit, filters }) => {
  const events = filterNodeEvents(await authApi.listAudit(), filters);
  return {
    items: events.slice(offset, offset + limit),
    total: events.length,
    offset,
    limit,
    bounded: true,
  };
};

const loadSpacePage =
  (spaceId: string): AuditLoader =>
  async ({ offset, limit, filters }): Promise<AuditPage> => {
    const query: AuditListQuery = {
      offset,
      limit,
      action: normalizeFilterValue(filters.action) || undefined,
      actorId: normalizeFilterValue(filters.actorId) || undefined,
      outcome: filters.outcome || undefined,
    };
    const page: SpaceAuditPage = await spaceApi.listAudit(spaceId, query);
    return page;
  };

export function AuditLogViewer(props: AuditLogViewerProps) {
  const [action, setAction] = createSignal("");
  const [actorId, setActorId] = createSignal("");
  const [outcome, setOutcome] = createSignal<AuditOutcome | "">("");
  const [pageIndex, setPageIndex] = createSignal(0);
  const [page, setPage] = createSignal<AuditPage>();
  const [loading, setLoading] = createSignal(true);
  const [error, setError] = createSignal<unknown>();
  let requestVersion = 0;

  const filters = createMemo<AuditFilters>(() => ({
    action: action(),
    actorId: actorId(),
    outcome: outcome(),
  }));
  const totalPages = createMemo(() =>
    Math.max(1, Math.ceil((page()?.total ?? 0) / PAGE_SIZE))
  );
  const hasFilters = createMemo(() =>
    Boolean(action() || actorId() || outcome())
  );

  const runQuery = async (query: AuditQuery) => {
    const version = ++requestVersion;
    setLoading(true);
    setError(undefined);
    try {
      const nextPage = await props.load(query);
      if (version !== requestVersion) return;
      setPage(nextPage);
    } catch (cause) {
      if (version !== requestVersion) return;
      setPage(undefined);
      setError(cause);
    } finally {
      if (version === requestVersion) setLoading(false);
    }
  };

  createEffect(() => {
    const query: AuditQuery = {
      offset: pageIndex() * PAGE_SIZE,
      limit: PAGE_SIZE,
      filters: filters(),
    };
    setPage(undefined);
    void runQuery(query);
  });

  const changeFilter = (
    setter: (value: string) => void,
    value: string,
  ) => {
    setter(value);
    setPageIndex(0);
  };

  const clearFilters = () => {
    setAction("");
    setActorId("");
    setOutcome("");
    setPageIndex(0);
  };

  const selectPage = (nextPage: number) => {
    const nextIndex = Math.max(0, Math.min(nextPage, totalPages() - 1));
    if (nextIndex === pageIndex()) return;
    setPage(undefined);
    setPageIndex(nextIndex);
  };

  const failureKey = () =>
    props.source === "node"
      ? "securityPage.auditFailedLoad"
      : "settings.failedAuditLoad";

  return (
    <div class="auditViewer">
      <Show when={props.source === "node"}>
        <p class="ui-muted">{t("securityPage.auditBoundedNotice")}</p>
      </Show>
      <div class="filterStrip" aria-label={t("auditLog.filters")}>
        <label>
          {t("auditLog.action")}
          <input
            value={action()}
            placeholder={t("auditLog.actionPlaceholder")}
            onInput={(event) =>
              changeFilter(setAction, event.currentTarget.value)}
          />
        </label>
        <label>
          {t("auditLog.actor")}
          <input
            value={actorId()}
            placeholder={t("auditLog.actorPlaceholder")}
            onInput={(event) =>
              changeFilter(setActorId, event.currentTarget.value)}
          />
        </label>
        <label>
          {t("auditLog.outcome")}
          <select
            value={outcome()}
            onChange={(event) =>
              changeFilter(
                (value) => setOutcome(value as AuditOutcome | ""),
                event.currentTarget.value,
              )}
          >
            <For each={outcomes}>
              {(value) => (
                <option value={value}>
                  {value
                    ? t(`auditLog.outcome.${value}` as TranslationKey)
                    : t("auditLog.all")}
                </option>
              )}
            </For>
          </select>
        </label>
        <Show when={hasFilters()}>
          <button
            class="btn"
            type="button"
            onClick={clearFilters}
          >
            {t("auditLog.clearFilters")}
          </button>
        </Show>
      </div>
      <Show when={loading()}>
        <p class="ui-muted" role="status">{t("auditLog.loading")}</p>
      </Show>
      <Show when={error()}>
        <p class="ui-alert ui-alert-error" role="alert">
          {formatUserFacingError(error(), failureKey())}
        </p>
      </Show>
      <Show when={!loading() && !error()}>
        <Show
          when={page()}
          fallback={<p class="ui-muted">{t("auditLog.empty")}</p>}
        >
          {(currentPage) => (
            <>
              <Show
                when={currentPage().items.length > 0}
                fallback={
                  <p class="ui-muted">
                    {t(hasFilters() ? "auditLog.noMatches" : "auditLog.empty")}
                  </p>
                }
              >
                <div class="ui-table-wrapper overflow-x-auto">
                  <table class="ui-table auditTable">
                    <thead class="ui-table-head">
                      <tr>
                        <th class="ui-table-header-cell" scope="col">
                          {t("auditLog.timestamp")}
                        </th>
                        <th class="ui-table-header-cell" scope="col">
                          {t("auditLog.action")}
                        </th>
                        <th class="ui-table-header-cell" scope="col">
                          {t("auditLog.actor")}
                        </th>
                        <th class="ui-table-header-cell" scope="col">
                          {t("auditLog.outcome")}
                        </th>
                        <th class="ui-table-header-cell" scope="col">
                          {t("auditLog.target")}
                        </th>
                        <th class="ui-table-header-cell" scope="col">
                          {t("auditLog.details")}
                        </th>
                      </tr>
                    </thead>
                    <tbody class="ui-table-body">
                      <For each={currentPage().items}>
                        {(event) => (
                          <tr class="ui-table-row">
                            <td class="ui-table-cell">
                              {formatDateTimeLabel(event.timestamp)}
                            </td>
                            <td class="ui-table-cell">
                              <code>{event.action}</code>
                            </td>
                            <td class="ui-table-cell">
                              {eventActor(event) ?? "—"}
                            </td>
                            <td class="ui-table-cell">
                              <span
                                class={`auditOutcome auditOutcome-${event.outcome}`}
                              >
                                {outcomes.includes(
                                    event.outcome as AuditOutcome,
                                  )
                                  ? t(
                                    `auditLog.outcome.${event.outcome}` as TranslationKey,
                                  )
                                  : event.outcome}
                              </span>
                            </td>
                            <td class="ui-table-cell">
                              {event.target_type ?? "—"}
                              <Show when={event.target_id}>
                                {(targetId) => (
                                  <>
                                    <br />
                                    <code>{targetId()}</code>
                                  </>
                                )}
                              </Show>
                            </td>
                            <td class="ui-table-cell">
                              <details>
                                <summary>{t("auditLog.viewDetails")}</summary>
                                <dl class="auditDetails">
                                  <div>
                                    <dt>{t("auditLog.eventId")}</dt>
                                    <dd>
                                      <code>{event.event_id}</code>
                                    </dd>
                                  </div>
                                  <div>
                                    <dt>{t("auditLog.subject")}</dt>
                                    <dd>{eventSubject(event) ?? "—"}</dd>
                                  </div>
                                  <div>
                                    <dt>{t("auditLog.scope")}</dt>
                                    <dd>
                                      <code>{eventScopeId(event)}</code>
                                    </dd>
                                  </div>
                                  <div>
                                    <dt>{t("auditLog.credentialId")}</dt>
                                    <dd>{event.credential_id ?? "—"}</dd>
                                  </div>
                                  <div>
                                    <dt>{t("auditLog.requestId")}</dt>
                                    <dd>{event.request_id ?? "—"}</dd>
                                  </div>
                                  <div>
                                    <dt>{t("auditLog.metadata")}</dt>
                                    <dd>
                                      <code>
                                        {serializeMetadata(
                                          eventMetadata(event),
                                        )}
                                      </code>
                                    </dd>
                                  </div>
                                  <Show when={eventHash(event)}>
                                    {(hash) => (
                                      <div>
                                        <dt>{t("auditLog.eventHash")}</dt>
                                        <dd>
                                          <code>{hash()}</code>
                                        </dd>
                                      </div>
                                    )}
                                  </Show>
                                </dl>
                              </details>
                            </td>
                          </tr>
                        )}
                      </For>
                    </tbody>
                  </table>
                </div>
              </Show>
              <Show when={currentPage().total > 0}>
                <div
                  class="auditPagination"
                  aria-label={t("auditLog.pagination")}
                >
                  <button
                    class="btn"
                    type="button"
                    disabled={pageIndex() === 0 || loading()}
                    onClick={() => selectPage(pageIndex() - 1)}
                  >
                    {t("auditLog.previous")}
                  </button>
                  <span class="ui-muted">
                    {t("auditLog.page", {
                      current: pageIndex() + 1,
                      total: totalPages(),
                      count: currentPage().total,
                    })}
                  </span>
                  <button
                    class="btn"
                    type="button"
                    disabled={pageIndex() + 1 >= totalPages() || loading()}
                    onClick={() => selectPage(pageIndex() + 1)}
                  >
                    {t("auditLog.next")}
                  </button>
                </div>
              </Show>
            </>
          )}
        </Show>
      </Show>
    </div>
  );
}

export function NodeAuditLogViewer() {
  return <AuditLogViewer source="node" load={loadNodePage} />;
}

export function SpaceAuditLogViewer(props: { spaceId: string }) {
  const load = createMemo(() => loadSpacePage(props.spaceId));
  return <AuditLogViewer source="space" load={load()} />;
}
