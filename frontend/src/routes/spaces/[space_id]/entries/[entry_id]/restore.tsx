import { A, useNavigate, useParams } from "@solidjs/router";
import { createMemo, For, Show, createSignal } from "solid-js";
import { formatDateTimeLabel } from "~/lib/date-format";
import {
  revisionActor,
  revisionForm,
  revisionOperationLabel,
  revisionSummary,
  revisionTitle,
} from "~/lib/entry-history";
import { formatUserFacingError } from "~/lib/user-facing-error";
import { t } from "~/lib/i18n";
import { entryApi } from "~/lib/ugoite-client";
import { createResource } from "~/lib/recoverable-resource";
import { spaceRoute } from "~/lib/space-shell-route";

export const route = spaceRoute({ navigation: "forms", title: "restore" });

export default function SpaceEntryRestoreRoute() {
  const navigate = useNavigate();
  const params = useParams<{ space_id: string; entry_id: string }>();
  const spaceId = () => params.space_id;
  const entryId = () => params.entry_id;
  const encodedEntryId = () => encodeURIComponent(entryId());
  const entryPath = () =>
    `/spaces/${spaceId()}/entries/${encodedEntryId()}`;
  const [selectedRevision, setSelectedRevision] = createSignal<string | null>(
    null,
  );
  const [restoreError, setRestoreError] = createSignal<string | null>(null);
  const [isRestoring, setIsRestoring] = createSignal(false);

  const [history] = createResource(() =>
    entryApi.history(spaceId(), entryId())
  );
  const [currentEntry] = createResource(
    () => selectedRevision() ? entryId() : null,
    (selectedEntryId) => entryApi.get(spaceId(), selectedEntryId),
  );
  const [selectedContent] = createResource(
    () => selectedRevision(),
    (revisionId) => entryApi.getRevision(spaceId(), entryId(), revisionId),
  );
  const historyError = createMemo(() =>
    history.error
      ? formatUserFacingError(
        history.error,
        "entryRestore.loadError",
        "entry.history",
      )
      : null
  );
  const reviewError = createMemo(() => {
    if (currentEntry.error) {
      return formatUserFacingError(
        currentEntry.error,
        "entryRevision.currentUnavailable",
        "entry.get",
      );
    }
    if (selectedContent.error) {
      return formatUserFacingError(
        selectedContent.error,
        "entryRevision.loadError",
        "entry.revision",
      );
    }
    return null;
  });

  const handleRestore = async () => {
    const revisionId = selectedRevision();
    if (!revisionId || !selectedContent()) return;
    setIsRestoring(true);
    setRestoreError(null);
    try {
      await entryApi.restore(spaceId(), entryId(), revisionId);
      navigate(entryPath());
    } catch (error) {
      setRestoreError(
        formatUserFacingError(error, "entryRevision.restoreError", "entry.restore"),
      );
    } finally {
      setIsRestoring(false);
    }
  };

  return (
    <>
      <div class="screenHead">
        <div class="screenTitle">
          <div class="eyebrow">
            {t("entryRestore.eyebrow")} · {entryId()}
          </div>
          <h1>{t("entryRestore.title")}</h1>
        </div>
        <A href={entryPath()} class="btn">
          {t("entryRestore.backToEntry")}
        </A>
      </div>

      <Show when={history.loading}>
        <p class="ui-muted">{t("entryRestore.loading")}</p>
      </Show>
      <Show when={historyError()}>
        <p class="ui-alert ui-alert-error">{historyError()}</p>
      </Show>
      <Show when={history()}>
        {(data) => (
          <div class="settingsMain">
            <p class="ui-muted">{t("entryRestore.selectRevision")}</p>
            <Show
              when={data().revisions.length > 0}
              fallback={<p class="ui-muted">{t("entryHistory.empty")}</p>}
            >
              <ul class="rowStack">
                <For each={data().revisions}>
                  {(revision) => (
                    <li class="rowBtn">
                      <input
                        type="radio"
                        name="revision"
                        value={revision.revision_id}
                        aria-label={`${revisionOperationLabel(revision)}: ${revisionTitle(revision)}`}
                        checked={selectedRevision() === revision.revision_id}
                        onChange={() => setSelectedRevision(revision.revision_id)}
                      />
                      <span class="ui-stack-sm">
                        <span>
                          <strong>{revisionOperationLabel(revision)}</strong>
                          <span class="ui-muted"> · {revisionSummary(revision)}</span>
                        </span>
                        <span class="ui-entry-history-meta">
                          <span>
                            <b>{t("entryHistory.actor")}:</b> {revisionActor(revision)}
                          </span>
                          <span>
                            <b>{t("entryHistory.timestamp")}:</b>{" "}
                            {formatDateTimeLabel(revision.timestamp)}
                          </span>
                        </span>
                        <span class="ui-entry-history-meta">
                          <span>
                            <b>{t("common.title")}:</b> {revisionTitle(revision)}
                          </span>
                          <span>
                            <b>{t("common.form")}:</b> {revisionForm(revision)}
                          </span>
                        </span>
                        <small class="ui-muted">
                          {t("entryHistory.revisionId")}: {revision.revision_id}
                        </small>
                      </span>
                    </li>
                  )}
                </For>
              </ul>
            </Show>

            <Show when={selectedContent()}>
              {(selected) => (
                <>
                  <p class="ui-alert ui-alert-warning">
                    {t("entryRevision.restoreNotice")}
                  </p>
                  <div class="ui-entry-history-review">
                    <section class="ui-card">
                      <h2>{t("entryRevision.currentValue")}</h2>
                      <Show
                        when={currentEntry()}
                        fallback={
                          <p class="ui-muted">
                            {t("entryRevision.currentUnavailable")}
                          </p>
                        }
                      >
                        {(current) => (
                          <>
                            <dl class="ui-entry-detail-list">
                              <div>
                                <dt>{t("common.title")}</dt>
                                <dd>{current().title || t("common.untitled")}</dd>
                              </div>
                              <div>
                                <dt>{t("common.form")}</dt>
                                <dd>{current().form || t("entryHistory.unknownValue")}</dd>
                              </div>
                            </dl>
                            <h3>{t("entryRevision.source")}</h3>
                            <pre class="code">{current().content}</pre>
                          </>
                        )}
                      </Show>
                    </section>
                    <section class="ui-card">
                      <h2>{t("entryRevision.selectedValue")}</h2>
                      <dl class="ui-entry-detail-list">
                        <div>
                          <dt>{t("common.title")}</dt>
                          <dd>{revisionTitle(selected())}</dd>
                        </div>
                        <div>
                          <dt>{t("common.form")}</dt>
                          <dd>{revisionForm(selected())}</dd>
                        </div>
                        <div>
                          <dt>{t("entryRevision.revisionId")}</dt>
                          <dd>{selected().revision_id}</dd>
                        </div>
                      </dl>
                      <h3>{t("entryRevision.source")}</h3>
                      <pre class="code">{selected().markdown}</pre>
                    </section>
                  </div>
                  <Show when={reviewError()}>
                    <p class="ui-alert ui-alert-error">{reviewError()}</p>
                  </Show>
                  <button
                    type="button"
                    class="btn primary"
                    onClick={handleRestore}
                    disabled={isRestoring() || selectedContent.loading}
                  >
                    {isRestoring()
                      ? t("entryRevision.restoring")
                      : t("entryRevision.restore")}
                  </button>
                </>
              )}
            </Show>
            <Show when={restoreError()}>
              <p class="ui-alert ui-alert-error">{restoreError()}</p>
            </Show>
          </div>
        )}
      </Show>
    </>
  );
}
