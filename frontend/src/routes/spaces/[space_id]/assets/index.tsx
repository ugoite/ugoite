import { A, useParams } from "@solidjs/router";
import { For, Show } from "solid-js";
import { LocalBusyIndicator } from "~/components/LocalBusyIndicator";
import { RowList, RowListItem, RowListLink } from "~/components/RowList";
import { formatAssetSize } from "~/lib/asset-reference";
import { intlLocale, t } from "~/lib/i18n";
import { createResource } from "~/lib/recoverable-resource";
import { assetApi, type AssetListItem } from "~/lib/ugoite-client";
import { formatUserFacingError } from "~/lib/user-facing-error";
import { spaceRoute } from "~/lib/space-shell-route";

export const route = spaceRoute({ navigation: "assets" });

type AssetGroup = {
  asset_id: string;
  name: string;
  media_type: string;
  size_bytes: number;
  occurrences: AssetListItem[];
};

const groupAssetItems = (items: AssetListItem[]): AssetGroup[] => {
  const groups = new Map<string, AssetGroup>();
  for (const item of items) {
    const group = groups.get(item.asset_id) ?? {
      asset_id: item.asset_id,
      name: item.name,
      media_type: item.media_type,
      size_bytes: item.size_bytes,
      occurrences: [],
    };
    group.occurrences.push(item);
    groups.set(item.asset_id, group);
  }
  return [...groups.values()].sort((left, right) =>
    left.name.localeCompare(right.name)
  );
};

export default function SpaceAssetsIndexRoute() {
  const params = useParams<{ space_id: string }>();
  const spaceId = () => params.space_id;
  const [items, { refetch }] = createResource(
    spaceId,
    (id) => assetApi.list(id),
  );

  const assetGroups = () => groupAssetItems(items() ?? []);

  return (
    <div class="assetInventory" aria-busy={items.loading || undefined}>
      <h1 class="ui-sr-only" id="assets-files-title">
        {t("assetsPage.filesHeading")}
      </h1>
      <div class="screenHead">
        <A
          class="btn primary"
          href={`/spaces/${encodeURIComponent(spaceId())}/forms`}
        >
          {t("assetsPage.upload")}
        </A>
      </div>

      {/* Panel-local spinner alongside the list: rows stay mounted. */}
      <Show when={items.loading}>
        <LocalBusyIndicator label={t("assetsPage.loading")} />
      </Show>

      <Show when={items.error}>
        <div class="ui-alert ui-alert-error flex flex-wrap items-center gap-3">
          <span>
            {formatUserFacingError(items.error, "assetsPage.failedLoad")}
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

      <Show when={!items.error}>
        <Show
          when={assetGroups().length > 0}
          fallback={
            <Show when={!items.loading}>
              <div class="assetEmpty ui-stack-sm">
                <h2 class="text-base font-semibold">{t("assetsPage.empty")}</h2>
                <p class="text-sm ui-muted">
                  {t("assetsPage.emptyDescription")}
                </p>
              </div>
            </Show>
          }
        >
          <RowList
            label={t("assetsPage.filesHeading")}
            labelledBy="assets-files-title"
          >
            <For each={assetGroups()}>
              {(asset) => (
                <RowListItem
                  main={
                    <RowListLink
                      href={`/spaces/${encodeURIComponent(spaceId())}/assets/${
                        encodeURIComponent(asset.asset_id)
                      }`}
                      primary={asset.name}
                      secondary={asset.media_type}
                      meta={formatAssetSize(
                        asset.size_bytes,
                        intlLocale(),
                      )}
                      chevron
                    />
                  }
                />
              )}
            </For>
          </RowList>
        </Show>
      </Show>
    </div>
  );
}
