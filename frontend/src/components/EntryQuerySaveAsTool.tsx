import { useNavigate } from "@solidjs/router";
import { type Accessor, createMemo } from "solid-js";
import { UiIcon } from "./UiIcon";
import { buildEntryQueryComposition } from "~/lib/entry-query-composition";
import type { EntryProjection, EntryQuery } from "~/lib/entry-query";
import type { Form } from "~/lib/types";
import type { CompositionStudioSeed } from "~/lib/composition-draft";
import { spaceCompositionNewPath } from "~/lib/space-path";
import { t } from "~/lib/i18n";

export interface EntryQuerySaveAsToolProps {
  spaceId: Accessor<string>;
  defaultName: Accessor<string>;
  query: Accessor<EntryQuery>;
  projection: Accessor<EntryProjection>;
  form: Accessor<Form | undefined>;
  knownForms: Accessor<readonly Form[]>;
}

/**
 * Seed-navigation toolbar button for an EntryQuery view. The button stays
 * disabled with the builder reason as its accessible name while the
 * current view has no exact Composition grammar (e.g. All-scope search).
 * Otherwise it navigates to the Studio new route carrying the entry-query
 * seed; the Studio stays immediately save-ready.
 */
export function EntryQuerySaveAsTool(props: EntryQuerySaveAsToolProps) {
  const navigate = useNavigate();

  const buildResult = createMemo(() =>
    buildEntryQueryComposition({
      query: props.query(),
      projection: props.projection(),
      form: props.form(),
      knownForms: props.knownForms(),
    })
  );
  const canSave = () => buildResult().status === "ok";
  const buttonLabel = () => {
    const result = buildResult();
    return result.status === "ok"
      ? t("composition.saveAsTool")
      : `${t("composition.saveAsTool")}, ${t(result.reason)}`;
  };

  const openInStudio = () => {
    const result = buildResult();
    if (result.status !== "ok") return;
    const seed: CompositionStudioSeed = {
      kind: "entry_query",
      seed: {
        formId: result.source.form_id,
        name: props.defaultName(),
        fieldSchema: result.fieldSchema,
        query: {
          ...(result.source.query.text !== undefined
            ? { text: result.source.query.text }
            : {}),
          filters: result.source.query.filters,
          sort: result.source.query.sort,
          projection: result.source.query.projection,
        },
      },
    };
    navigate(spaceCompositionNewPath(props.spaceId()), { state: { seed } });
  };

  return (
    <button
      type="button"
      class="ui-button ui-button-secondary entry-browser-save-tool"
      aria-label={buttonLabel()}
      title={buttonLabel()}
      disabled={!canSave()}
      onClick={openInStudio}
    >
      <UiIcon name="save" />
    </button>
  );
}
