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
  type CompositionStudioDocument,
  draftFromDocument,
} from "~/lib/composition-draft";
import { t } from "~/lib/i18n";
import { spaceCompositionRevisionPath } from "~/lib/space-path";
import { spaceRoute } from "~/lib/space-shell-route";
import { formApi, sqlApi } from "~/lib/ugoite-client";

export const route = spaceRoute({ navigation: "home" });

/**
 * Display-only source-name lookup for restoring a draft from the exact
 * revision. Matches the stable Saved SQL entry id or Form id only; names
 * never participate in semantics and missing metadata falls back to the
 * document source id.
 */
const resolveSourceNames = async (
  spaceId: string,
  document: CompositionLintDocument,
): Promise<Record<string, string>> => {
  const names: Record<string, string> = {};
  let formNames = new Map<string, string>();
  try {
    const forms = await formApi.list(spaceId);
    formNames = new Map(forms.map((form) => [form.id, form.name]));
  } catch {
    formNames = new Map();
  }
  for (const source of document.spec.sources) {
    if (source.kind === "saved_sql" && source.entry_id) {
      try {
        const entry = await sqlApi.get(spaceId, source.entry_id);
        if (entry.name) names[source.id] = entry.name;
      } catch {
        // Fall back to the document source id below.
      }
    } else if (source.kind === "entry_query" && source.form_id) {
      const name = formNames.get(source.form_id);
      if (name) names[source.id] = name;
    }
  }
  return names;
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
    try {
      setDraft(
        draftFromDocument(
          document as unknown as CompositionStudioDocument,
          sourceNames,
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
        <p class="ui-text-danger" role="alert">{loadError()}</p>
        <BackLink
          href={backHref()}
          label={t("composition.studioBackToRevision")}
        />
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
