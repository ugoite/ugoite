import { Show } from "solid-js";
import type { JSX } from "solid-js";
import { UiIcon } from "~/components/UiIcon";
import { formatDateLabel } from "~/lib/date-format";
import { t } from "~/lib/i18n";
import { isReservedMetadataForm } from "~/lib/metadata-forms";
import { displaySqlName } from "~/lib/sql-metadata";
import type { SqlEntry } from "~/lib/types";

/**
 * Shared presentational row labels for Forms and Saved SQL (POL-UI-003,
 * POL-UI-006, POL-UI-007). The Forms/Saved SQL list pages and the
 * Composition Studio source picker render the same labels, so a label
 * improvement reflects both ways. The labels carry human names only;
 * activation (link vs button), chevrons, and row actions stay with the
 * call sites, so page behavior is unchanged.
 */

/** Form row primary: glyph plus human name plus the system-form marker. */
export function FormRowLabel(props: { name: string }): JSX.Element {
  return (
    <>
      <span class="glyph" aria-hidden="true">
        {props.name.slice(0, 1).toUpperCase()}
      </span>
      <span class="formRowName">{props.name}</span>
      <Show when={isReservedMetadataForm(props.name)}>
        <span
          class="systemFormIcon"
          aria-label={t("formsPage.systemForm")}
          title={t("formsPage.systemForm")}
        >
          <UiIcon name="storage" />
        </span>
      </Show>
    </>
  );
}

export type SavedSqlRowEntry = Pick<
  SqlEntry,
  "name" | "kind" | "metadata" | "variables" | "updated_at"
>;

/** Saved SQL row primary: human query name only, never raw identifiers. */
export function SavedSqlRowLabel(
  props: { entry: SavedSqlRowEntry },
): JSX.Element {
  return <>{displaySqlName(props.entry)}</>;
}

/** Saved SQL row secondary: the variables flow marker when it exists. */
export function savedSqlRowSecondary(
  entry: SavedSqlRowEntry,
): string | undefined {
  return entry.variables.length > 0 ? t("searchPage.variables") : undefined;
}

/** Saved SQL row meta: the human updated-date label. */
export function savedSqlRowMeta(entry: SavedSqlRowEntry): string {
  return formatDateLabel(entry.updated_at);
}
