import { useParams } from "@solidjs/router";
import { CompositionStudio } from "~/components/CompositionStudio";
import { createCompositionGraphDraft } from "~/lib/composition-variable-binding";
import { t } from "~/lib/i18n";
import { spaceCompositionsPath } from "~/lib/space-path";
import { spaceRoute } from "~/lib/space-shell-route";

export const route = spaceRoute({ navigation: "compositions" });

export default function CompositionNewRoute() {
  const params = useParams<{ space_id: string }>();
  const initialDraft = createCompositionGraphDraft();

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
