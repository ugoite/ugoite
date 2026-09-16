import { A, useNavigate, useParams } from "@solidjs/router";
import { createMemo, Show, createSignal } from "solid-js";
import { formatDateTimeLabel } from "~/lib/date-format";
import {
  revisionActor,
  revisionForm,
  revisionOperationLabel,
  revisionTitle,
} from "~/lib/entry-history";
import { formatUserFacingError } from "~/lib/user-facing-error";
import { t } from "~/lib/i18n";
import { entryApi } from "~/lib/ugoite-client";
import { createResource } from "~/lib/recoverable-resource";
import { spaceRoute } from "~/lib/space-shell-route";

export const route = spaceRoute({ navigation: "forms", title: "revision" });

export default function SpaceEntryRevisionRoute() {
  const navigate = useNavigate();
  const params = useParams<
    { space_id: string; entry_id: string; revision_id: string }
  >();
  const spaceId = () => params.space_id;
  const entryId = () => params.entry_id;
  const revisionId = () => params.revision_id;
  const entryPath = () =>
    `/spaces/${spaceId()}/entries/${encodeURIComponent(entryId())}`;

  const [currentEntry] = createResource(() =>
    entryApi.get(spaceId(), entryId())
  );
  const [revision] = createResource(() =>
    entryApi.getRevision(spaceId(), entryId(), revisionId())
  );
  const [restoreError, setRestoreError] = createSignal<string | null>(null);
  const [isRestoring, setIsRestoring] = createSignal(false);
  const reviewError = createMemo(() =>
    revision.error
      ? formatUserFacingError(
        revision.error,
        "entryRevision.loadError",
        "entry.revision",
      )
      : null
  );

  const handleRestore = async () => {
    if (!revision()) return;
    setIsRestoring(true);
    setRestoreError(null);
    try {
      // Restore is an append-only mutation. The response carries the newly
      // current revision; navigating to the Entry route reopens that state.
      await entryApi.restore(spaceId(), entryId(), revisionId());
      navigate(entryPath());
    } catch (error) {
      setRestoreError(
        formatUserFacingError(
          error,
          "entryRevision.restoreError",
          "entry.restore",
        ),
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
            {t("entryRevision.eyebrow")} · {entryId()}
          </div>
          <h1>{t("entryRevision.title")}</h1>
        </div>
        <A href={`${entryPath()}/history`} class="btn">
          {t("entryRevision.backToHistory")}
        </A>
      </div>

      <Show when={revision.loading}>
        <p class="ui-muted">{t("entryRevision.loading")}</p>
      </Show>
      <Show when={reviewError()}>
        <p class="ui-alert ui-alert-error">{reviewError()}</p>
      </Show>
      <Show when={revision()}>
        {(selected) => (
          <div class="settingsMain">
            <p class="ui-alert ui-alert-warning">
              {t("entryRevision.restoreNotice")}
            </p>
            <div class="ui-entry-history-review">
              <section
                class="ui-entry-history-section"
                aria-label={t("entryRevision.currentValue")}
              >
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

              <hr class="ui-entry-history-divider" aria-hidden="true" />

              <section
                class="ui-entry-history-section"
                aria-label={t("entryRevision.selectedValue")}
              >
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
                    <dt>{t("entryRevision.operation")}</dt>
                    <dd>{revisionOperationLabel(selected())}</dd>
                  </div>
                  <div>
                    <dt>{t("entryRevision.actor")}</dt>
                    <dd>{revisionActor(selected())}</dd>
                  </div>
                  <div>
                    <dt>{t("entryRevision.timestamp")}</dt>
                    <dd>{formatDateTimeLabel(selected().timestamp)}</dd>
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

            <button
              type="button"
              class="btn primary ui-entry-history-restore"
              onClick={handleRestore}
              disabled={isRestoring()}
            >
              {isRestoring()
                ? t("entryRevision.restoring")
                : t("entryRevision.restore")}
            </button>
            <Show when={restoreError()}>
              <p class="ui-alert ui-alert-error">{restoreError()}</p>
            </Show>
          </div>
        )}
      </Show>
    </>
  );
}
