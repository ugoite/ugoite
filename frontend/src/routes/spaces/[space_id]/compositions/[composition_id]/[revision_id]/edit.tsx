import { useParams } from "@solidjs/router";
import { createSignal, onMount, Show } from "solid-js";
import { BackLink } from "~/components/BackLink";
import { CompositionStudio } from "~/components/CompositionStudio";
import { LocalBusyIndicator } from "~/components/LocalBusyIndicator";
import { compositionApi } from "~/lib/composition-api";
import {
  type CompositionGraphDraft,
  compositionGraphDraftFromDocument,
} from "~/lib/composition-variable-binding";
import { t } from "~/lib/i18n";
import { spaceCompositionRevisionPath } from "~/lib/space-path";
import { spaceRoute } from "~/lib/space-shell-route";

export const route = spaceRoute({ navigation: "compositions" });

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
  const [draft, setDraft] = createSignal<CompositionGraphDraft | null>(null);
  const [loadError, setLoadError] = createSignal<string | null>(null);

  const load = async () => {
    try {
      const raw = await compositionApi.get(
        spaceId(),
        params.composition_id,
        params.revision_id,
      );
      const spec = raw.fields?.["spec"];
      if (typeof spec !== "string") throw new Error("Missing spec");
      const linted = await compositionApi.lint(spec);
      if (!linted.ok || !linted.value) throw new Error("Invalid spec");
      const canonical = await compositionApi.canonicalizeDocument(
        linted.value.document,
      );
      setDraft(compositionGraphDraftFromDocument(canonical.document));
    } catch {
      setLoadError(t("composition.studioInvalidDocument"));
    }
  };

  onMount(() => void load());

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
