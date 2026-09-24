import { useCurrentMatches, useParams } from "@solidjs/router";
import type { RouteSectionProps } from "@solidjs/router";
import { createMemo } from "solid-js";
import { spaceRoute, type SpaceShellRouteInfo } from "~/lib/space-shell-route";
import { SpaceShell } from "~/components/SpaceShell";

export const route = spaceRoute({ navigation: "home" });

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
  const spaceId = () => params.space_id;
  const routeInfo = createMemo(() => getRouteInfo(matches));

  return (
    <SpaceShell
      spaceId={spaceId()}
      activeNavigation={routeInfo().navigation}
    >
      {props.children}
    </SpaceShell>
  );
}
