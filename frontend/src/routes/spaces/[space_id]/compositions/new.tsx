import { useLocation, useParams } from "@solidjs/router";
import { onMount } from "solid-js";
import { CompositionStudio } from "~/components/CompositionStudio";
import {
  applyStudioSeed,
  createEmptyDraft,
  studioSeedState,
} from "~/lib/composition-draft";
import { t } from "~/lib/i18n";
import { spaceCompositionsPath } from "~/lib/space-path";
import { spaceRoute } from "~/lib/space-shell-route";

export const route = spaceRoute({ navigation: "compositions" });

export default function CompositionNewRoute() {
  const params = useParams<{ space_id: string }>();
  const location = useLocation();
  // The seed is consumed once, on mount, when present. The draft keeps the
  // seeded source with the prefilled tool name, so the Studio is save-ready
  // immediately. Clearing the location state keeps back/forward from
  // double-adding the same source.
  const seed = studioSeedState(location.state);
  const initialDraft = seed
    ? applyStudioSeed(createEmptyDraft(), seed.seed).draft
    : createEmptyDraft();
  onMount(() => {
    if (!seed) return;
    history.replaceState(null, "", location.pathname);
  });

  return (
    <CompositionStudio
      spaceId={params.space_id}
      initialDraft={initialDraft}
      saveMode={{ kind: "create" }}
      backHref={spaceCompositionsPath(params.space_id)}
      backLabel={t("composition.listHeading")}
    />
  );
}
