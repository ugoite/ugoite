import { A, useNavigate, useParams } from "@solidjs/router";
import { createMemo, createSignal, Show } from "solid-js";
import { assetApi } from "~/lib/ugoite-client";
import { AccessPolicyEditor } from "~/components/AccessPolicyEditor";
import { SpaceShell } from "~/components/SpaceShell";
import { t } from "~/lib/i18n";
import { createResource } from "~/lib/recoverable-resource";

export default function SpaceAssetDetailRoute() {
  const navigate = useNavigate();
  const params = useParams<{ space_id: string; asset_id: string }>();
  const spaceId = () => params.space_id;
  const assetId = () => params.asset_id;
  const [deleteError, setDeleteError] = createSignal<string | null>(null);
  const [isDeleting, setIsDeleting] = createSignal(false);

  const [assets] = createResource(async () => {
    return await assetApi.list(spaceId());
  });

  const asset = createMemo(() => {
    return assets()?.find((item) => item.id === assetId()) || null;
  });

  const handleDelete = async () => {
    setDeleteError(null);
    setIsDeleting(true);
    try {
      await assetApi.delete(spaceId(), assetId());
      navigate(`/spaces/${spaceId()}/assets`);
    } catch (err) {
      setDeleteError(
        err instanceof Error ? err.message : t("assetDetail.failedDelete"),
      );
    } finally {
      setIsDeleting(false);
    }
  };

  return (
    <SpaceShell
      spaceId={spaceId()}
      activeNavigation="forms"
      title={t("assetDetail.heading")}
    >
      <div class="screenHead">
        <div class="screenTitle">
          <div class="eyebrow">
            {t("spaceShell.bottom.grid")} / {t("assetsPage.heading")}
          </div>
          <h1>{t("assetDetail.heading")}</h1>
        </div>
        <A href={`/spaces/${spaceId()}/assets`} class="btn">
          {t("assetDetail.backToAssets")}
        </A>
      </div>

      <div class="settingsMain surface">
        <Show when={assets.loading}>
          <p class="ui-muted">{t("assetDetail.loading")}</p>
        </Show>
        <Show when={assets.error}>
          <p class="ui-alert ui-alert-error">{t("assetDetail.failedLoad")}</p>
        </Show>
        <Show when={asset()}>
          {(item) => (
            <div class="entryGrid">
              <div class="fieldCard wide">
                <p class="text-sm">
                  {t("assetDetail.name", { name: item().name })}
                </p>
                <p class="text-sm ui-muted">
                  {t("assetDetail.id", { id: item().id })}
                </p>
                <p class="text-sm ui-muted">
                  {t("assetDetail.path", { path: item().path })}
                </p>
              </div>
              <div class="fieldCard wide">
                <AccessPolicyEditor
                  spaceId={spaceId()}
                  kind="asset"
                  resourceId={assetId()}
                />
              </div>
              <button
                type="button"
                class="btn danger"
                onClick={handleDelete}
                disabled={isDeleting()}
              >
                {isDeleting()
                  ? t("assetDetail.deleting")
                  : t("assetDetail.delete")}
              </button>
              <Show when={deleteError()}>
                <p class="ui-alert ui-alert-error">{deleteError()}</p>
              </Show>
            </div>
          )}
        </Show>
      </div>
    </SpaceShell>
  );
}
