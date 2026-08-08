import { useCurrentMatches, useLocation, useParams } from "@solidjs/router";
import type { RouteSectionProps } from "@solidjs/router";
import { createMemo } from "solid-js";
import { t, type TranslationKey } from "~/lib/i18n";
import { spaceRoute, type SpaceShellRouteInfo } from "~/lib/space-shell-route";
import { SpaceShell } from "~/components/SpaceShell";
import { settingsSections } from "~/lib/settings-sections";

export const route = spaceRoute({ navigation: "home" });

const titleKeys: Record<
  Exclude<
    SpaceShellRouteInfo["title"],
    undefined | "settings" | "settingsStorage"
  >,
  TranslationKey
> = {
  asset: "spaceShell.title.asset",
  assets: "spaceShell.title.assets",
  entryHistory: "spaceShell.title.entryHistory",
  newEntry: "spaceShell.title.newEntry",
  restore: "spaceShell.title.restore",
  revision: "spaceShell.title.revision",
  savedSql: "spaceShell.title.savedSql",
  savedSqlDetail: "spaceShell.title.savedSqlDetail",
  sqlNew: "spaceShell.title.sqlNew",
  sqlVariables: "spaceShell.title.sqlVariables",
  formTypes: "spaceShell.title.formTypes",
};

const getRouteInfo = (matches: ReturnType<typeof useCurrentMatches>) => {
  for (const match of [...matches()].reverse()) {
    const info = match.route.info?.spaceShell as
      | SpaceShellRouteInfo
      | undefined;
    if (info) return info;
  }
  return { navigation: "home" as const };
};

export default function SpaceLayout(props: RouteSectionProps) {
  const params = useParams<{ space_id: string }>();
  const matches = useCurrentMatches();
  const location = useLocation();
  const spaceId = () => params.space_id;
  const routeInfo = createMemo(() => getRouteInfo(matches));
  const title = createMemo(() => {
    const info = routeInfo();
    if (!info.title) return undefined;
    if (info.title === "settingsStorage") {
      return `${t("settings.title")} / ${t("settings.section.storage")}`;
    }
    if (info.title === "settings") {
      const section = new URLSearchParams(location.search).get("section");
      const sectionKey = settingsSections.find((item) => item.id === section)
        ?.key ?? "settings.section.general";
      return `${t("settings.title")} / ${t(sectionKey)}`;
    }
    return t(titleKeys[info.title]);
  });

  return (
    <SpaceShell
      spaceId={spaceId()}
      activeNavigation={routeInfo().navigation}
      title={title()}
    >
      {props.children}
    </SpaceShell>
  );
}
