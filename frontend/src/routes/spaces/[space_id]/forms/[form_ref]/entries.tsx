import { useNavigate, useParams } from "@solidjs/router";
import { createEffect, createMemo, Show } from "solid-js";
import { BackLink } from "~/components/BackLink";
import { EntryBrowser } from "~/components/EntryBrowser";
import {
  createEntryQueryController,
  type EntryProjection,
  type EntryQueryCapabilities,
  type EntryQueryScope,
  systemEntryCapabilities,
} from "~/lib/entry-query";
import { useEntriesRouteContext } from "~/lib/entries-route-context";
import {
  filterCreatableEntryForms,
  isReservedMetadataForm,
} from "~/lib/metadata-forms";
import { t } from "~/lib/i18n";
import {
  decodeSpaceSegment,
  spaceEntriesPath,
  spaceEntryPath,
  spaceFormsPath,
} from "~/lib/space-path";
import { spaceRoute } from "~/lib/space-shell-route";

export const route = spaceRoute({ navigation: "forms" });

const fieldProjection = (
  capabilities: EntryQueryCapabilities,
): EntryProjection => {
  const fields = capabilities.fields
    .filter((field) => field.projectable && field.field.kind !== "form")
    .map((field) => field.field);
  return fields.length > 0 ? { kind: "fields", fields } : { kind: "preview" };
};

/**
 * Canonical Form-scoped Entry workspace.
 *
 * Forms are the entry point to Entries: this route requires a Form and
 * shows that Form's current Entries beside creation. There is no unscoped
 * all-Forms list surface.
 */
export default function SpaceFormEntriesPane() {
  const navigate = useNavigate();
  const params = useParams<{ space_id: string; form_ref: string }>();
  const ctx = useEntriesRouteContext();
  const spaceId = () => ctx.spaceId();
  // Solid's router typically decodes params already; the segment helper
  // stays idempotent so encoded links resolve the same Form.
  const formRef = () => decodeSpaceSegment(params.form_ref);
  const creatableForms = createMemo(() =>
    filterCreatableEntryForms(ctx.forms())
  );
  const hasCreatableForms = createMemo(() => creatableForms().length > 0);
  const selectedForm = createMemo(() =>
    ctx.forms().find((form) => form.name === formRef())
  );
  const isReservedForm = createMemo(() =>
    formRef() !== "" && isReservedMetadataForm(formRef())
  );

  const queryScope = createMemo<EntryQueryScope>(() =>
    selectedForm()?.id
      ? { kind: "form", form_id: selectedForm()!.id! }
      : { kind: "all" }
  );
  const capabilities = createMemo<EntryQueryCapabilities>(() => {
    const scope = queryScope();
    const system = systemEntryCapabilities(scope);
    const formCapabilities = selectedForm()
      ? Object.values(selectedForm()!.fields)
        .map((field) => field.query_capability)
        .filter((field): field is NonNullable<typeof field> =>
          field !== undefined
        )
      : [];
    return { scope, fields: [...system.fields, ...formCapabilities] };
  });

  const controller = createEntryQueryController(
    () => spaceId(),
    { scope: queryScope(), filters: [], sort: [] },
    fieldProjection(capabilities()),
  );
  let lastQueryConfiguration = "";
  createEffect(() => {
    if (ctx.loadingForms()) return;
    if (!selectedForm()?.id) return;
    const nextQuery = { scope: queryScope(), filters: [], sort: [] };
    const nextProjection = fieldProjection(capabilities());
    const configuration = JSON.stringify({
      space_id: spaceId(),
      nextQuery,
      nextProjection,
    });
    if (configuration === lastQueryConfiguration) return;
    lastQueryConfiguration = configuration;
    void controller.configure(nextQuery, nextProjection);
  });

  const isUnknownForm = createMemo(() => {
    if (!formRef() || ctx.loadingForms()) return false;
    if (isReservedMetadataForm(formRef())) return false;
    return !selectedForm()?.id;
  });

  return (
    <div class="mx-auto max-w-6xl entriesPage">
      <div class="flex flex-wrap items-center justify-between gap-3 entriesHeader">
        <div>
          <h1 class="ui-page-title">
            {formRef()}
          </h1>
          <BackLink
            href={spaceFormsPath(spaceId())}
            label={t("entriesPage.formBack")}
          />
        </div>
      </div>

      <div class="mt-6 entriesBody">
        <Show when={isUnknownForm()}>
          <p class="text-sm ui-muted">
            {t("entriesPage.unknownFormHint", { form: formRef() })}
          </p>
        </Show>
        <Show when={!isReservedForm()}>
          <div class="entriesCreateRow">
            <button
              type="button"
              class="ui-button ui-button-primary text-sm"
              disabled={!hasCreatableForms()}
              onClick={() =>
                navigate(
                  spaceEntriesPath(
                    spaceId(),
                    `/new?form=${encodeURIComponent(formRef())}`,
                  ),
                )}
            >
              {t("entriesPage.newShort")}
            </button>
          </div>
        </Show>
        <Show when={!isReservedForm() && !isUnknownForm()}>
          <EntryBrowser
            controller={controller}
            capabilities={capabilities()}
            formLabels={Object.fromEntries(
              ctx.forms().filter((form) => form.id).map((form) => [
                form.id!,
                form.name,
              ]),
            )}
            onSelect={(row) => navigate(spaceEntryPath(spaceId(), row.id))}
          />
        </Show>
      </div>
    </div>
  );
}
