const FORBIDDEN_SYMBOLS = [
  "parseMarkdownToStructuredDraft",
  "parseMarkdownFrontmatterTags",
  "updateH2Section",
  "replaceFirstH1",
] as const;

// These modules are explicitly compatibility/presentation-only. They do not
// decide whether a Knowledge mutation is valid or durable.
const COMPATIBILITY_MODULES = new Set([
  "frontend/src/lib/entry-input.ts",
  "frontend/src/lib/markdown.ts",
  "frontend/src/components/FormTable.tsx",
]);

function symbolPattern(symbol: string): RegExp {
  return new RegExp(`\\b${symbol}\\b`);
}

export type AuthorityViolation = {
  path: string;
  symbols: string[];
};

export function findEntryAuthorityViolations(
  path: string,
  source: string,
): AuthorityViolation | undefined {
  // Tests and fixtures may mention the legacy helpers to prove they are not
  // used as the authority. Only production Entry authoring modules are
  // subject to this dependency gate.
  if (!path.startsWith("frontend/src/") || path.includes(".test.")) {
    return undefined;
  }
  if (COMPATIBILITY_MODULES.has(path)) return undefined;
  const symbols = FORBIDDEN_SYMBOLS.filter((symbol) =>
    symbolPattern(symbol).test(source)
  );
  return symbols.length > 0 ? { path, symbols } : undefined;
}

export async function readEntryAuthorityViolations(
  root = ".",
): Promise<AuthorityViolation[]> {
  const command = new Deno.Command("git", {
    args: ["ls-files", "frontend/src"],
    cwd: root,
    stdout: "piped",
  });
  const output = await command.output();
  if (!output.success) {
    throw new Error(new TextDecoder().decode(output.stderr));
  }
  const paths = new TextDecoder().decode(output.stdout).trim().split("\n")
    .filter((path) => path.endsWith(".ts") || path.endsWith(".tsx"));
  const violations: AuthorityViolation[] = [];
  for (const path of paths) {
    let source: string;
    try {
      source = await Deno.readTextFile(`${root}/${path}`);
    } catch (error) {
      if (error instanceof Deno.errors.NotFound) continue;
      throw error;
    }
    const violation = findEntryAuthorityViolations(path, source);
    if (violation) violations.push(violation);
  }
  return violations;
}
