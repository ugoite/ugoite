import { promises as fs } from "node:fs";
import path from "node:path";
import YAML from "yaml";

const docsRoot = path.resolve(process.cwd(), "../docs");
const specRoot = path.join(docsRoot, "spec");

export type LinkItem = {
  id: string;
  title: string;
  summary?: string;
};

export type RequirementGroup = {
  setId: string;
  sourceFile: string;
  scope: string;
  requirements: Array<{
    id: string;
    title: string;
    priority?: string;
    status?: string;
  }>;
};

export type FeatureApi = {
  id: string;
  method: string;
  backend: {
    path: string;
    file: string;
    function: string;
  };
  frontend?: {
    path: string;
    file: string;
    function: string;
  };
  ugoite_core: {
    file: string;
    function: string;
  };
  ugoite_cli?: {
    command?: string;
    file: string;
    function: string;
  };
};

export type FeatureGroup = {
  kind: string;
  file: string;
  apis: FeatureApi[];
};

export type UiPageSpec = {
  fileName: string;
  id: string;
  title: string;
  route: string;
  implementation?: string;
};

type FeatureRegistry = {
  files?: Array<{
    kind: string;
    file: string;
  }>;
};

async function readYaml<T>(absolutePath: string): Promise<T> {
  const raw = await fs.readFile(absolutePath, "utf-8");
  return YAML.parse(raw) as T;
}

function sortByTitle<T extends { title: string }>(items: T[]): T[] {
  return [...items].sort((a, b) => a.title.localeCompare(b.title));
}

export async function getPhilosophies(): Promise<LinkItem[]> {
  const filePath = path.join(specRoot, "philosophy/foundation.yaml");
  const data = await readYaml<{ philosophies?: Array<{ id: string; title: string; statement?: string }> }>(filePath);
  return sortByTitle(
    (data.philosophies ?? []).map((item) => ({
      id: item.id,
      title: item.title,
      summary: item.statement?.trim(),
    }))
  );
}

export async function getPolicies(): Promise<LinkItem[]> {
  const filePath = path.join(specRoot, "policies/policies.yaml");
  const data = await readYaml<{ policies?: Array<{ id: string; title: string; summary?: string }> }>(filePath);
  return sortByTitle(
    (data.policies ?? []).map((item) => ({
      id: item.id,
      title: item.title,
      summary: item.summary,
    }))
  );
}

export async function getRequirementGroups(): Promise<RequirementGroup[]> {
  const reqDir = path.join(specRoot, "requirements");
  const files = (await fs.readdir(reqDir)).filter((name) => name.endsWith(".yaml"));
  const groups: RequirementGroup[] = [];

  for (const fileName of files) {
    const fullPath = path.join(reqDir, fileName);
    const data = await readYaml<{
      requirements?: Array<{
        set_id: string;
        source_file: string;
        scope?: string;
        id: string;
        title: string;
        priority?: string;
        status?: string;
      }>;
    }>(fullPath);

    const entries = data.requirements ?? [];
    if (entries.length === 0) continue;

    const first = entries[0];
    groups.push({
      setId: first.set_id,
      sourceFile: first.source_file,
      scope: first.scope ?? "",
      requirements: sortByTitle(
        entries.map((entry) => ({
          id: entry.id,
          title: entry.title,
          priority: entry.priority,
          status: entry.status,
        }))
      ),
    });
  }

  return groups.sort((a, b) => a.setId.localeCompare(b.setId));
}

export async function getFeatureGroups(): Promise<FeatureGroup[]> {
  const registryPath = path.join(specRoot, "features/features.yaml");
  const registry = await readYaml<FeatureRegistry>(registryPath);
  const files = registry.files ?? [];
  const groups: FeatureGroup[] = [];

  for (const entry of files) {
    const fullPath = path.join(specRoot, "features", entry.file);
    const data = await readYaml<{ kind?: string; apis?: FeatureApi[] }>(fullPath);
    groups.push({
      kind: data.kind ?? entry.kind,
      file: entry.file,
      apis: data.apis ?? [],
    });
  }

  return groups.sort((a, b) => a.kind.localeCompare(b.kind));
}

export async function getUiPages(): Promise<UiPageSpec[]> {
  const uiPagesDir = path.join(specRoot, "ui/pages");
  const files = (await fs.readdir(uiPagesDir)).filter((name) => name.endsWith(".yaml"));
  const pages: UiPageSpec[] = [];

  for (const fileName of files) {
    const fullPath = path.join(uiPagesDir, fileName);
    const data = await readYaml<{
      page?: {
        id?: string;
        title?: string;
        route?: string;
        implementation?: string;
      };
    }>(fullPath);
    if (!data.page?.id || !data.page.title || !data.page.route) continue;

    pages.push({
      fileName,
      id: data.page.id,
      title: data.page.title,
      route: data.page.route,
      implementation: data.page.implementation,
    });
  }

  return pages.sort((a, b) => a.title.localeCompare(b.title));
}

export function getDocsRoot(): string {
  return docsRoot;
}

export function getSpecRoot(): string {
  return specRoot;
}

/* ─── Relationship graph data ─── */

export type PolicyDetail = {
  id: string;
  title: string;
  summary?: string;
  linkedRequirements: string[];
  linkedSpecifications: string[];
};

export type SpecEntry = {
  id: string;
  sourceFile: string;
  linkedPolicies: string[];
  linkedRequirements: string[];
};

export async function getPoliciesDetailed(): Promise<PolicyDetail[]> {
  const filePath = path.join(specRoot, "policies/policies.yaml");
  const data = await readYaml<{
    policies?: Array<{
      id: string;
      title: string;
      summary?: string;
      linked_requirements?: string[];
      linked_specifications?: string[];
    }>;
  }>(filePath);
  return (data.policies ?? []).map((p) => ({
    id: p.id,
    title: p.title,
    summary: p.summary,
    linkedRequirements: p.linked_requirements ?? [],
    linkedSpecifications: p.linked_specifications ?? [],
  }));
}

export async function getSpecifications(): Promise<SpecEntry[]> {
  const filePath = path.join(specRoot, "specifications.yaml");
  const data = await readYaml<
    Array<{
      id: string;
      source_file: string;
      linked_policies?: string[];
      linked_requirements?: string[];
    }>
  >(filePath);
  return (data ?? []).map((s) => ({
    id: s.id,
    sourceFile: s.source_file,
    linkedPolicies: s.linked_policies ?? [],
    linkedRequirements: s.linked_requirements ?? [],
  }));
}
