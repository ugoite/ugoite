import { useParams } from "@solidjs/router";
import { CompositionStudio } from "~/components/CompositionStudio";
import { createEmptyDraft } from "~/lib/composition-draft";
import { t } from "~/lib/i18n";
import { spaceCompositionsPath } from "~/lib/space-path";
import { spaceRoute } from "~/lib/space-shell-route";

export const route = spaceRoute({ navigation: "home" });

export default function CompositionNewRoute() {
  const params = useParams<{ space_id: string }>();

  return (
    <CompositionStudio
      spaceId={params.space_id}
      initialDraft={createEmptyDraft()}
      saveMode={{ kind: "create" }}
      backHref={spaceCompositionsPath(params.space_id)}
      backLabel={t("composition.listHeading")}
    />
  );
}
