import { Show } from "solid-js";

export interface ResultPaginationProps {
  canPrevious: boolean;
  canNext: boolean;
  busy: boolean;
  previousLabel: string;
  nextLabel: string;
  ariaLabel: string;
  onPrevious: () => void;
  onNext: () => void;
}

/**
 * Shared Previous/Next vocabulary for already-fetched result pages.
 * Knows nothing about rows, cursors, continuations, queries, errors,
 * selection, retry, or source kinds: the owning controller or query handle
 * decides availability and performs the page change. A single page renders
 * no navigation at all.
 */
export function ResultPagination(props: ResultPaginationProps) {
  return (
    <Show when={props.canPrevious || props.canNext}>
      <nav class="result-pagination" aria-label={props.ariaLabel}>
        <button
          type="button"
          class="ui-button ui-button-secondary result-pagination-button"
          disabled={!props.canPrevious || props.busy}
          onClick={() => props.onPrevious()}
        >
          {props.previousLabel}
        </button>
        <button
          type="button"
          class="ui-button ui-button-secondary result-pagination-button"
          disabled={!props.canNext || props.busy}
          onClick={() => props.onNext()}
        >
          {props.nextLabel}
        </button>
      </nav>
    </Show>
  );
}
