import { createMemo, For, onMount, Show } from "solid-js";
import type { Accessor } from "solid-js";
import { createEntryStore } from "~/lib/entry-store";
import type { EntryRecord } from "~/lib/types";
import { entryDisplayLabel } from "~/lib/entry-label";
import { t } from "~/lib/i18n";
import { formatDateLabel } from "~/lib/date-format";
import { formatValueForDisplay } from "~/lib/display-value";
import { LocalBusyIndicator } from "./LocalBusyIndicator";

/** Props for controlled mode (passing external state) */
export interface EntryListControlledProps {
  entries: Accessor<EntryRecord[]>;
  loading: Accessor<boolean>;
  error: Accessor<string | null>;
  selectedEntryId?: string;
  onSelect?: (entryId: string) => void;
}

/** Props for standalone mode (manages its own state) */
export interface EntryListStandaloneProps {
  spaceId: string;
  selectedEntryId?: string;
  onSelect?: (entryId: string) => void;
}

export type EntryListProps =
  | EntryListControlledProps
  | EntryListStandaloneProps;

function isControlled(
  props: EntryListProps,
): props is EntryListControlledProps {
  return "entries" in props && typeof props.entries === "function";
}

export function EntryList(props: EntryListProps) {
  // Either use provided state or create internal store
  const controlled = isControlled(props);

  // For standalone mode, create internal store
  const internalStore = controlled
    ? null
    : createEntryStore(() => (props as EntryListStandaloneProps).spaceId);

  const standaloneStore = internalStore as NonNullable<typeof internalStore>;
  const entries = createMemo(() => {
    if (controlled) return (props as EntryListControlledProps).entries();
    return standaloneStore.entries();
  });
  const loading = createMemo(() => {
    if (controlled) return (props as EntryListControlledProps).loading();
    return standaloneStore.loading();
  });
  const error = createMemo(() => {
    if (controlled) return (props as EntryListControlledProps).error();
    return standaloneStore.error();
  });

  onMount(() => {
    // Only load if in standalone mode
    if (!controlled && internalStore) {
      internalStore.loadEntries();
    }
  });

  const handleEntryClick = (entryId: string) => {
    props.onSelect?.(entryId);
  };

  /* v8 ignore start */
  return (
    <div class="entry-list-container" aria-busy={loading() || undefined}>
      {/* Panel-local spinner alongside the list: rows stay mounted. */}
      <Show when={loading()}>
        <LocalBusyIndicator label={t("entriesList.loading")} />
      </Show>

      <Show when={error()}>
        <div class="error-message ui-alert ui-alert-error flex items-start gap-2">
          <svg
            class="w-5 h-5 flex-shrink-0 mt-0.5"
            fill="currentColor"
            viewBox="0 0 20 20"
          >
            <path
              fill-rule="evenodd"
              d="M10 18a8 8 0 100-16 8 8 0 000 16zM8.707 7.293a1 1 0 00-1.414 1.414L8.586 10l-1.293 1.293a1 1 0 101.414 1.414L10 11.414l1.293 1.293a1 1 0 001.414-1.414L11.414 10l1.293-1.293a1 1 0 00-1.414-1.414L10 8.586 8.707 7.293z"
              clip-rule="evenodd"
            />
          </svg>
          <span>{error()}</span>
        </div>
      </Show>

      <Show when={!loading() && entries().length === 0 && !error()}>
        <div class="empty-state p-12 text-center">
          <div class="flex flex-col items-center">
            <svg
              class="w-16 h-16 ui-muted mb-4"
              fill="none"
              stroke="currentColor"
              viewBox="0 0 24 24"
            >
              <path
                stroke-linecap="round"
                stroke-linejoin="round"
                stroke-width="1.5"
                d="M9 12h6m-6 4h6m2 5H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z"
              />
            </svg>
            <p class="ui-muted font-medium mb-1">{t("entriesList.empty")}</p>
            <p class="text-sm ui-muted">
              {t("entriesList.createFirst")}
            </p>
          </div>
        </div>
      </Show>

      <Show when={entries().length > 0}>
        <ul class="space-y-2">
          <For each={entries()}>
            {(entry) => (
              <EntryListItem
                entry={entry}
                isSelected={props.selectedEntryId === entry.id}
                onClick={() => handleEntryClick(entry.id)}
              />
            )}
          </For>
        </ul>
      </Show>
    </div>
  );
}
/* v8 ignore stop */

interface EntryListItemProps {
  entry: EntryRecord;
  isSelected: boolean;
  onClick: () => void;
}

function EntryListItem(props: EntryListItemProps) {
  const propertyEntries = () =>
    Object.entries(props.entry.properties ?? {}).slice(0, 3);

  /* v8 ignore start */
  return (
    <li data-testid="entry-item">
      <button
        type="button"
        class={`entry-item-button ui-card ui-card-interactive ui-card-hover w-full text-left cursor-pointer transition-all duration-200 ${
          props.isSelected ? "ui-card-selected" : ""
        }`}
        onClick={props.onClick}
        aria-pressed={props.isSelected}
      >
        <div class="flex justify-between items-start mb-2">
          <h3 class="font-semibold truncate flex-1 pr-2">
            {entryDisplayLabel(props.entry)}
          </h3>
          <Show when={props.entry.form}>
            <span class="ui-pill text-xs whitespace-nowrap">
              {props.entry.form}
            </span>
          </Show>
        </div>

        <Show when={propertyEntries().length > 0}>
          <div class="mt-2 text-sm ui-muted space-y-1">
            <For each={propertyEntries()}>
              {([key, value]) => (
                <div class="flex items-baseline">
                  <span class="ui-label mr-2 text-xs uppercase tracking-wide">
                    {key}:
                  </span>
                  <span class="truncate">
                    {formatValueForDisplay(value)}
                  </span>
                </div>
              )}
            </For>
          </div>
        </Show>

        <div class="mt-3 text-xs ui-muted flex items-center">
          <svg
            class="w-3 h-3 mr-1 opacity-50"
            fill="none"
            stroke="currentColor"
            viewBox="0 0 24 24"
          >
            <path
              stroke-linecap="round"
              stroke-linejoin="round"
              stroke-width="2"
              d="M12 8v4l3 3m6-3a9 9 0 11-18 0 9 9 0 0118 0z"
            />
          </svg>
          <span>
            {t("entriesList.updated", {
              date: formatDateLabel(props.entry.updated_at),
            })}
          </span>
        </div>
      </button>
    </li>
  );
}
/* v8 ignore stop */
