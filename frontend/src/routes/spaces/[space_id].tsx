import { useLocation, useParams } from "@solidjs/router";
import type { RouteSectionProps } from "@solidjs/router";
import { createMemo } from "solid-js";
import {
  SpaceShell,
  type SpaceNavigation,
} from "~/components/SpaceShell";

export const routeTitle = (
  pathname: string,
  search: string,
): string | undefined => {
  if (pathname.endsWith("/entries/new")) return "New Entry";
  if (/\/entries\/[^/]+\/history\/[^/]+$/.test(pathname)) return "Revision";
  if (/\/entries\/[^/]+\/history$/.test(pathname)) return "Entry / History";
  if (/\/entries\/[^/]+\/restore$/.test(pathname)) return "Restore";
  if (/\/assets\/[^/]+$/.test(pathname)) return "Asset";
  if (pathname.endsWith("/assets")) return "Assets";
  if (pathname.endsWith("/sql")) return "Saved SQL";
  if (/\/sql\/[^/]+$/.test(pathname)) return "Saved SQL detail";
  if (pathname.endsWith("/queries/new")) return "SQL / New";
  if (/\/queries\/[^/]+\/variables$/.test(pathname)) {
    return "SQL / Variables";
  }
  if (pathname.endsWith("/test-connection")) return "Settings / Storage";
  if (pathname.endsWith("/settings")) {
    const section = new URLSearchParams(search).get("section");
    const sectionTitle: Record<string, string> = {
      general: "General",
      members: "Members",
      agents: "Agents",
      credentials: "Credentials",
      storage: "Storage",
    };
    return `Settings / ${sectionTitle[section ?? ""] ?? "General"}`;
  }
  if (pathname.endsWith("/search")) return "Search";
  if (pathname.endsWith("/forms/types")) return "Forms / Field Types";
  if (pathname.endsWith("/forms")) return "Forms";
  return undefined;
};

export const routeNavigation = (pathname: string): SpaceNavigation => {
  if (pathname.includes("/settings")) return "settings";
  if (
    pathname.includes("/search") || pathname.includes("/sql") ||
    pathname.includes("/queries") || pathname.includes("/query")
  ) return "search";
  if (
    pathname.includes("/forms") || pathname.includes("/entries") ||
    pathname.includes("/assets")
  ) return "forms";
  return "home";
};

export default function SpaceLayout(props: RouteSectionProps) {
  const params = useParams<{ space_id: string }>();
  const location = useLocation();
  const spaceId = () => params.space_id;
  const pathname = () => location.pathname;
  const activeNavigation = createMemo(() => routeNavigation(pathname()));
  const title = createMemo(() => routeTitle(pathname(), location.search));

  return (
    <SpaceShell
      spaceId={spaceId()}
      activeNavigation={activeNavigation()}
      title={title()}
    >
      {props.children}
    </SpaceShell>
  );
}
