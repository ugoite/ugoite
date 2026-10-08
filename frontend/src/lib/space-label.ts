import type { Space } from "~/lib/types";
import { t } from "~/lib/i18n";

/** Prefer the Space's human name/slug; never use its route UID as display text. */
export const spaceDisplayLabel = (
  space: Pick<Space, "name" | "slug">,
): string => space.name?.trim() || space.slug?.trim() || t("common.untitled");
