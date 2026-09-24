import type { RouteDefinition } from "@solidjs/router";
import type { SpaceNavigation } from "~/components/SpaceShell";

export type SpaceShellRouteInfo = { navigation: SpaceNavigation };

export const spaceRoute = (
  info: SpaceShellRouteInfo,
): Pick<RouteDefinition, "info"> => ({
  info: { spaceShell: info },
});
