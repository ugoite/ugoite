import { useParams } from "@solidjs/router";
import { createMemo, createSignal, For, Show } from "solid-js";
import { ConfirmDestructiveAction } from "~/components/ConfirmDestructiveAction";
import { LocalBusyIndicator } from "~/components/LocalBusyIndicator";
import { UiIcon } from "~/components/UiIcon";
import { formatDateTimeLabel } from "~/lib/date-format";
import {
  actorDisplayNameLookup,
  shortActorFallback,
} from "~/lib/entry-history";
import { changeApi, spaceApi, type SpaceChange } from "~/lib/ugoite-client";
import type { SpaceMember } from "~/lib/types";
import { createResource } from "~/lib/recoverable-resource";
import { t, type TranslationKey } from "~/lib/i18n";
import { formatUserFacingError } from "~/lib/user-facing-error";
import { spaceRoute } from "~/lib/space-shell-route";

export const route = spaceRoute({
  navigation: "history",
});

const changeKind = (change: SpaceChange): string =>
  change.reverts_change_id
    ? t("spaceHistory.revert")
    : t("spaceHistory.change");

type PendingRecovery =
  | { kind: "revert"; change: SpaceChange }
  | { kind: "undo"; change: SpaceChange };

const isDefiniteRevisionConflict = (error: unknown): boolean => {
  if (!error || typeof error !== "object") return false;
  const diagnostic = error as { code?: unknown; status?: unknown };
  return diagnostic.code === "REVISION_CONFLICT" && diagnostic.status === 409;
};

const isDefiniteApiRejection = (error: unknown): boolean => {
  if (!error || typeof error !== "object") return false;
  const diagnostic = error as { kind?: unknown; status?: unknown };
  return typeof diagnostic.kind === "string" &&
    typeof diagnostic.status === "number" &&
    diagnostic.status >= 400 && diagnostic.status < 500 &&
    diagnostic.status !== 408 && diagnostic.status !== 425;
};

