/* @vitest-environment node */
import { describe, expect, it } from "vitest";
import { readdirSync, readFileSync, statSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { parse } from "yaml";
import { settingsSections } from "../lib/settings-sections";

type PageSpec = {
  page?: {
    id?: string;
    title?: string;
    route?: string;
    implementation?: string;
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
  "select",
]);

const collectYamlFiles = (dir: string): string[] => {
  const entries = readdirSync(dir);
  const files: string[] = [];
  for (const entry of entries) {
    const fullPath = path.join(dir, entry);
    const stats = statSync(fullPath);
    if (stats.isDirectory()) {
      files.push(...collectYamlFiles(fullPath));
      continue;
    }
    if (entry.endsWith(".yaml")) {
      files.push(fullPath);
    }
  }
  return files;
};

const loadPages = () => {
  const files = collectYamlFiles(pagesDir);
  const pages = files.map((filePath) => {
    const contents = readFileSync(filePath, "utf8");
    return {
      filePath,
      spec: parse(contents) as PageSpec,
    };
  });
  return pages;
};

const collectRouteFiles = (dir: string): string[] => {
  const entries = readdirSync(dir);
  const files: string[] = [];
  for (const entry of entries) {
    const fullPath = path.join(dir, entry);
    const stats = statSync(fullPath);
    if (stats.isDirectory()) {
      files.push(...collectRouteFiles(fullPath));
      continue;
    }
    if (entry.endsWith(".tsx") && !entry.endsWith(".test.tsx")) {
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
  const withoutExt = relative.replace(/\.tsx$/, "");
  const segments = withoutExt.split("/").map(segmentFromFile).filter(Boolean);
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
    for (const item of value) {
      collectTargets(item, targets);
    }
    return;
  }
  if (!value || typeof value !== "object") {
    return;
  }
  for (const [key, entry] of Object.entries(value)) {
    if (key.startsWith("target_page") && typeof entry === "string") {
      targets.push(entry);
      continue;
    }
    collectTargets(entry, targets);
  }
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
    const pages = loadPages();
    for (const { spec, filePath } of pages) {
      const allComponents = [
        ...(spec.components?.shared ?? []),
        ...(spec.components?.body ?? []),
      ];
      for (const component of allComponents) {
        const type = component.type;
        expect(type, `${filePath} has component missing type`).toBeTruthy();
        expect(
          allowedComponentTypes.has(String(type)),
          `${filePath} uses unsupported component type: ${String(type)}`,
        ).toBe(true);
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
        ).toBe(true);
      }
    }
  });

  it("REQ-FE-040: connects Space Settings sections to implementation and test evidence", () => {
    const page = loadPages().find(({ spec }) =>
      spec.page?.id === "space-settings"
    );
    const panel = page?.spec.components?.body?.find(({ id }) =>
      id === "settings-panel"
    );
    expect(panel).toBeTruthy();
    expect(
      statSync(path.join(repoRoot, String(panel?.implementation))).isFile(),
    ).toBe(true);
    expect(panel?.navigation).toMatchObject({
      component: "RowList",
      section_source: "frontend/src/lib/settings-sections.ts",
      section_state: "query.section",
      layout: "flat-category-list",
    });
    const navigation = panel?.navigation as {
      implementation: string;
      tests: Array<{ path: string; selector: string }>;
    };
    expect(
      statSync(path.join(repoRoot, navigation.implementation)).isFile(),
    ).toBe(true);
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
      expect(typeof section.title).toBe("string");
      const implementation = section.implementation as string[];
      expect(implementation.length, `${String(section.id)} implementation`)
        .toBeGreaterThan(0);
      for (const sourcePath of implementation) {
        expect(
          statSync(path.join(repoRoot, sourcePath)).isFile(),
          `${String(section.id)} implementation is missing: ${sourcePath}`,
        ).toBe(true);
      }

      const tests = section.tests as Array<{
        path: string;
        selector: string;
      }>;
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
        const contractPath = path.resolve(
          path.dirname(page!.filePath),
          contract.reference,
        );
        expect(statSync(contractPath).isFile()).toBe(true);
        const componentSpec = parse(readFileSync(contractPath, "utf8")) as {
          components?: Array<Record<string, unknown>>;
        };
        const component = componentSpec.components?.find(({ id }) =>
          id === contract.id
        );
        expect(component).toBeTruthy();
        expect(implementation).toContain(component?.implementation);
        expect(component?.variants).toMatchObject({
          general: { controls: ["space-name", "save"] },
          storage: {
            controls: ["storage-uri", "test-connection", "save"],
            advanced_details: ["endpoint", "configuration-status"],
            save_payload: "storage-configuration-only",
          },
        });
      }

      const targetPage = section.target_page;
      if (targetPage) {
        const target = loadPages().find(({ spec }) =>
          spec.page?.id === targetPage
        );
        expect(target, `${String(section.id)} target page: ${targetPage}`)
          .toBeTruthy();
      }
    }
  });

  it("REQ-FE-040: validates docs pages map to implemented routes", () => {
    const pages = loadPages();
    const routes = new Set(loadRoutes().map(({ route }) => route));
    for (const { spec, filePath } of pages) {
      const route = spec.page?.route;
      expect(route, `${filePath} missing route`).toBeTruthy();
      expect(
        routes.has(String(route)),
        `${filePath} route not implemented: ${String(route)}`,
      ).toBe(
        true,
      );
    }
  });

  it("REQ-FE-040: validates implemented routes are documented", () => {
    const pages = loadPages();
    const documented = new Set(
      pages.map(({ spec }) => spec.page?.route).filter((
        route,
      ): route is string => Boolean(route)),
    );
    const routes = new Set(loadRoutes().map(({ route }) => route));
    for (const route of routes) {
      expect(
        documented.has(route),
        `missing docs/spec/ui/pages entry for route: ${route}`,
      ).toBe(
        true,
      );
    }
  });

  it("REQ-FE-040: implemented page specs reject placeholder route content", () => {
    const pages = loadPages();
    const routesByPath = new Map(
      loadRoutes().map((route) => [route.route, route]),
    );
    for (const { spec, filePath } of pages) {
      if (spec.page?.implementation !== "implemented") {
        continue;
      }
      const route = spec.page?.route;
      expect(route, `${filePath} missing route`).toBeTruthy();
      const routeRecord = routesByPath.get(String(route));
      expect(routeRecord, `${filePath} route not implemented: ${String(route)}`)
        .toBeTruthy();
      if (!routeRecord) {
        continue;
      }
      const matchedPattern = placeholderStubPatterns.find((pattern) =>
        pattern.test(routeRecord.source)
      );
      expect(
        matchedPattern,
        `${filePath} marks ${
          String(route)
        } implemented but ${routeRecord.filePath} still contains placeholder copy`,
      ).toBeUndefined();
    }
  });

  it("REQ-FE-040: validates shared space chrome", () => {
    const shellPath = path.join(
      repoRoot,
      "docs/spec/ui/components/space-shell.yaml",
    );
    const shell = parse(readFileSync(shellPath, "utf8")) as {
      components?: Array<Record<string, unknown>>;
    };
    const components = shell.components ?? [];
    const sidebar = components.find((component) =>
      component.id === "global-sidebar"
    );
    const topbar = components.find((component) =>
      component.id === "workspace-topbar"
    );
    const mobileNavigation = components.find((component) =>
      component.id === "mobile-bottom-navigation"
    );
    const spaceSelector = components.find((component) =>
      component.id === "space-selector"
    );

    expect(sidebar).toMatchObject({
      type: "sidebar",
      position: "left-fixed",
      width: "228px",
      items: ["Spaces"],
    });
    const spaceSidebar = components.find((component) =>
      component.id === "space-sidebar"
    );
    expect(spaceSidebar).toMatchObject({
      type: "sidebar",
      position: "left-fixed",
      width: "228px",
      items: ["Home", "Forms", "Search", "Settings"],
    });
    expect(topbar).toMatchObject({ type: "top-bar", height: "58px" });
    expect(mobileNavigation).toMatchObject({
      type: "bottom-navigation",
      height: "66px",
      breakpoint: "900px",
      items: ["Home", "Forms", "Search", "Settings"],
    });
    const globalMobileNavigation = components.find((component) =>
      component.id === "global-mobile-bottom-navigation"
    );
    expect(globalMobileNavigation).toMatchObject({
      type: "bottom-navigation",
      height: "66px",
      breakpoint: "900px",
      items: ["Spaces"],
    });
    expect(spaceSelector).toMatchObject({
      type: "select",
      scope: "authorized-spaces",
      option_label: "name-then-slug-then-localized-untitled",
      unresolved_current_space_label: "localized-loading-text",
      unavailable_current_space_label: "localized-neutral-label",
      displays_space_uid: false,
    });
  });

  it("REQ-FE-067: keeps one ordinary Form and Entry workspace", () => {
    const pages = loadPages();
    const forms = pages.find(({ spec }) => spec.page?.id === "space-form-grid");
    const entries = pages.find(({ spec }) =>
      spec.page?.id === "space-form-entries"
    );
    expect(
      forms?.spec.components?.body?.some((component) =>
        component.type === "tab-bar"
      ),
    ).toBe(false);
    const entryList = entries?.spec.components?.body?.find((component) =>
      component.id === "form-entry-list"
    );
    expect(entryList).toMatchObject({
      type: "query-results",
      component: "EntryBrowser",
      reference: "../components/entry-browser.yaml",
    });
    const componentPath = path.resolve(
      path.dirname(entries!.filePath),
      String(entryList?.reference),
    );
    expect(componentPath.startsWith(`${componentsDir}${path.sep}`)).toBe(true);
    const entryBrowser = parse(readFileSync(componentPath, "utf8")) as {
      components?: Array<Record<string, unknown>>;
    };
    const table = entryBrowser.components?.find(({ id }) =>
      id === "form-entry-table"
    );
    expect(table).toMatchObject({
      query_results: {
        pagination: "server-keyset",
        count: "explicit-only",
      },
      columns: { system_timestamps: { order: ["created_at", "updated_at"] } },
      selection: {
        row_click: "select-only",
        open_action: { control: "native-button" },
      },
      responsive: {
        page_horizontal_overflow: false,
        trailing_action: "remains-at-right-edge",
      },
    });
    expect(table?.multi_sort_dialog).toMatchObject({ maximum_rules: 8 });
    const compat = pages.find(({ spec }) =>
      spec.page?.id === "space-entries-object"
    );
    expect(compat?.spec.page?.implementation).not.toBe("implemented");
  });

  it("REQ-SRCH-006: specifies Search-owned submit and saved-query toolbar slots", () => {
    const search = loadPages().find(({ spec }) =>
      spec.page?.id === "space-search"
    );
    expect(search?.spec.page).toMatchObject({
      implementation: "implemented",
      visible_heading: "screen-reader-only",
    });
    const results = search?.spec.components?.body?.find(({ id }) =>
      id === "keyword-result-list"
    );
    expect(results).toMatchObject({
      type: "query-results",
      reference: "../components/entry-browser.yaml",
      toolbar: {
        search_control: {
          owner: "search-page",
          role: "search",
          commit: "explicit-submit",
          deep_link: "query.q",
        },
        result_actions: [
          "columns-dialog",
          "filters-dialog",
          "multi-sort-dialog",
        ],
        navigation: {
          control: "icon-link",
          icon: "UiIcon.sql",
          accessible_name: "searchPage.openSavedQueries",
          target: "/spaces/{space_id}/sql",
        },
      },
    });

    const componentPath = path.resolve(
      path.dirname(search!.filePath),
      String(results?.reference),
    );
    const entryBrowser = parse(readFileSync(componentPath, "utf8")) as {
      components?: Array<Record<string, unknown>>;
    };
    expect(entryBrowser.components?.find(({ id }) => id === "form-entry-table"))
      .toMatchObject({
        toolbar: {
          built_in_search: { default: "visible", update: "input" },
          search_control: {
            prop: "searchControl",
            behavior: "replaces-built-in-search",
          },
          navigation: {
            prop: "toolbarNavigation",
            behavior: "render-caller-owned-content-when-supplied",
            position: "after-display-actions",
          },
        },
      });
  });
});
