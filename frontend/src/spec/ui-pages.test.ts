/* @vitest-environment node */
import { describe, expect, it } from "vitest";
import { readdirSync, readFileSync, statSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { parse, parseAllDocuments } from "yaml";

type PageSpec = {
  page?: {
    id?: string;
    title?: string;
    route?: string;
    implementation?: string;
    contract_status?: string;
    visible_heading?: string;
  };
  components?: {
    shared?: Array<Record<string, unknown>>;
    body?: Array<Record<string, unknown>>;
  };
};

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(__dirname, "../../..");
const pagesDir = path.join(repoRoot, "docs/spec/ui/pages");
const componentsDir = path.join(repoRoot, "docs/spec/ui/components");

const allowedComponentTypes = new Set([
  "tab-list",
  "tab-bar",
  "floating-icon-button",
  "heading",
  "icon-button",
  "search-bar",
  "filter-button",
  "dialog",
  "list",
  "grid-list",
  "text-input",
  "sql-editor",
  "button",
  "link",
  "form",
  "markdown-editor",
  "toolbar",
  "sort-button",
  "data-grid",
  "csv-download-button",
  "settings-panel",
  "segmented-control",
  "search-panel",
  "structured-search-form",
  "entry-card-grid",
  "sidebar",
  "persistent-workspace-navigation",
  "page-header",
  "two-column-workspace",
  "searchable-master-list",
  "redirect",
  "query-results",
  "query-pagination",
  "select",
  "typed-variable-list",
  "history",
  "composition-studio",
  "shared-sql-workspace",
]);

const allowedSharedComponentTypes = new Set([
  "account-entry",
  "alert",
  "alternate-authentication-actions",
  "bottom-navigation",
  "device-approval",
  "editable-data-grid",
  "form",
  "inline-guidance",
  "list",
  "native-query-result-table",
  "page-header",
  "query-pagination",
  "read-only-audit-event-table",
  "recoverable-page-error",
  "resource-access-policy-editor",
  "searchable-entry-picker",
  "select",
  "server-query-results",
  "settings-panel",
  "sidebar",
  "single-task-surface",
  "tab-bar",
  "toolbar",
  "top-bar",
  "typed-form-field",
  "utility-panel",
]);

const collectYamlFiles = (dir: string): string[] => {
  const entries = readdirSync(dir);
  const files: string[] = [];
  for (const entry of entries) {
    const fullPath = path.join(dir, entry);
    const stats = statSync(fullPath);
    if (stats.isDirectory()) {
      files.push(...collectYamlFiles(fullPath));
    } else if (entry.endsWith(".yaml")) {
      files.push(fullPath);
    }
  }
  return files;
};

const loadPages = () =>
  collectYamlFiles(pagesDir).map((filePath) => ({
    filePath,
    spec: parse(readFileSync(filePath, "utf8")) as PageSpec,
  }));

const collectComponentFileReferences = (
  value: unknown,
  references: Array<{ kind: "path" | "component"; value: string }>,
) => {
  if (Array.isArray(value)) {
    for (const item of value) collectComponentFileReferences(item, references);
    return;
  }
  if (!value || typeof value !== "object") return;
  for (const [key, entry] of Object.entries(value)) {
    if (
      typeof entry === "string" &&
      entry.startsWith("frontend/")
    ) {
      references.push({ kind: "path", value: entry });
    } else if (
      typeof entry === "string" &&
      (/reference/i.test(key) || key === "contract") &&
      entry.endsWith(".yaml")
    ) {
      references.push({ kind: "component", value: entry });
    } else {
      collectComponentFileReferences(entry, references);
    }
  }
};

const loadSharedComponentSpecs = () => {
  const specs: Array<{ filePath: string; spec: Record<string, unknown> }> = [];
  for (const filePath of collectYamlFiles(componentsDir)) {
    const documents = parseAllDocuments(readFileSync(filePath, "utf8"));
    const document = documents[0];
    expect(document.errors, filePath).toHaveLength(0);
    const parsed = document.toJSON() as Record<string, unknown> | null;
    if (!parsed || typeof parsed !== "object") continue;
    if (
      parsed.component_group || parsed.component ||
      Array.isArray(parsed.components)
    ) {
      specs.push({ filePath, spec: parsed });
    }
  }
  return specs;
};

const validateUiYamlReference = (sourcePath: string, reference: string) => {
  const resolved = reference.startsWith("docs/")
    ? path.resolve(repoRoot, reference)
    : path.resolve(path.dirname(sourcePath), reference);
  expect(
    resolved.startsWith(`${componentsDir}${path.sep}`) ||
      resolved.startsWith(`${pagesDir}${path.sep}`),
    `${sourcePath} UI reference escapes the registry: ${reference}`,
  ).toBe(true);
  expect(statSync(resolved).isFile(), `${sourcePath} missing ${reference}`)
    .toBe(true);
  const document = parseAllDocuments(readFileSync(resolved, "utf8"))[0];
  expect(document.errors, resolved).toHaveLength(0);
  expect(document.toJSON(), resolved).toBeTruthy();
  // Some shared UI component contracts use YAML front matter and Markdown
  // sections instead of a structured component list.
};

const collectTargets = (value: unknown, targets: string[]) => {
  if (Array.isArray(value)) {
    for (const item of value) collectTargets(item, targets);
  } else if (value && typeof value === "object") {
    for (const [key, entry] of Object.entries(value)) {
      if (key.startsWith("target_page") && typeof entry === "string") {
        targets.push(entry);
      } else {
        collectTargets(entry, targets);
      }
    }
  }
};

const resolveComponentReference = (pagePath: string, reference: string) => {
  const resolved = path.resolve(path.dirname(pagePath), reference);
  expect(resolved.startsWith(`${componentsDir}${path.sep}`)).toBe(true);
  expect(statSync(resolved).isFile()).toBe(true);
  return resolved;
};

describe("UI spec YAML registry", () => {
  it("REQ-FE-040: loads UI page specs", () => {
    const pages = loadPages();
    expect(pages.length).toBeGreaterThan(0);

    for (const { spec, filePath } of pages) {
      expect(spec.page?.id, filePath).toBeTruthy();
      expect(spec.page?.title, filePath).toBeTruthy();
      expect(spec.page?.route, filePath).toBeTruthy();
      expect(spec.page?.implementation, filePath).toBeTruthy();
      expect(["unimplemented", "implemented", "in-progress"]).toContain(
        spec.page?.implementation,
      );
    }
  });

  it("REQ-FE-040: validates component types", () => {
    for (const { spec, filePath } of loadPages()) {
      const components = [
        ...(spec.components?.shared ?? []),
        ...(spec.components?.body ?? []),
      ];
      for (const component of components) {
        expect(component.type, `${filePath} has component missing type`)
          .toBeTruthy();
        expect(
          allowedComponentTypes.has(String(component.type)),
          `${filePath} uses unsupported component type: ${
            String(component.type)
          }`,
        ).toBe(true);
      }
    }
  });

  it("REQ-FE-040: validates shared component structure and references", () => {
    for (const { filePath, spec } of loadSharedComponentSpecs()) {
      const group = spec.component_group as Record<string, unknown> | undefined;
      if (group) {
        expect(typeof group.id, `${filePath} component_group.id`).toBe(
          "string",
        );
        expect(String(group.id).length, `${filePath} component_group.id`)
          .toBeGreaterThan(0);
        expect(Array.isArray(spec.components), `${filePath} components`)
          .toBe(true);
      }

      const hasComponentList = Array.isArray(spec.components);
      const components = hasComponentList
        ? spec.components as Array<Record<string, unknown>>
        : spec.component && typeof spec.component === "object"
        ? [spec.component as Record<string, unknown>]
        : [];
      const componentIds = new Set<string>();
      for (const component of components) {
        expect(typeof component.id, `${filePath} component.id`).toBe("string");
        const id = String(component.id);
        expect(id.length, `${filePath} component.id`).toBeGreaterThan(0);
        expect(componentIds.has(id), `${filePath} repeats component id: ${id}`)
          .toBe(false);
        componentIds.add(id);

        if (component.type !== undefined) {
          expect(typeof component.type, `${filePath} ${id} type`).toBe(
            "string",
          );
          expect(
            allowedSharedComponentTypes.has(String(component.type)),
            `${filePath} ${id} uses unsupported component type: ${
              String(component.type)
            }`,
          ).toBe(true);
        } else {
          expect(hasComponentList, `${filePath} ${id} missing type`).toBe(
            false,
          );
        }
      }

      const references: Array<{
        kind: "path" | "component";
        value: string;
      }> = [];
      collectComponentFileReferences(spec, references);
      for (const reference of references) {
        if (reference.kind === "path") {
          expect(
            statSync(path.join(repoRoot, reference.value)).isFile(),
            `${filePath} references missing file: ${reference.value}`,
          ).toBe(true);
          continue;
        }
        validateUiYamlReference(filePath, reference.value);
      }
    }

    for (const { filePath, spec } of loadPages()) {
      const references: Array<{
        kind: "path" | "component";
        value: string;
      }> = [];
      collectComponentFileReferences(spec, references);
      for (const reference of references) {
        if (reference.kind === "component") {
          validateUiYamlReference(filePath, reference.value);
        }
      }
    }
  });

  it("REQ-FE-040: validates page links", () => {
    const pages = loadPages();
    const pageIds = new Set(
      pages.map(({ spec }) => spec.page?.id).filter((id): id is string =>
        Boolean(id)
      ),
    );

    for (const { spec, filePath } of pages) {
      const targets: string[] = [];
      collectTargets(spec, targets);
      for (const target of targets) {
        expect(
          pageIds.has(target),
          `${filePath} references missing page: ${target}`,
        )
          .toBe(true);
      }
    }
  });

  it("REQ-UX-RECOVERY-001: validates the owner recovery index reference", () => {
    const uiIndex = readFileSync(
      path.join(repoRoot, "docs/spec/ui/index.md"),
      "utf8",
    );
    expect(uiIndex).toContain("`components/space-access-recovery.yaml`");
    expect(
      statSync(path.join(componentsDir, "space-access-recovery.yaml")).isFile(),
    ).toBe(true);
  });

  it("resolves the SQL results pagination reference", () => {
    const sqlResults = loadPages().find(({ spec }) =>
      spec.page?.id === "space-sql-results"
    );
    const pagination = sqlResults?.spec.components?.body?.find(({ id }) =>
      id === "sql-results-pagination"
    );
    expect(pagination).toBeTruthy();
    const reference = String(pagination?.reference);
    const contractPath = resolveComponentReference(
      sqlResults!.filePath,
      reference,
    );
    const shared = parse(readFileSync(contractPath, "utf8")) as {
      components?: Array<Record<string, unknown>>;
    };
    expect(shared.components?.some(({ id }) => id === "result-pagination"))
      .toBe(true);
  });

  it("resolves the Form Entries component reference", () => {
    const entries = loadPages().find(({ spec }) =>
      spec.page?.id === "space-form-entries"
    );
    const entryList = entries?.spec.components?.body?.find(({ id }) =>
      id === "form-entry-list"
    );
    expect(entryList).toBeTruthy();
    const componentPath = resolveComponentReference(
      entries!.filePath,
      String(entryList?.reference),
    );
    const entryBrowser = parse(readFileSync(componentPath, "utf8")) as {
      components?: Array<Record<string, unknown>>;
    };
    expect(entryBrowser.components?.some(({ id }) => id === "form-entry-table"))
      .toBe(true);
  });

  it("validates indexed component reference paths", () => {
    const resultTablePath = path.join(componentsDir, "result-table.yaml");
    const entryBrowserPath = path.join(componentsDir, "entry-browser.yaml");
    const formTablePath = path.join(componentsDir, "form-table.yaml");
    const uiIndex = readFileSync(
      path.join(repoRoot, "docs/spec/ui/index.md"),
      "utf8",
    );
    for (
      const componentPath of [resultTablePath, entryBrowserPath, formTablePath]
    ) {
      expect(statSync(componentPath).isFile()).toBe(true);
      const relativePath = path.relative(componentsDir, componentPath);
      expect(uiIndex).toContain(`\`components/${relativePath}\``);
    }

    const entryBrowser = parse(readFileSync(entryBrowserPath, "utf8")) as {
      components?: Array<Record<string, unknown>>;
    };
    expect(entryBrowser.components?.some(({ id }) => id === "form-entry-table"))
      .toBe(true);
  });

  it("resolves result-table references from pages and shared components", () => {
    const resultTablePath = path.join(componentsDir, "result-table.yaml");
    const resultTable = parse(readFileSync(resultTablePath, "utf8")) as {
      components?: Array<Record<string, unknown>>;
    };
    expect(
      resultTable.components?.some(({ id }) => id === "result-table-viewport"),
    )
      .toBe(true);

    const entryBrowser = parse(readFileSync(
      path.join(componentsDir, "entry-browser.yaml"),
      "utf8",
    )) as { components?: Array<Record<string, unknown>> };
    const entryTable = entryBrowser.components?.find(({ id }) =>
      id === "form-entry-table"
    );
    expect(entryTable).toBeTruthy();
    expect(
      path.resolve(
        componentsDir,
        String(entryTable?.result_presentation_reference),
      ),
    )
      .toBe(resultTablePath);

    const pages = loadPages();
    const sqlResults = pages.find(({ spec }) =>
      spec.page?.id === "space-sql-results"
    );
    const sqlTable = sqlResults?.spec.components?.body?.find(({ id }) =>
      id === "sql-results-table"
    );
    expect(
      path.resolve(
        path.dirname(sqlResults!.filePath),
        String(sqlTable?.result_presentation_reference),
      ),
    )
      .toBe(resultTablePath);

    const revision = pages.find(({ spec }) =>
      spec.page?.id === "space-composition-revision"
    );
    const composition = revision?.spec.components?.body?.find(({ id }) =>
      id === "composition-components"
    );
    const tablePresentation = composition?.table_presentation as
      | { result_presentation_reference?: string }
      | undefined;
    expect(
      path.resolve(
        path.dirname(revision!.filePath),
        String(tablePresentation?.result_presentation_reference),
      ),
    )
      .toBe(resultTablePath);
  });

  it("resolves Search results to the shared EntryBrowser component", () => {
    const search = loadPages().find(({ spec }) =>
      spec.page?.id === "space-search"
    );
    const results = search?.spec.components?.body?.find(({ id }) =>
      id === "keyword-result-list"
    );
    expect(results).toBeTruthy();
    const componentPath = resolveComponentReference(
      search!.filePath,
      String(results?.reference),
    );
    const entryBrowser = parse(readFileSync(componentPath, "utf8")) as {
      components?: Array<Record<string, unknown>>;
    };
    expect(entryBrowser.components?.some(({ id }) => id === "form-entry-table"))
      .toBe(true);
  });
});
