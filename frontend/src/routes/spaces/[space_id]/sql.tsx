import type { RouteSectionProps } from "@solidjs/router";
import { spaceRoute } from "~/lib/space-shell-route";

export const route = spaceRoute({ navigation: "search" });

export default function SpaceSqlRoute(props: RouteSectionProps) {
  return props.children;
}
