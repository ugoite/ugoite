import { useParams } from "@solidjs/router";
import { createSignal, onMount, Show } from "solid-js";
import { BackLink } from "~/components/BackLink";
import { CompositionStudio } from "~/components/CompositionStudio";
import { LocalBusyIndicator } from "~/components/LocalBusyIndicator";
import {
  compositionApi,
  type CompositionLintDocument,
} from "~/lib/composition-api";
import {
  type CompositionDraft,
  type CompositionStudioRevisionDocument,
  draftFromDocument,
} from "~/lib/composition-draft";
import { t } from "~/lib/i18n";
import { spaceCompositionRevisionPath } from "~/lib/space-path";
import { spaceRoute } from "~/lib/space-shell-route";
import { entryApi, sqlApi } from "~/lib/ugoite-client";

export const route = spaceRoute({ navigation: "compositions" });

/**
 * Display-only lookup for the referenced Saved SQL name. Inline SQL names
 * are already stored on their Composition source.
 */
const resolveSourceNames = async (
  spaceId: string,
  document: CompositionLintDocument,
): Promise<Record<string, string>> => {
  const names: Record<string, string> = {};
  for (const [index, source] of document.spec.sources.entries()) {
    names[source.id] = `${t("composition.studioSource")} ${index + 1}`;
    if (source.kind === "saved_sql" && source.entry_id) {
      try {
        const entry = await sqlApi.get(spaceId, source.entry_id);
        if (entry.name) names[source.id] = entry.name;
      } catch {
        // Keep the ordered human label when current source metadata is unavailable.
      }
    } else if (source.kind === "inline_sql" && source.name) {
      names[source.id] = source.name;
    }
  }
  return names;
};

/** Read SQL text only from the immutable Saved SQL revision pinned in spec. */
const resolveSavedSqlBodies = async (
  spaceId: string,
  document: CompositionLintDocument,
): Promise<Record<string, string>> => {
  const savedSources = document.spec.sources.filter((source) =>
    source.kind === "saved_sql"
  );
  const bodies = await Promise.all(savedSources.map(async (source) => {
    const revision = await entryApi.getRevision(
      spaceId,
      source.entry_id,
      source.revision_id,
    );
    const sql = revision.fields?.sql;
    if (typeof sql !== "string") {
      throw new Error("Saved SQL revision does not contain SQL text");
    }
    return [source.id, sql] as const;
  }));
  return Object.fromEntries(bodies);
};

export default function CompositionEditRoute() {
  const params = useParams<{
    space_id: string;
    composition_id: string;
    revision_id: string;
  }>();
  const spaceId = () => params.space_id;
  const backHref = () =>
    spaceCompositionRevisionPath(
      spaceId(),
      params.composition_id,
      params.revision_id,
    );

  const [draft, setDraft] = createSignal<CompositionDraft | null>(null);
  const [loadError, setLoadError] = createSignal<string | null>(null);

  const load = async () => {
    let spec: unknown;
    try {
      const raw = await compositionApi.get(
        spaceId(),
        params.composition_id,
        params.revision_id,
      );
      spec = raw.fields?.["spec"];
    } catch {
      setLoadError(t("composition.studioLoadFailed"));
      return;
    }
    if (typeof spec !== "string") {
      setLoadError(t("composition.studioInvalidDocument"));
      return;
    }
    let linted: Awaited<ReturnType<typeof compositionApi.lint>>;
    try {
      linted = await compositionApi.lint(spec);
    } catch {
      setLoadError(t("composition.studioLoadFailed"));
      return;
    }
    if (!linted.ok || !linted.value) {
      setLoadError(t("composition.studioInvalidDocument"));
      return;
    }
    const document = linted.value.document;
    if (
      document.format !== "ugoite.composition" ||
      document.format_version !== 1 || document.kind !== "dashboard" ||
      !Array.isArray(document.spec.sources) ||
      !Array.isArray(document.spec.components)
    ) {
      setLoadError(t("composition.studioInvalidDocument"));
      return;
    }
    const sourceNames = await resolveSourceNames(spaceId(), document);
    let savedSqlBodies: Record<string, string>;
    try {
      savedSqlBodies = await resolveSavedSqlBodies(spaceId(), document);
    } catch {
      setLoadError(t("composition.studioLoadFailed"));
      return;
    }
    try {
      setDraft(
        draftFromDocument(
          document as unknown as CompositionStudioRevisionDocument,
          sourceNames,
          savedSqlBodies,
        ),
      );
    } catch {
      setLoadError(t("composition.studioInvalidDocument"));
    }
  };

  onMount(() => {
    void load();
  });

  return (
    <div>
      <Show when={loadError()}>
        <div class="screenHead">
          <div class="screenHeadStart">
            <BackLink
              href={backHref()}
              label={t("composition.studioBackToRevision")}
            />
          </div>
        </div>
        <p class="ui-text-danger" role="alert">{loadError()}</p>
      </Show>
      <Show when={!loadError() && !draft()}>
        <LocalBusyIndicator label={t("composition.detailLoading")} />
      </Show>
      <Show when={draft()}>
        {(ready) => (
          <CompositionStudio
            spaceId={spaceId()}
            initialDraft={ready()}
            saveMode={{
              kind: "update",
              compositionId: params.composition_id,
              baseRevisionId: params.revision_id,
            }}
            backHref={backHref()}
            backLabel={t("composition.studioBackToRevision")}
          />
        )}
      </Show>
    </div>
  );
}
