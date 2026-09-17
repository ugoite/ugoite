import { A, useParams } from "@solidjs/router";
import { For, Show } from "solid-js";
import { LocalBusyIndicator } from "~/components/LocalBusyIndicator";
import { UiIcon } from "~/components/UiIcon";
import { formatDateLabel } from "~/lib/date-format";
import { formatAssetSize } from "~/lib/asset-reference";
import { readAssetReferences } from "~/lib/draft-values";
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
  return readAssetReferences(value).references;
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
    <div class="assetInventory" aria-busy={entries.loading || undefined}>
      <div class="screenHead">
        <div class="screenTitle">
          <div class="eyebrow">{t("assetsPage.eyebrow")}</div>
          <h1>{t("assetsPage.filesHeading")}</h1>
        </div>
      </div>

      <p class="mb-6 max-w-3xl text-sm ui-muted">
        {t("assetsPage.description")}
      </p>

      {/* Panel-local spinner alongside the list: rows stay mounted. */}
      <Show when={entries.loading}>
        <LocalBusyIndicator label={t("assetsPage.loading")} />
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

      <Show when={!entries.error}>
        <Show when={!entries.loading}>
          <p role="status" class="mb-4 text-sm ui-muted">
            {t("assetsPage.complete")}
          </p>
        </Show>
        <Show
          when={assetGroups().length > 0}
          fallback={
            <Show when={!entries.loading}>
              <div class="assetEmpty ui-stack-sm">
                <h2 class="text-base font-semibold">{t("assetsPage.empty")}</h2>
                <p class="text-sm ui-muted">
                  {t("assetsPage.emptyDescription")}
                </p>
                <div>
                  <A
                    class="ui-button ui-button-secondary inline-flex items-center gap-2 text-sm"
                    href={`/spaces/${encodeURIComponent(spaceId())}/forms`}
                  >
                    <UiIcon name="forms" />
                    {t("assetsPage.openForms")}
                  </A>
                </div>
              </div>
            </Show>
          }
        >
          <div class="assetRows" role="list">
            <For each={assetGroups()}>
              {(asset) => (
                <article class="spaceAssetRow" role="listitem">
                  <div class="assetRowHeader">
                    <span class="assetRowIcon" aria-hidden="true">
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

                  <details class="assetRowDetails">
                    <summary>{t("assetsPage.details")}</summary>
                    <dl class="assetRowMetaGrid">
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
                  </details>

                  <div class="assetReferences">
                    <h3 class="ui-label">{t("assetsPage.entryReferences")}</h3>
                    <For each={asset.occurrences}>
                      {(occurrence) => (
                        <A
                          class="assetReferenceRow"
                          href={`/spaces/${encodeURIComponent(spaceId())}/entries/${
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
    </div>
  );
}
