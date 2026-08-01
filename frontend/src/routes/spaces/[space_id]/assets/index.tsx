import { A, useParams } from "@solidjs/router";
import { createSignal, Show } from "solid-js";
import { AssetUploader } from "~/components/AssetUploader";
import { assetApi } from "~/lib/ugoite-client";
import { t } from "~/lib/i18n";
import { createResource } from "~/lib/recoverable-resource";
import type { Asset } from "~/lib/types";
import { formatUserFacingError } from "~/lib/user-facing-error";
import { spaceRoute } from "~/lib/space-shell-route";

export const route = spaceRoute({ navigation: "forms", title: "assets" });

export default function SpaceAssetsRoute() {
  const params = useParams<{ space_id: string }>();
  const spaceId = () => params.space_id;
  const [actionError, setActionError] = createSignal<string | null>(null);
  const [assets, { refetch }] = createResource(() => assetApi.list(spaceId()));

  const handleUpload = async (file: File): Promise<Asset> => {
    setActionError(null);
    const created = await assetApi.upload(spaceId(), file, file.name);
    await refetch();
    return created;
  };

  const handleRemove = async (assetId: string) => {
    setActionError(null);
    try {
      await assetApi.delete(spaceId(), assetId);
      await refetch();
    } catch (error) {
      setActionError(
        formatUserFacingError(error, "assetDetail.failedDelete"),
      );
    }
  };

  return (
    <>
      <div class="screenHead">
        <div class="screenTitle">
          <div class="eyebrow">{t("spaceShell.bottom.grid")}</div>
          <h1>{t("assetsPage.heading")}</h1>
        </div>
        <A href={`/spaces/${spaceId()}/entries`} class="btn">
          {t("assetsPage.backToEntries")}
        </A>
      </div>
      <div class="settingsMain surface">
        <AssetUploader
          assets={assets() || []}
          onUpload={handleUpload}
          onRemove={handleRemove}
        />
        <Show when={actionError()}>
          <p class="ui-alert ui-alert-error">{actionError()}</p>
        </Show>
        <Show when={assets.loading}>
          <p class="ui-muted">{t("dashboard.section.assets.loading")}</p>
        </Show>
        <Show when={assets.error}>
          <p class="ui-alert ui-alert-error">
            {formatUserFacingError(assets.error, "assetsPage.failedLoad")}
          </p>
        </Show>
      </div>
    </>
  );
}