export default function SpaceHistoryRoute() {
  const params = useParams<{ space_id: string }>();
  const spaceId = () => params.space_id;
  const [history, { refetch }] = createResource(() =>
    changeApi.list(spaceId())
  );
  // Best-effort member directory for actor display names. The Change API
  // carries only opaque actor principal IDs; when the directory is
  // unavailable (or the actor left), rows fall back to the stable short
  // form — never a raw UUID.
  const [members] = createResource(
    () => spaceId(),
    async (id): Promise<SpaceMember[]> => {
      try {
        return await spaceApi.listMembers(id);
      } catch {
        return [];
      }
    },
    { initialValue: [] as SpaceMember[] },
  );
  const actorLookup = createMemo(() =>
    actorDisplayNameLookup(
      (members() ?? []).map((member) => ({
        principal_id: member.principal.principal_id,
        display_name: member.principal.display_name,
      })),
    )
  );
  const actorName = (actorId: string): string => {
    const raw = actorId.trim();
    if (!raw) return t("entryHistory.unknownActor");
    return actorLookup()?.(raw)?.trim() || shortActorFallback(raw);
  };
  const [pending, setPending] = createSignal<PendingRecovery | null>(null);
  const [message, setMessage] = createSignal("");
  const [working, setWorking] = createSignal(false);
  const [notice, setNotice] = createSignal<string | null>(null);
  const [failure, setFailure] = createSignal<string | null>(null);
  const [refreshWarning, setRefreshWarning] = createSignal<string | null>(null);
  const [refreshWarningKey, setRefreshWarningKey] = createSignal<
    TranslationKey | null
  >(null);
  const [historyNeedsReview, setHistoryNeedsReview] = createSignal(false);

  const pendingOpen = () => pending() !== null;

  const cancelConfirm = () => {
    if (working()) return;
    setPending(null);
    setMessage("");
  };

  // A server-confirmed Change closes the dialog even though the request is
  // still marked busy while the timeline is refreshed.
  const finishConfirm = () => {
    setPending(null);
    setMessage("");
  };

  const refreshHistory = async (warningKey: TranslationKey) => {
    setHistoryNeedsReview(true);
    try {
      await refetch();
      if (history.state === "errored") {
        setRefreshWarning(t(warningKey));
        setRefreshWarningKey(warningKey);
      } else {
        setRefreshWarning(null);
        setRefreshWarningKey(null);
        setHistoryNeedsReview(false);
      }
    } catch {
      setRefreshWarning(t(warningKey));
      setRefreshWarningKey(warningKey);
    }
  };

  const confirmRecovery = async () => {
    const current = pending();
    if (!current || working()) return;
    setWorking(true);
    setNotice(null);
    setFailure(null);
    setRefreshWarning(null);
    try {
      // Revert and undo append new Changes; past states are never rewritten.
      // The UI only reports success after the server-confirmed result.
      if (current.kind === "revert") {
        const result = await changeApi.revert(
          spaceId(),
          current.change.change_id,
          message().trim() ? { message: message().trim() } : {},
        );
        setNotice(
          t("spaceHistory.revertSuccess", { value: result.change_id }),
        );
      } else {
        const runId = current.change.run_id ?? "";
        const result = await changeApi.undoRun(spaceId(), runId);
        setNotice(
          t("spaceHistory.undoSuccess", {
            count: result.reverted_change_count,
          }),
        );
      }
      finishConfirm();
      await refreshHistory("spaceHistory.refreshFailedAfterSave");
    } catch (err) {
      finishConfirm();
      if (isDefiniteRevisionConflict(err)) {
        setFailure(t("spaceHistory.conflict"));
        await refreshHistory("spaceHistory.refreshFailedAfterConflict");
      } else if (isDefiniteApiRejection(err)) {
        setFailure(
          formatUserFacingError(err, "spaceHistory.operationFailed"),
        );
      } else {
        // A lost response can happen after the server committed. Re-read the
        // timeline before the user decides whether another operation is safe.
        setFailure(t("spaceHistory.operationResultUnknown"));
        await refreshHistory("spaceHistory.refreshFailedAfterUnknownResult");
      }
    } finally {
      setWorking(false);
    }
  };

  return (
    <>
      <h1 class="ui-sr-only">{t("spaceHistory.title")}</h1>
      {/* Panel-local spinner: existing rows stay mounted during refetch. */}
      <Show when={history.loading}>
        <LocalBusyIndicator label={t("spaceHistory.loading")} />
      </Show>
      <Show when={history.error && !refreshWarning()}>
        <p class="ui-alert ui-alert-error">{t("spaceHistory.loadError")}</p>
      </Show>
      <Show when={notice()}>
        <p class="ui-alert ui-alert-success">{notice()}</p>
      </Show>
      <Show when={failure()}>
        <p class="ui-alert ui-alert-error">{failure()}</p>
      </Show>
      <Show when={refreshWarning()}>
        <div class="ui-alert ui-alert-warning">
          <p>{refreshWarning()}</p>
          <button
            type="button"
            class="ui-button ui-button-secondary mt-2 text-sm"
            disabled={history.loading}
            onClick={() => {
              const key = refreshWarningKey();
              if (key) void refreshHistory(key);
            }}
          >
            {t("spaceHistory.retryHistoryRefresh")}
          </button>
        </div>
      </Show>
      <Show when={history()}>
        {(data) => (
          <Show
            when={data().length > 0}
            fallback={<p class="ui-muted">{t("spaceHistory.empty")}</p>}
          >
            <div class="ui-table-wrapper overflow-x-auto">
              <table
                class="ui-table historyTable"
                aria-busy={history.loading || undefined}
              >
                <thead class="ui-table-head">
                  <tr>
                    <th class="ui-table-header-cell" scope="col">
                      {t("spaceHistory.change")}
                    </th>
                    <th class="ui-table-header-cell" scope="col">
                      {t("spaceHistory.actor")}
                    </th>
                    <th class="ui-table-header-cell" scope="col">
                      {t("auditLog.timestamp")}
                    </th>
                    <th class="ui-table-header-cell" scope="col">
                      {t("auditLog.details")}
                    </th>
                  </tr>
                </thead>
                <tbody class="ui-table-body">
                  <For each={data()}>
                    {(change) => (
                      <tr class="ui-table-row">
                        <td class="ui-table-cell">
                          <span class="historyRowIcon glyph active">
                            <UiIcon name="history" />
                          </span>
                          <b>{changeKind(change)}</b>
                          <Show when={change.message}>
                            <span>{change.message}</span>
                          </Show>
                          <span class="mt-2 flex flex-wrap gap-2">
                            <button
                              type="button"
                              class="ui-button ui-button-secondary text-sm"
                              disabled={working() || historyNeedsReview()}
                              onClick={() => {
                                setNotice(null);
                                setFailure(null);
                                setRefreshWarning(null);
                                setPending({ kind: "revert", change });
                              }}
                            >
                              {t("spaceHistory.revertAction")}
                            </button>
                            <Show when={change.run_id}>
                              <button
                                type="button"
                                class="ui-button ui-button-secondary text-sm"
                                disabled={working() || historyNeedsReview()}
                                onClick={() => {
                                  setNotice(null);
                                  setFailure(null);
                                  setRefreshWarning(null);
                                  setPending({ kind: "undo", change });
                                }}
                              >
                                {t("spaceHistory.undoRunAction")}
                              </button>
                            </Show>
                          </span>
                        </td>
                        <td class="ui-table-cell">
                          {actorName(change.actor_principal_id)}
                        </td>
                        <td class="ui-table-cell">
                          {formatDateTimeLabel(change.created_at_micros / 1000)}
                        </td>
                        <td class="ui-table-cell">
                          <details>
                            <summary>{t("auditLog.viewDetails")}</summary>
                            <dl class="auditDetails">
                              <div>
                                <dt>{t("spaceHistory.change")}</dt>
                                <dd>
                                  <code>{change.change_id}</code>
                                </dd>
                              </div>
                              <Show when={change.run_id}>
                                <div>
                                  <dt>Run</dt>
                                  <dd>
                                    <code>{change.run_id}</code>
                                  </dd>
                                </div>
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
        )}
      </Show>
      {
        /*
        Revert/undo confirmation (shared destructive-action dialog): the
        append-only notice is explicit before the operation, with the
        optional revert message as dialog content.
      */
      }
      <ConfirmDestructiveAction
        open={pendingOpen()}
        title={pending()?.kind === "undo"
          ? t("spaceHistory.undoRunAction")
          : t("spaceHistory.revertAction")}
        body={t("spaceHistory.appendOnlyNotice")}
        confirmLabel={t("spaceHistory.confirmAppend")}
        busy={working()}
        onConfirm={() => void confirmRecovery()}
        onClose={cancelConfirm}
      >
        <Show when={pending()?.kind === "revert" && pending()?.change}>
          {(change) => (
            <div class="ui-stack-sm">
              <label
                class="ui-label"
                for={`revert-message-${change().change_id}`}
              >
                {t("spaceHistory.messageLabel")}
              </label>
              <input
                id={`revert-message-${change().change_id}`}
                type="text"
                class="ui-input mt-2 w-full"
                value={message()}
                disabled={working()}
                onInput={(event) => setMessage(event.currentTarget.value)}
              />
            </div>
          )}
        </Show>
      </ConfirmDestructiveAction>
    </>
  );
}
