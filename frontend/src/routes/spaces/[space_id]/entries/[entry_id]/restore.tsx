import { A, useNavigate, useParams } from "@solidjs/router";
import { onMount } from "solid-js";
import { t } from "~/lib/i18n";
import { spaceRoute } from "~/lib/space-shell-route";

export const route = spaceRoute({ navigation: "forms", title: "entryHistory" });

// Legacy compat: /entries/:entry_id/restore now redirects to the single
// History path (/entries/:entry_id/history). Restore itself lives on the
// revision review route (history/:revision_id) via entryApi.restore.
export default function SpaceEntryRestoreRedirectRoute() {
  const navigate = useNavigate();
  const params = useParams<{ space_id: string; entry_id: string }>();
  const historyPath = () =>
    `/spaces/${params.space_id}/entries/${
      encodeURIComponent(params.entry_id ?? "")
    }/history`;

  onMount(() => {
    if (!params.space_id || !params.entry_id) return;
    navigate(historyPath(), { replace: true });
  });

  return (
    <div class="screenHead">
      <div class="screenTitle">
        <div class="eyebrow">{params.entry_id}</div>
        <h1>{t("entryHistory.title")}</h1>
      </div>
      <A href={historyPath()} class="btn">
        {t("entryRevision.backToHistory")}
      </A>
      <p class="text-sm ui-muted" role="status">{t("entryHistory.loading")}</p>
    </div>
  );
}
