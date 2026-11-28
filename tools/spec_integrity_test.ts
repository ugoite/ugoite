import { assert } from "@std/assert/assert";
import path from "node:path";

const repoRoot = path.resolve(Deno.cwd());
Deno.test("REQ-API-013: MCP documentation describes the shipped semantic facade", async () => {
  const source = await Deno.readTextFile(
    path.join(repoRoot, "docs/architecture/api/mcp.md"),
  );
  assert(source.includes("POST /mcp"), "MCP docs must describe POST /mcp");
  assert(
    source.includes("ugoite.search"),
    "MCP docs must describe ugoite.search",
  );
  assert(
    source.includes("ugoite://entry/{id}"),
    "MCP docs must describe the entry resource URI",
  );
  assert(
    source.includes("/.well-known/oauth-protected-resource"),
    "MCP docs must describe protected-resource metadata",
  );
  assert(/DPoP/i.test(source), "MCP docs must describe DPoP");
});
