import { createEffect, createMemo, createSignal, For, Show } from "solid-js";
import { IconButton } from "~/components/IconButton";
import { t } from "~/lib/i18n";
import type {
  SqlDocument,
  SqlDocumentNode,
  SqlDocumentNodeKind,
} from "~/lib/ugoite-client";

export type SqlDocumentStructureReplacement = {
  node_id: number;
  expected_kind: SqlDocumentNodeKind;
  replacement: string;
};

interface SqlDocumentStructureEditorProps {
  document: SqlDocument;
  disabled?: boolean;
  onReplaceNode: (replacement: SqlDocumentStructureReplacement) => void;
}

type SelectableNode = {
  node: SqlDocumentNode;
  fragment: string;
  children: SelectableNode[];
};

const decodeRange = (
  sourceBytes: Uint8Array,
  range: SqlDocumentNode["range"],
): string | undefined => {
  if (!range) return undefined;
  const { start, end } = range;
  if (
    !Number.isSafeInteger(start) || !Number.isSafeInteger(end) || start < 0 ||
    end < start || end > sourceBytes.byteLength
  ) return undefined;

  try {
    return new TextDecoder("utf-8", { fatal: true }).decode(
      sourceBytes.subarray(start, end),
    );
  } catch {
    // Rust ranges are UTF-8 byte offsets. A split code point is not selectable.
    return undefined;
  }
};

const buildTree = (document: SqlDocument): SelectableNode[] => {
  const sourceBytes = new TextEncoder().encode(document.source);
  const selectable: SelectableNode[] = [];
  const seenIds = new Set<number>();

  for (const node of document.nodes) {
    if (seenIds.has(node.id)) continue;
    seenIds.add(node.id);
    const fragment = decodeRange(sourceBytes, node.range);
    if (fragment === undefined) continue;
    selectable.push({ node, fragment, children: [] });
  }

  const byId = new Map(selectable.map((item) => [item.node.id, item]));
  const roots: SelectableNode[] = [];
  for (const item of selectable) {
    const parent = item.node.parent === null
      ? undefined
      : byId.get(item.node.parent);
    if (!parent || parent === item) {
      roots.push(item);
      continue;
    }

    // Fail closed on malformed cyclic ancestry rather than losing every node.
    let ancestor: SelectableNode | undefined = parent;
    const visited = new Set<number>([item.node.id]);
    let cyclic = false;
    while (ancestor) {
      if (visited.has(ancestor.node.id)) {
        cyclic = true;
        break;
      }
      visited.add(ancestor.node.id);
      ancestor = ancestor.node.parent === null
        ? undefined
        : byId.get(ancestor.node.parent);
    }
    if (cyclic) roots.push(item);
    else parent.children.push(item);
  }
  return roots;
};

const flattenTree = (nodes: SelectableNode[]): SelectableNode[] => {
  const result: SelectableNode[] = [];
  const visit = (items: SelectableNode[]) => {
    for (const item of items) {
      result.push(item);
      visit(item.children);
    }
  };
  visit(nodes);
  return result;
};

