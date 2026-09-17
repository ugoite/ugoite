import { A, useNavigate, useParams } from "@solidjs/router";
import { createMemo, createSignal, Show } from "solid-js";
import { ButtonSpinner } from "~/components/ButtonSpinner";
import { createEntryFieldInputId, EntryFields } from "~/components/EntryFields";
import { formatDateTimeLabel } from "~/lib/date-format";
import { LocalBusyIndicator } from "~/components/LocalBusyIndicator";
import { parseEntryMarkdownPresentation } from "~/lib/entry-input";
import { formatUserFacingError } from "~/lib/user-facing-error";
import { t } from "~/lib/i18n";
import { entryApi } from "~/lib/ugoite-client";
import { createResource } from "~/lib/recoverable-resource";
import { spaceRoute } from "~/lib/space-shell-route";

export const route = spaceRoute({ navigation: "forms", title: "revision" });

/** Revision subtitle: shared locale-aware date plus a localized marker. */
export function formatRevisionSubtitle(
  value: string | number | null | undefined,
): string {
  return t("entryRevision.subtitle", {
    date: formatDateTimeLabel(value),
    marker: t("entryRevision.readOnly"),
  });
}

export default function SpaceEntryRevisionRoute() {
  const navigate = useNavigate();
  const params = useParams<
    { space_id: string; entry_id: string; revision_id: string }
  >();
  const spaceId = () => params.space_id;
  const entryId = () => params.entry_id;
  const revisionId = () => params.revision_id;
  const entryPath = () =>
    `/spaces/${encodeURIComponent(spaceId())}/entries/${
      encodeURIComponent(entryId())
    }`;

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

  // The stored Markdown is the revision content authority; the shared
  // EntryFields renderer shows it read-only (every control disabled).
  const parsedRevision = createMemo(() => {
    const markdown = revision()?.markdown ?? "";
    try {
      return parseEntryMarkdownPresentation(markdown);
    } catch {
      return { title: "", fields: {} as Record<string, string> };
    }
  });
  const revisionTitleValue = createMemo(() =>
    parsedRevision().title || revision()?.title || ""
  );
  const revisionFields = createMemo(() =>
    Object.keys(parsedRevision().fields).map((name, index) => ({
      name,
      fieldId: createEntryFieldInputId(name, index),
    }))
  );

  const handleRestore = async () => {
    if (!revision() || isRestoring()) return;
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
          <Show when={revision()}>
            {(selected) => (
              <p class="ui-page-subtitle revision-subtitle">
                {formatRevisionSubtitle(selected().timestamp)}
              </p>
            )}
          </Show>
        </div>
        <A href={`${entryPath()}/history`} class="btn">
          {t("entryRevision.backToHistory")}
        </A>
      </div>

      {/* Panel-local spinner: rendered content stays mounted on refetch. */}
      <Show when={revision.loading}>
        <LocalBusyIndicator label={t("entryRevision.loading")} />
      </Show>
      <Show when={reviewError()}>
        <p class="ui-alert ui-alert-error">{reviewError()}</p>
      </Show>
      <Show when={revision()}>
        <div class="settingsMain" aria-busy={revision.loading || undefined}>
          <p class="ui-alert ui-alert-warning">
            {t("entryRevision.restoreNotice")}
          </p>
          <EntryFields
            titleValue={revisionTitleValue()}
            fields={revisionFields()}
            getValue={(name) => parsedRevision().fields[name] ?? ""}
            readOnly
          />

          <div class="revision-restore-row">
            <button
              type="button"
              class="btn primary ui-entry-history-restore"
              aria-label={t("entryRevision.restore")}
              aria-busy={isRestoring() || undefined}
              onClick={() => void handleRestore()}
              disabled={isRestoring()}
            >
              <Show when={isRestoring()}>
                <ButtonSpinner />
              </Show>
              {t("entryRevision.restoreShort")}
            </button>
          </div>
          <Show when={restoreError()}>
            <p class="ui-alert ui-alert-error">{restoreError()}</p>
          </Show>
        </div>
      </Show>
    </>
  );
}
