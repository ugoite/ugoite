import { useCurrentMatches, useLocation, useParams } from "@solidjs/router";
import type { RouteSectionProps } from "@solidjs/router";
import { createMemo } from "solid-js";
import { locale } from "~/lib/i18n";
import { spaceRoute, type SpaceShellRouteInfo } from "~/lib/space-shell-route";
import { SpaceShell } from "~/components/SpaceShell";

export const route = spaceRoute({ navigation: "home" });

const titleCopy = {
  en: {
    asset: "Asset",
    assets: "Assets",
    entryHistory: "Entry / History",
    newEntry: "New Entry",
    restore: "Restore",
    revision: "Revision",
    savedSql: "Saved SQL",
    savedSqlDetail: "Saved SQL detail",
    settings: "Settings",
    settingsStorage: "Settings / Storage",
    sqlNew: "SQL / New",
    sqlVariables: "SQL / Variables",
    formTypes: "Forms / Field Types",
  },
  ja: {
    asset: "アセット",
    assets: "アセット",
    entryHistory: "エントリー / 履歴",
    newEntry: "新しいエントリー",
    restore: "復元",
    revision: "リビジョン",
    savedSql: "保存済みSQL",
    savedSqlDetail: "保存済みSQL詳細",
    settings: "設定",
    settingsStorage: "設定 / ストレージ",
    sqlNew: "SQL / 新規",
    sqlVariables: "SQL / 変数",
    formTypes: "フォーム / フィールドタイプ",
  },
} as const;

const settingsSectionCopy = {
  en: {
    general: "General",
    members: "Members",
    agents: "Agents",
    credentials: "Credentials",
    storage: "Storage",
  },
  ja: {
    general: "一般",
    members: "メンバー",
    agents: "エージェント",
    credentials: "認証情報",
    storage: "ストレージ",
  },
} as const;

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
    const language = locale() === "ja" ? "ja" : "en";
    const copy = titleCopy[language];
    if (info.title !== "settings") return copy[info.title];

    const section = new URLSearchParams(location.search).get("section") ??
      "general";
    const sectionCopy = settingsSectionCopy[language];
    return `${copy.settings} / ${
      sectionCopy[section as keyof typeof sectionCopy] ?? sectionCopy.general
    }`;
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
