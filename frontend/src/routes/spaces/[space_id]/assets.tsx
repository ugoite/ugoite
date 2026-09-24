import type { RouteSectionProps } from "@solidjs/router";
import { spaceRoute } from "~/lib/space-shell-route";

export const route = spaceRoute({ navigation: "assets" });

export default function SpaceAssetsRoute(props: RouteSectionProps) {
  return props.children;
}
