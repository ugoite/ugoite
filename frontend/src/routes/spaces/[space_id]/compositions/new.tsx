import { useParams, useSearchParams } from "@solidjs/router";
import { CompositionStudio } from "~/components/CompositionStudio";
import {
  type CompositionSavedSqlRevisionRef,
  createCompositionGraphDraft,
} from "~/lib/composition-variable-binding";
import { t } from "~/lib/i18n";
import { spaceCompositionsPath } from "~/lib/space-path";
import { spaceRoute } from "~/lib/space-shell-route";

export const route = spaceRoute({ navigation: "compositions" });

export default function CompositionNewRoute() {
  const params = useParams<{ space_id: string }>();
  const [searchParams] = useSearchParams();
  const initialDraft = createCompositionGraphDraft();
  const initialSavedSqlBinding = ():
    | CompositionSavedSqlRevisionRef
    | undefined => {
    const entryId = searchParams.saved_sql_id;
    const revisionId = searchParams.saved_sql_revision;
    return typeof entryId === "string" && entryId &&
        typeof revisionId === "string" && revisionId
      ? { entry_id: entryId, revision_id: revisionId }
      : undefined;
  };

  return (
    <CompositionStudio
      spaceId={params.space_id}
      initialDraft={initialDraft}
      initialSavedSqlBinding={initialSavedSqlBinding()}
      saveMode={{ kind: "create" }}
      backHref={spaceCompositionsPath(params.space_id)}
      backLabel={t("composition.listHeading")}
    />
  );
}
