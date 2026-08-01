import type { RouteDefinition } from "@solidjs/router";
import type { SpaceNavigation } from "~/components/SpaceShell";

export type SpaceShellTitle =
  | "asset"
  | "assets"
  | "entryHistory"
  | "newEntry"
  | "restore"
  | "revision"
  | "savedSql"
  | "savedSqlDetail"
  | "settings"
  | "settingsStorage"
  | "sqlNew"
  | "sqlVariables"
  | "formTypes";

export type SpaceShellRouteInfo = {
  navigation: SpaceNavigation;
  title?: SpaceShellTitle;
};

export const spaceRoute = (
  info: SpaceShellRouteInfo,
): Pick<RouteDefinition, "info"> => ({
  info: { spaceShell: info },
});
