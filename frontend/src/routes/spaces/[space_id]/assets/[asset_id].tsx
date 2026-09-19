import { useNavigate, useParams } from "@solidjs/router";
import { createMemo, createSignal, For, Show } from "solid-js";
import { ActionIconBar } from "~/components/ActionIconBar";
import { BackLink } from "~/components/BackLink";
import { ConfirmDestructiveAction } from "~/components/ConfirmDestructiveAction";
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
  // Delete is BLOCKED (not warned) while any visible Entry references the
  // asset: the button disables and names the referencing Entry+field.
  // Hidden or unauthorized references stay fail-closed on the server; the UI
  // never guesses about references it cannot see.
  const isReferenced = createMemo(() => occurrences().length > 0);
  const referenceSummary = createMemo(() =>
    occurrences()
      .map((occurrence) =>
        `${occurrence.form} · ${occurrence.field} (${occurrence.entry_id})`
      )
      .join(", ")
  );
  const [deleteConfirmOpen, setDeleteConfirmOpen] = createSignal(false);

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

  const openDeleteConfirm = () => {
    if (busy() !== null || isReferenced()) return;
    setActionError(null);
    setDeleteConfirmOpen(true);
  };

  const closeDeleteConfirm = () => {
    if (busy() !== null) return;
    setDeleteConfirmOpen(false);
  };

  const remove = async () => {
    if (busy() !== null || isReferenced()) return;
    setBusy("delete");
    setActionError(null);
    try {
      await assetApi.delete(spaceId(), assetId());
      setDeleteConfirmOpen(false);
      navigate(`/spaces/${encodeURIComponent(spaceId())}/assets`);
    } catch (error) {
      // Server fail-closed stays for hidden/unauthorized references: the
      // failure surfaces here and the detail stays mounted for retry.
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

      {
        /*
        Zero visible references is a valid detail state (not a load error):
        the asset metadata is unknown, but Delete stays available behind a
        confirmation and the server stays fail-closed for hidden references.
      */
      }
      <Show when={!items.loading && !items.error && !head()}>
        <div class="ui-stack-sm">
          <p class="text-sm ui-muted">{t("assetDetail.noReferences")}</p>
          <ActionIconBar
            label={t("assetDetail.heading")}
            actions={[
              {
                id: "delete",
                icon: "trash",
                label: t("assetDetail.delete"),
                accessibleName: t("assetDetail.delete"),
                danger: true,
                busy: busy() === "delete",
                disabled: busy() !== null,
                onClick: () => openDeleteConfirm(),
              },
            ]}
          />
          <Show when={actionError()}>
            <p class="ui-alert ui-alert-error" role="alert">
              {actionError()}
            </p>
          </Show>
          <ConfirmDestructiveAction
            open={deleteConfirmOpen()}
            title={t("assetDetail.delete")}
            body={t("assetDetail.confirmDeleteUnknown")}
            confirmLabel={t("assetDetail.delete")}
            busy={busy() === "delete"}
            error={actionError()}
            onConfirm={() => void remove()}
            onClose={closeDeleteConfirm}
          />
          <details class="settingsAdvanced">
            <summary>{t("settings.advancedDetails")}</summary>
            <p class="text-sm ui-muted">
              <code>{assetId()}</code>
            </p>
          </details>
        </div>
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
                  accessibleName: isReferenced()
                    ? t("assetDetail.deleteBlocked", {
                      references: referenceSummary(),
                    })
                    : `${t("assetDetail.delete")}: ${asset().name}`,
                  danger: true,
                  busy: busy() === "delete",
                  disabled: busy() !== null || isReferenced(),
                  onClick: () => openDeleteConfirm(),
                },
              ]}
            />
            <Show when={isReferenced()}>
              <p class="ui-alert ui-alert-warning text-sm" role="note">
                {t("assetDetail.deleteBlocked", {
                  references: referenceSummary(),
                })}
              </p>
            </Show>
            <Show when={actionError()}>
              <p class="ui-alert ui-alert-error" role="alert">
                {actionError()}
              </p>
            </Show>
            <ConfirmDestructiveAction
              open={deleteConfirmOpen()}
              title={t("assetDetail.delete")}
              body={t("assetDetail.confirmDelete", {
                name: asset().name,
              })}
              confirmLabel={t("assetDetail.delete")}
              busy={busy() === "delete"}
              error={actionError()}
              onConfirm={() => void remove()}
              onClose={closeDeleteConfirm}
            />
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
