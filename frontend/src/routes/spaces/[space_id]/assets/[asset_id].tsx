import { useNavigate, useParams } from "@solidjs/router";
import { createMemo, createSignal, For, Show } from "solid-js";
import { ActionIconBar } from "~/components/ActionIconBar";
import { BackLink } from "~/components/BackLink";
import { LocalBusyIndicator } from "~/components/LocalBusyIndicator";
import { RowList, RowListItem, RowListLink } from "~/components/RowList";
import { formatAssetSize } from "~/lib/asset-reference";
import { intlLocale, t } from "~/lib/i18n";
import { createResource } from "~/lib/recoverable-resource";
import { assetApi, type AssetListItem } from "~/lib/ugoite-client";
import { formatUserFacingError } from "~/lib/user-facing-error";
import { spaceRoute } from "~/lib/space-shell-route";

export const route = spaceRoute({ navigation: "assets", title: "asset" });

const groupOccurrences = (
  items: AssetListItem[],
  assetId: string,
): AssetListItem[] => items.filter((item) => item.asset_id === assetId);

export default function SpaceAssetDetailRoute() {
  const params = useParams<{ space_id: string; asset_id: string }>();
  const navigate = useNavigate();
  const spaceId = () => params.space_id;
  const assetId = () => params.asset_id;
  const [items] = createResource(spaceId, (id) => assetApi.list(id));
  const [actionError, setActionError] = createSignal<string | null>(null);
  const [busy, setBusy] = createSignal<"download" | "delete" | null>(null);

  const occurrences = createMemo(() =>
    groupOccurrences(items() ?? [], assetId())
  );
  const head = createMemo(() => occurrences()[0]);

  const download = async () => {
    const first = head();
    if (!first || busy() !== null) return;
    setBusy("download");
    setActionError(null);
    try {
      const blob = await assetApi.read(
        spaceId(),
        assetId(),
        first.form,
        first.entry_id,
      );
      const url = URL.createObjectURL(blob);
      try {
        const link = document.createElement("a");
        link.href = url;
        link.download = first.name;
        document.body.appendChild(link);
        link.click();
        link.remove();
      } finally {
        setTimeout(() => URL.revokeObjectURL(url), 1000);
      }
    } catch (error) {
      setActionError(
        formatUserFacingError(error, "assetDetail.failedLoad"),
      );
    } finally {
      setBusy(null);
    }
  };

  const remove = async () => {
    if (busy() !== null) return;
    setBusy("delete");
    setActionError(null);
    try {
      await assetApi.delete(spaceId(), assetId());
      navigate(`/spaces/${encodeURIComponent(spaceId())}/assets`);
    } catch (error) {
      setActionError(
        formatUserFacingError(error, "assetDetail.failedDelete"),
      );
      setBusy(null);
    }
  };

  return (
    <>
      <div class="screenHead">
        <div class="screenTitle">
          <div class="eyebrow">{t("assetsPage.eyebrow")}</div>
          <Show
            when={head()}
            fallback={<h1>{t("assetDetail.heading")}</h1>}
          >
            {(asset) => <h1>{asset().name}</h1>}
          </Show>
        </div>
        <BackLink
          href={`/spaces/${encodeURIComponent(spaceId())}/assets`}
          label={t("assetDetail.backToAssets")}
        />
      </div>

      <Show when={items.loading && !head()}>
        <LocalBusyIndicator label={t("assetDetail.loading")} />
      </Show>
      <Show when={items.error}>
        <p class="ui-alert ui-alert-error">
          {formatUserFacingError(items.error, "assetDetail.failedLoad")}
        </p>
      </Show>
      <Show when={!items.loading && !items.error && !head()}>
        <p class="ui-muted">{t("assetDetail.failedLoad")}</p>
      </Show>

      <Show when={head()}>
        {(asset) => (
          <div class="ui-stack-sm">
            <p class="text-sm ui-muted">
              {asset().media_type} · {formatAssetSize(
                asset().size_bytes,
                intlLocale(),
              )}
            </p>
            <ActionIconBar
              label={t("assetDetail.heading")}
              actions={[
                {
                  id: "download",
                  icon: "download",
                  label: t("assetDetail.download"),
                  busy: busy() === "download",
                  disabled: busy() !== null,
                  onClick: () => void download(),
                },
                {
                  id: "delete",
                  icon: "trash",
                  label: t("assetDetail.delete"),
                  accessibleName: `${t("assetDetail.delete")}: ${asset().name}`,
                  danger: true,
                  busy: busy() === "delete",
                  disabled: busy() !== null,
                  onClick: () => void remove(),
                },
              ]}
            />
            <Show when={actionError()}>
              <p class="ui-alert ui-alert-error">{actionError()}</p>
            </Show>
            <details class="settingsAdvanced">
              <summary>{t("settings.advancedDetails")}</summary>
              <p class="text-sm ui-muted">
                <code>{asset().asset_id}</code>
              </p>
            </details>
            <section aria-labelledby="asset-references-title">
              <h2 id="asset-references-title" class="text-base font-semibold">
                {t("assetDetail.references")}
              </h2>
              <Show
                when={occurrences().length > 0}
                fallback={
                  <p class="text-sm ui-muted">
                    {t("assetDetail.noReferences")}
                  </p>
                }
              >
                <RowList label={t("assetDetail.references")}>
                  <For each={occurrences()}>
                    {(occurrence) => (
                      <RowListItem
                        main={
                          <RowListLink
                            href={`/spaces/${
                              encodeURIComponent(spaceId())
                            }/entries/${
                              encodeURIComponent(occurrence.entry_id)
                            }`}
                            primary={occurrence.form}
                            secondary={occurrence.field}
                            chevron
                          />
                        }
                      />
                    )}
                  </For>
                </RowList>
              </Show>
            </section>
          </div>
        )}
      </Show>
    </>
  );
}
