import { A, useParams } from "@solidjs/router";
import { createSignal, For, Show } from "solid-js";
import { UiIcon } from "~/components/UiIcon";
import { formatDateTimeLabel } from "~/lib/date-format";
import { changeApi, type SpaceChange } from "~/lib/ugoite-client";
import { createResource } from "~/lib/recoverable-resource";
import { t } from "~/lib/i18n";
import { formatUserFacingError } from "~/lib/user-facing-error";
import { spaceRoute } from "~/lib/space-shell-route";

export const route = spaceRoute({ navigation: "home", title: "spaceHistory" });

const changeKind = (change: SpaceChange): string =>
  change.reverts_change_id
    ? t("spaceHistory.revert")
    : t("spaceHistory.change");

type PendingRecovery =
  | { kind: "revert"; change: SpaceChange }
  | { kind: "undo"; change: SpaceChange };

export default function SpaceHistoryRoute() {
  const params = useParams<{ space_id: string }>();
  const spaceId = () => params.space_id;
  const [history, { refetch }] = createResource(() =>
    changeApi.list(spaceId())
  );
  const [pending, setPending] = createSignal<PendingRecovery | null>(null);
  const [message, setMessage] = createSignal("");
  const [working, setWorking] = createSignal(false);
  const [notice, setNotice] = createSignal<string | null>(null);
  const [failure, setFailure] = createSignal<string | null>(null);

  const closeConfirm = () => {
    setPending(null);
    setMessage("");
  };

  const confirmRecovery = async () => {
    const current = pending();
    if (!current || working()) return;
    setWorking(true);
    setNotice(null);
    setFailure(null);
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
      closeConfirm();
      await refetch();
    } catch (err) {
      setFailure(
        formatUserFacingError(err, "spaceHistory.operationFailed"),
      );
    } finally {
      setWorking(false);
    }
  };

  return (
    <>
      <div class="screenHead">
        <div class="screenTitle">
          <div class="eyebrow">{spaceId()}</div>
          <h1>{t("spaceHistory.title")}</h1>
        </div>
        <A href={`/spaces/${spaceId()}/dashboard`} class="btn">
          {t("spaceHistory.backToSpace")}
        </A>
      </div>
      <p class="ui-muted">{t("spaceHistory.description")}</p>
      <Show when={history.loading}>
        <p class="ui-muted">{t("spaceHistory.loading")}</p>
      </Show>
      <Show when={history.error}>
        <p class="ui-alert ui-alert-error">{t("spaceHistory.loadError")}</p>
      </Show>
      <Show when={notice()}>
        <p class="ui-alert ui-alert-success">{notice()}</p>
      </Show>
      <Show when={failure()}>
        <p class="ui-alert ui-alert-error">{failure()}</p>
      </Show>
      <Show when={history()}>
        {(data) => (
          <Show
            when={data().length > 0}
            fallback={<p class="ui-muted">{t("spaceHistory.empty")}</p>}
          >
            <div class="rowStack">
              <For each={data()}>
                {(change) => (
                  <div class="ui-card">
                    <span class="glyph active">
                      <UiIcon name="history" />
                    </span>
                    <span>
                      <b>{changeKind(change)}</b>
                      <Show when={change.message}>
                        <span>{change.message}</span>
                      </Show>
                      <small>
                        {formatDateTimeLabel(change.created_at_micros / 1000)}
                      </small>
                      <small>{change.actor_principal_id}</small>
                      <small>
                        {t("spaceHistory.changeId", {
                          value: change.change_id,
                        })}
                      </small>
                      <Show when={change.run_id}>
                        <small>
                          {t("spaceHistory.runId", {
                            value: change.run_id ?? "",
                          })}
                        </small>
                      </Show>
                      <span class="mt-2 flex flex-wrap gap-2">
                        <button
                          type="button"
                          class="ui-button ui-button-secondary text-sm"
                          disabled={working()}
                          onClick={() => {
                            setNotice(null);
                            setFailure(null);
                            setPending({ kind: "revert", change });
                          }}
                        >
                          {t("spaceHistory.revertAction")}
                        </button>
                        <Show when={change.run_id}>
                          <button
                            type="button"
                            class="ui-button ui-button-secondary text-sm"
                            disabled={working()}
                            onClick={() => {
                              setNotice(null);
                              setFailure(null);
                              setPending({ kind: "undo", change });
                            }}
                          >
                            {t("spaceHistory.undoRunAction")}
                          </button>
                        </Show>
                      </span>
                      <Show
                        when={pending()?.change.change_id === change.change_id}
                      >
                        <span class="mt-2 ui-stack-sm">
                          <small>{t("spaceHistory.appendOnlyNotice")}</small>
                          <Show when={pending()?.kind === "revert"}>
                            <label
                              class="ui-label"
                              for={`revert-message-${change.change_id}`}
                            >
                              {t("spaceHistory.messageLabel")}
                            </label>
                            <input
                              id={`revert-message-${change.change_id}`}
                              type="text"
                              class="ui-input mt-2 w-full"
                              value={message()}
                              onInput={(event) =>
                                setMessage(event.currentTarget.value)}
                            />
                          </Show>
                          <span class="mt-2 flex flex-wrap gap-2">
                            <button
                              type="button"
                              class="ui-button ui-button-primary text-sm"
                              disabled={working()}
                              onClick={() => void confirmRecovery()}
                            >
                              {working()
                                ? t("searchPage.running")
                                : t("spaceHistory.confirmAppend")}
                            </button>
                            <button
                              type="button"
                              class="ui-button ui-button-secondary text-sm"
                              disabled={working()}
                              onClick={closeConfirm}
                            >
                              {t("common.cancel")}
                            </button>
                          </span>
                        </span>
                      </Show>
                    </span>
                  </div>
                )}
              </For>
            </div>
          </Show>
        )}
      </Show>
    </>
  );
}
