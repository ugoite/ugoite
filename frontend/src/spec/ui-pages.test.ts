/* @vitest-environment node */
import { describe, expect, it } from "vitest";
import { readdirSync, readFileSync, statSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { parse, parseAllDocuments } from "yaml";
import { settingsSections } from "../lib/settings-sections";

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
const routesDir = path.join(repoRoot, "frontend/src/routes/spaces/[space_id]");

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

const collectRouteFiles = (dir: string): string[] => {
  const files: string[] = [];
  for (const entry of readdirSync(dir)) {
    const fullPath = path.join(dir, entry);
    if (statSync(fullPath).isDirectory()) {
      files.push(...collectRouteFiles(fullPath));
    } else if (entry.endsWith(".tsx") && !entry.endsWith(".test.tsx")) {
      files.push(fullPath);
    }
  }
  return files;
};

const placeholderStubPatterns = [
  /\bis not yet available\b/i,
  /\bcoming soon\b/i,
  /\bnot implemented\b/i,
  /\bplaceholder (?:screen|ui|route)\b/i,
];

const segmentFromFile = (segment: string) => {
  if (segment === "index") return "";
  if (segment.startsWith("[") && segment.endsWith("]")) {
    return `{${segment.slice(1, -1)}}`;
  }
  return segment;
};

const routeFromFilePath = (filePath: string) => {
  const relative = path.relative(routesDir, filePath).replace(/\\/g, "/");
  const segments = relative.replace(/\.tsx$/, "").split("/")
    .map(segmentFromFile).filter(Boolean);
  return `/spaces/{space_id}${segments.length ? `/${segments.join("/")}` : ""}`;
};

const loadRoutes = () =>
  collectRouteFiles(routesDir).map((filePath) => ({
    filePath,
    route: routeFromFilePath(filePath),
    source: readFileSync(filePath, "utf8"),
  }));

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

  it("REQ-FE-040: validates Space Settings implementation references", () => {
    const page = loadPages().find(({ spec }) =>
      spec.page?.id === "space-settings"
    );
    expect(page).toBeTruthy();
    const panel = page?.spec.components?.body?.find(({ id }) =>
      id === "settings-panel"
    );
    expect(panel).toBeTruthy();

    const navigation = panel?.navigation as {
      section_source: string;
      implementation: string;
      tests: Array<{ path: string; selector: string }>;
    };
    const settingsPanel = panel as Record<string, unknown>;
    expect(
      statSync(path.join(repoRoot, String(settingsPanel.implementation)))
        .isFile(),
    )
      .toBe(true);
    expect(statSync(path.join(repoRoot, navigation.section_source)).isFile())
      .toBe(true);
    expect(statSync(path.join(repoRoot, navigation.implementation)).isFile())
      .toBe(true);
    for (const test of navigation.tests) {
      const testSource = readFileSync(path.join(repoRoot, test.path), "utf8");
      expect(testSource, `navigation test selector: ${test.path}`).toContain(
        test.selector,
      );
    }

    const sections = panel?.sections as Array<Record<string, unknown>>;
    expect(sections.map(({ id }) => id)).toEqual(
      settingsSections.map(({ id }) => id),
    );
    for (const section of sections) {
      const implementation = section.implementation as string[];
      expect(implementation.length, `${String(section.id)} implementation`)
        .toBeGreaterThan(0);
      for (const sourcePath of implementation) {
        expect(
          statSync(path.join(repoRoot, sourcePath)).isFile(),
          `${String(section.id)} implementation is missing: ${sourcePath}`,
        ).toBe(true);
      }

      const tests = section.tests as Array<{ path: string; selector: string }>;
      expect(tests.length, `${String(section.id)} tests`).toBeGreaterThan(0);
      for (const test of tests) {
        const testSource = readFileSync(path.join(repoRoot, test.path), "utf8");
        expect(testSource, `${String(section.id)} test selector: ${test.path}`)
          .toContain(test.selector);
      }

      const contract = section.component_contract as
        | { id: string; reference: string }
        | undefined;
      if (contract) {
        const contractPath = resolveComponentReference(
          page!.filePath,
          contract.reference,
        );
        const componentSpec = parse(readFileSync(contractPath, "utf8")) as {
          components?: Array<Record<string, unknown>>;
        };
        const component = componentSpec.components?.find(({ id }) =>
          id === contract.id
        );
        expect(component, `${String(section.id)} component reference`)
          .toBeTruthy();
        expect(implementation).toContain(component?.implementation);
      }

      const targetPage = section.target_page;
      if (targetPage) {
        expect(
          loadPages().some(({ spec }) => spec.page?.id === targetPage),
          `${String(section.id)} target page: ${String(targetPage)}`,
        ).toBe(true);
      }
    }
  });

  it("REQ-FE-040: validates docs pages map to implemented routes", () => {
    const routes = new Set(loadRoutes().map(({ route }) => route));
    for (const { spec, filePath } of loadPages()) {
      expect(spec.page?.route, `${filePath} missing route`).toBeTruthy();
      expect(
        routes.has(String(spec.page?.route)),
        `${filePath} route not implemented: ${String(spec.page?.route)}`,
      ).toBe(true);
    }
  });

  it("REQ-FE-040: validates implemented routes are documented", () => {
    const documented = new Set(
      loadPages().map(({ spec }) => spec.page?.route).filter((
        route,
      ): route is string => Boolean(route)),
    );
    for (const { route } of loadRoutes()) {
      expect(
        documented.has(route),
        `missing docs/spec/ui/pages entry for route: ${route}`,
      )
        .toBe(true);
    }
  });

  it("REQ-FE-040: implemented page specs reject placeholder route content", () => {
    const routesByPath = new Map(
      loadRoutes().map((route) => [route.route, route]),
    );
    for (const { spec, filePath } of loadPages()) {
      if (spec.page?.implementation !== "implemented") continue;
      expect(spec.page.route, `${filePath} missing route`).toBeTruthy();
      const routeRecord = routesByPath.get(String(spec.page.route));
      expect(
        routeRecord,
        `${filePath} route not implemented: ${String(spec.page.route)}`,
      )
        .toBeTruthy();
      if (!routeRecord) continue;
      const matchedPattern = placeholderStubPatterns.find((pattern) =>
        pattern.test(routeRecord.source)
      );
      expect(
        matchedPattern,
        `${filePath} marks ${
          String(spec.page.route)
        } implemented but ${routeRecord.filePath} still contains placeholder copy`,
      ).toBeUndefined();
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
