import type { RouteSectionProps } from "@solidjs/router";
import { spaceRoute } from "~/lib/space-shell-route";

export const route = spaceRoute({ navigation: "compositions" });

export default function SpaceCompositionLayout(props: RouteSectionProps) {
  return props.children;
}
