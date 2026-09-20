import { createEffect, createSignal, Show } from "solid-js";
import { createResource } from "~/lib/recoverable-resource";
import { searchApi } from "~/lib/ugoite-client";
import { t } from "~/lib/i18n";
import { SearchableSelect } from "~/components/fields/SearchableSelect";
import {
  buildRowReferenceOptions,
  type RowReferenceOption,
  rowReferenceSuggestionLimit,
} from "~/components/fields/row-reference";

export interface RowReferenceSelectProps {
  spaceId: string;
  targetForm: string;
  /** Stable entry id. Display labels are the deterministic entry ID. */
  value: string;
  onChange: (id: string) => void;
  fieldId: string;
  invalid?: boolean;
  describedBy?: string;
  limit?: number;
  /** True while the search text names no saved entry (create-dialog guard). */
  onPendingChange?: (pending: boolean) => void;
}

/**
 * Shared row-reference control for create and edit.
 *
 * Shows deterministic entry IDs, saves the stable entry id, and scopes every
 * lookup to the exact target Form. Keyboard arrows/Enter/Escape, clear,
 * and loading/error/empty states come from the generic SearchableSelect.
 */
export function RowReferenceSelect(props: RowReferenceSelectProps) {
  const targetForm = () => props.targetForm.trim();
  const [query, setQuery] = createSignal(props.value);
  const [selected, setSelected] = createSignal<RowReferenceOption | null>(
    props.value
      ? { id: props.value, title: props.value, label: props.value }
      : null,
  );
  /** Last confirmed selection. Escape reverts an in-progress search to it. */
  const [confirmed, setConfirmed] = createSignal<RowReferenceOption | null>(
    selected(),
  );

  // Emission guard (plain variable, never reactive): parent state echoes our
  // own onChange calls, but effect flushes can run mid-handler and observe
  // the stale parent value first. Adoption below keys on an actual parent
  // change (`lastParent`), so any interleaving of child writes and parent
  // echoes is safe: our own in-flight emission never resets the query.
  let pendingEmit: string | null = null;
  let lastParent = props.value;
  const emit = (id: string) => {
    pendingEmit = id;
    props.onChange(id);
  };

  createEffect(() => {
    const value = props.value;
    // Adopt external parent sets only. Child-signal writes (typing,
    // selecting, clearing) re-run this effect through the `selected()`
    // read below, but the unchanged parent value returns here before any
    // reset can clobber in-progress work.
    if (value === lastParent) return;
    lastParent = value;
    if (value === pendingEmit) {
      // Parent echoed our emission (or already held it). Adopt the echo as
      // the sync point and stop guarding.
      pendingEmit = null;
      return;
    }
    if (value === selected()?.id) return;
    if (pendingEmit !== null) return;
    setQuery(value);
    const next = value ? { id: value, title: value, label: value } : null;
    setSelected(next);
    setConfirmed(next);
  });

  const [options] = createResource(
    () => ({
      spaceId: props.spaceId.trim(),
      targetForm: targetForm(),
      query: query(),
    }),
    async ({ spaceId, targetForm: form, query: searchQuery }) => {
      if (!spaceId || !form) return [] as RowReferenceOption[];
      const entries = await searchApi.rowReferenceOptions(
        spaceId,
        form,
        searchQuery,
        props.limit ?? rowReferenceSuggestionLimit,
      );
      return buildRowReferenceOptions(entries);
    },
    { initialValue: [] as RowReferenceOption[] },
  );

  // Resolve the display label once options arrive. The saved value never
  // changes here.
  createEffect(() => {
    const current = selected();
    if (!current) return;
    const match = options().find((option) => option.id === current.id);
    if (match && match.title !== current.title) {
      setSelected(match);
      if (confirmed()?.id === match.id) setConfirmed(match);
      if (query() === current.title || query() === current.id) {
        setQuery(match.title);
      }
    }
  });

  const pending = () => query().trim() !== "" && !props.value.trim();
  createEffect(() => {
    props.onPendingChange?.(pending());
  });

  const handleQueryInput = (value: string) => {
    setQuery(value);
    setSelected(null);
    if (props.value) emit("");
    props.onPendingChange?.(value.trim() !== "");
  };

  const handleSelect = (option: RowReferenceOption) => {
    setSelected(option);
    setConfirmed(option);
    setQuery(option.title);
    emit(option.id);
    props.onPendingChange?.(false);
  };

  const handleClear = () => {
    setSelected(null);
    setConfirmed(null);
    setQuery("");
    emit("");
    props.onPendingChange?.(false);
  };

  /** Escape cancels the in-progress search and reverts to the confirmed pick. */
  const handleEscape = () => {
    const current = confirmed();
    if (!current) {
      handleClear();
      return;
    }
    setSelected(current);
    setQuery(current.title);
    if (props.value !== current.id) emit(current.id);
    props.onPendingChange?.(false);
  };

  return (
    <Show
      when={targetForm()}
      fallback={
        <input
          id={props.fieldId}
          type="text"
          class="ui-input"
          value={props.value}
          aria-invalid={props.invalid ? "true" : undefined}
          aria-describedby={props.describedBy}
          onInput={(event) => props.onChange(event.currentTarget.value)}
        />
      }
    >
      <SearchableSelect
        id={props.fieldId}
        query={query()}
        onQueryInput={handleQueryInput}
        options={options()}
        selected={selected()}
        onSelect={handleSelect}
        onClear={handleClear}
        loading={options.loading}
        error={options.error}
        invalid={props.invalid}
        describedBy={props.describedBy}
        onEscape={handleEscape}
        labels={{
          placeholder: t(
            "createDialog.entry.rowReference.searchPlaceholder",
            { form: targetForm() },
          ),
          help: t("createDialog.entry.rowReference.help", {
            form: targetForm(),
          }),
          selectedHeading: t("createDialog.entry.rowReference.selected"),
          clear: t("createDialog.entry.rowReference.clear"),
          loading: t("createDialog.entry.rowReference.loading", {
            form: targetForm(),
          }),
          loadError: t("createDialog.entry.rowReference.loadError", {
            form: targetForm(),
          }),
          noMatches: t("createDialog.entry.rowReference.noMatches", {
            form: targetForm(),
          }),
        }}
      />
    </Show>
  );
}
