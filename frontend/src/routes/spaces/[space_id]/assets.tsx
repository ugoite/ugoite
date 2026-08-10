import { A, useParams } from "@solidjs/router";
import { For, Show } from "solid-js";
import { UiIcon } from "~/components/UiIcon";
import { formatDateLabel } from "~/lib/date-format";
import { formatAssetSize, isAssetReference } from "~/lib/asset-reference";
import { intlLocale, t } from "~/lib/i18n";
import { createResource } from "~/lib/recoverable-resource";
import { entryApi } from "~/lib/ugoite-client";
import type { AssetReference, EntryRecord } from "~/lib/types";
import { formatUserFacingError } from "~/lib/user-facing-error";
import { spaceRoute } from "~/lib/space-shell-route";

export const route = spaceRoute({ navigation: "forms", title: "assets" });

type AssetOccurrence = {
  entry: Pick<EntryRecord, "id" | "title" | "form" | "updated_at">;
  field: string;
};

type AssetGroup = {
  reference: AssetReference;
  occurrences: AssetOccurrence[];
};

// Keep each request below the server's normal read ceiling. The entry.list
// offset is part of the portable protocol, so this workspace can derive a
// complete inventory from the Form-owned Entry source of truth.
const ASSET_WORKSPACE_PAGE_SIZE = 1_000;

const listAllEntries = async (spaceId: string): Promise<EntryRecord[]> => {
  const entries: EntryRecord[] = [];
  let offset = 0;

  while (true) {
    const page = offset === 0
      ? await entryApi.list(spaceId, ASSET_WORKSPACE_PAGE_SIZE)
      : await entryApi.list(spaceId, ASSET_WORKSPACE_PAGE_SIZE, offset);
    entries.push(...page);
    if (page.length < ASSET_WORKSPACE_PAGE_SIZE) return entries;
    offset += page.length;
  }
};

const referencesFromValue = (value: unknown): AssetReference[] => {
  if (isAssetReference(value)) return [value];
  if (!Array.isArray(value)) return [];
  return value.filter(isAssetReference);
};

const groupAssetReferences = (entries: EntryRecord[]): AssetGroup[] => {
  const groups = new Map<string, AssetGroup>();

  for (const entry of entries) {
    for (const [field, value] of Object.entries(entry.properties ?? {})) {
      for (const reference of referencesFromValue(value)) {
        const group = groups.get(reference.asset_id) ?? {
          reference,
          occurrences: [],
        };
        group.occurrences.push({
          entry: {
            id: entry.id,
            title: entry.title,
            form: entry.form,
            updated_at: entry.updated_at,
          },
          field,
        });
        groups.set(reference.asset_id, group);
      }
    }
  }

  return [...groups.values()].sort((left, right) =>
    left.reference.name.localeCompare(right.reference.name)
  );
};

export default function SpaceAssetsRoute() {
  const params = useParams<{ space_id: string }>();
  const spaceId = () => params.space_id;
  const [entries, { refetch }] = createResource(
    spaceId,
    listAllEntries,
  );

  const assetGroups = () => groupAssetReferences(entries() ?? []);

  return (
    <>
      <div class="screenHead">
        <div class="screenTitle">
          <div class="eyebrow">{t("assetsPage.eyebrow")}</div>
          <h1>{t("assetsPage.heading")}</h1>
        </div>
      </div>

      <p class="mb-6 max-w-3xl text-sm ui-muted">
        {t("assetsPage.description")}
      </p>

      <Show when={entries.loading}>
        <p role="status" class="ui-muted">{t("assetsPage.loading")}</p>
      </Show>

      <Show when={entries.error}>
        <div class="ui-alert ui-alert-error flex flex-wrap items-center gap-3">
          <span>
            {formatUserFacingError(
              entries.error,
              "assetsPage.failedLoad",
              "entry.list",
            )}
          </span>
          <button
            type="button"
            class="ui-button ui-button-secondary text-sm"
            onClick={() => void refetch()}
          >
            {t("assetsPage.retry")}
          </button>
        </div>
      </Show>

      <Show when={!entries.loading && !entries.error}>
        <p role="status" class="mb-4 text-sm ui-muted">
          {t("assetsPage.complete")}
        </p>
        <Show
          when={assetGroups().length > 0}
          fallback={
            <div class="ui-card ui-stack-sm p-6">
              <h2 class="text-base font-semibold">{t("assetsPage.empty")}</h2>
              <p class="text-sm ui-muted">{t("assetsPage.emptyDescription")}</p>
              <div>
                <A
                  class="ui-button ui-button-secondary inline-flex items-center gap-2 text-sm"
                  href={`/spaces/${spaceId()}/forms`}
                >
                  <UiIcon name="forms" />
                  {t("assetsPage.openForms")}
                </A>
              </div>
            </div>
          }
        >
          <div class="grid gap-4 md:grid-cols-2">
            <For each={assetGroups()}>
              {(asset) => (
                <article class="ui-card ui-stack-sm p-5">
                  <div class="flex items-start gap-3">
                    <span class="ui-icon-tile" aria-hidden="true">
                      <UiIcon name="asset" />
                    </span>
                    <div class="min-w-0">
                      <h2 class="truncate text-base font-semibold">
                        {asset.reference.name}
                      </h2>
                      <p class="text-sm ui-muted">
                        {asset.reference.media_type} · {formatAssetSize(
                          asset.reference.size_bytes,
                          intlLocale(),
                        )}
                      </p>
                    </div>
                  </div>

                  <dl class="grid gap-2 text-sm sm:grid-cols-2">
                    <div>
                      <dt class="ui-label">{t("assetsPage.id")}</dt>
                      <dd
                        class="truncate ui-muted"
                        title={asset.reference.asset_id}
                      >
                        {asset.reference.asset_id}
                      </dd>
                    </div>
                    <div>
                      <dt class="ui-label">{t("assetsPage.references")}</dt>
                      <dd class="ui-muted">{asset.occurrences.length}</dd>
                    </div>
                  </dl>

                  <div class="ui-stack-sm border-t border-[var(--ui-border)] pt-3">
                    <h3 class="ui-label">{t("assetsPage.entryReferences")}</h3>
                    <For each={asset.occurrences}>
                      {(occurrence) => (
                        <A
                          class="ui-card ui-card-interactive flex items-center justify-between gap-3 p-3 text-sm"
                          href={`/spaces/${spaceId()}/entries/${
                            encodeURIComponent(
                              occurrence.entry.id,
                            )
                          }`}
                        >
                          <span class="min-w-0">
                            <span class="block truncate font-medium">
                              {occurrence.entry.title || t("common.untitled")}
                            </span>
                            <span class="block truncate ui-muted">
                              {occurrence.entry.form || t("assetsPage.entry")} ·
                              {" "}
                              {occurrence.field}
                            </span>
                          </span>
                          <span class="shrink-0 text-xs ui-muted">
                            {formatDateLabel(occurrence.entry.updated_at)}
                          </span>
                        </A>
                      )}
                    </For>
                  </div>
                </article>
              )}
            </For>
          </div>
        </Show>
      </Show>
    </>
  );
}