const nodeLabel = (kind: SqlDocumentNodeKind): string => {
  switch (kind) {
    case "query_statement":
    case "other_statement":
      return t("sqlPage.sql");
    case "query":
      return "SELECT";
    case "with_clause":
      return "WITH";
    case "common_table_expression":
      return t("sqlPage.structure.node.cte");
    case "set_operation":
    case "set_operation_operator":
      return "UNION";
    case "set_operation_branch":
      return t("sqlPage.structure.node.branch");
    case "select":
      return "SELECT";
    case "select_item":
      return t("sqlPage.structure.columns");
    case "from_clause":
      return "FROM";
    case "relation":
    case "table_factor":
      return t("sqlPage.structure.node.relation");
    case "join":
      return t("sqlPage.structure.join");
    case "where_clause":
    case "predicate_expression":
      return t("sqlPage.structure.filter");
    case "group_by_clause":
      return t("sqlPage.structure.group_by");
    case "having_clause":
      return "HAVING";
    case "order_by_clause":
      return t("sqlPage.structure.sort");
    case "row_limit_clause":
      return "LIMIT";
    case "distinct_clause":
      return t("sqlPage.structure.distinct");
    case "case_expression":
      return "CASE";
    case "cast_expression":
      return "CAST";
    case "identifier_expression":
    case "compound_identifier_expression":
      return t("sqlPage.structure.node.identifier");
    case "literal_expression":
    case "value":
      return t("sqlPage.structure.node.value");
    case "binary_expression":
    case "unary_expression":
    case "nested_expression":
    case "other_expression":
      return t("sqlPage.structure.node.expression");
    case "function_expression":
      return t("sqlPage.structure.node.function");
    case "subquery_expression":
      return t("sqlPage.structure.node.subquery");
    default:
      // SQL AST node names without a user-facing translation stay compact.
      return t("sqlPage.sql");
  }
};

function renderNodes(
  nodes: SelectableNode[],
  selectedId: () => number | null,
  onSelect: (item: SelectableNode) => void,
) {
  return (
    <ul class="sql-document-structure-list">
      <For each={nodes}>
        {(item) => (
          <li>
            <button
              type="button"
              class="ui-button ui-button-secondary sql-document-structure-node"
              style={{ "min-height": "44px", "min-width": "44px" }}
              aria-label={nodeLabel(item.node.kind)}
              aria-pressed={selectedId() === item.node.id}
              onClick={() => onSelect(item)}
            >
              {nodeLabel(item.node.kind)}
            </button>
            <Show when={item.children.length > 0}>
              {renderNodes(item.children, selectedId, onSelect)}
            </Show>
          </li>
        )}
      </For>
    </ul>
  );
}

export function SqlDocumentStructureEditor(
  props: SqlDocumentStructureEditorProps,
) {
  const tree = createMemo(() => buildTree(props.document));
  const [selectedId, setSelectedId] = createSignal<number | null>(
    flattenTree(tree())[0]?.node.id ?? null,
  );
  const [replacement, setReplacement] = createSignal("");
  const selected = createMemo(() =>
    flattenTree(tree()).find((item) => item.node.id === selectedId())
  );

  createEffect(() => {
    const current = selected();
    if (current) {
      setReplacement(current.fragment);
    } else {
      const first = flattenTree(tree())[0];
      setSelectedId(first?.node.id ?? null);
      setReplacement(first?.fragment ?? "");
    }
  });

  const selectNode = (item: SelectableNode) => {
    setSelectedId(item.node.id);
    setReplacement(item.fragment);
  };

  const apply = () => {
    const item = selected();
    if (!item || props.disabled || replacement() === item.fragment) return;
    props.onReplaceNode({
      node_id: item.node.id,
      expected_kind: item.node.kind,
      replacement: replacement(),
    });
  };

  return (
    <section class="sql-document-structure-editor">
      <Show when={tree().length > 0}>
        <nav aria-label={t("sqlPage.structureActions")}>
          {renderNodes(tree(), selectedId, selectNode)}
        </nav>
      </Show>
      <Show when={selected()} keyed>
        {(item) => (
          <div class="sql-document-structure-selection">
            <pre
              class="sql-document-structure-fragment"
              aria-label={t("sqlPage.structure.currentSql")}
            >
              {item.fragment}
            </pre>
            <textarea
              class="sql-document-structure-replacement"
              style={{ "min-height": "44px", "min-width": "44px" }}
              aria-label={t("sqlPage.structure.proposedSql")}
              value={replacement()}
              disabled={props.disabled}
              onInput={(event) => setReplacement(event.currentTarget.value)}
            />
            <IconButton
              icon="save"
              label={t("sqlPage.structure.apply")}
              disabled={props.disabled || replacement() === item.fragment}
              onClick={apply}
            />
          </div>
        )}
      </Show>
    </section>
  );
}
