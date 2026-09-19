import type { RouteSectionProps } from "@solidjs/router";
import { spaceRoute } from "~/lib/space-shell-route";

export const route = spaceRoute({ navigation: "entries" });

export default function SpaceEntryHistoryLayout(props: RouteSectionProps) {
  return props.children;
}
