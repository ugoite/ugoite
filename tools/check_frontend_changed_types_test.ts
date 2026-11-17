import { assertEquals } from "@std/assert";
import {
  changedSourceDiffRange,
  isFrontendApplicationTypeScript,
} from "./check_frontend_changed_types.ts";

async function gitOutput(cwd: string, args: string[]): Promise<string> {
  const result = await new Deno.Command("git", {
    args,
    cwd,
    stdout: "piped",
    stderr: "piped",
  }).output();
  if (!result.success) {
    throw new Error(new TextDecoder().decode(result.stderr));
  }
  return new TextDecoder().decode(result.stdout).trim();
}

Deno.test("changed frontend type check includes application TypeScript", () => {
  assertEquals(
    isFrontendApplicationTypeScript("frontend/src/components/Composition.tsx"),
    true,
  );
  assertEquals(
    isFrontendApplicationTypeScript("frontend/src/lib/composition.ts"),
    true,
  );
});

Deno.test("changed frontend type check excludes tests, declarations, and other paths", () => {
  assertEquals(
    isFrontendApplicationTypeScript(
      "frontend/src/components/Composition.test.tsx",
    ),
    false,
  );
  assertEquals(
    isFrontendApplicationTypeScript("frontend/src/types.d.ts"),
    false,
  );
  assertEquals(isFrontendApplicationTypeScript("tools/composition.ts"), false);
});

Deno.test("changed source diff works with shallow base and head commits", async () => {
  const root = await Deno.makeTempDir({ prefix: "ugoite-shallow-diff-" });
  try {
    const origin = `${root}/origin.git`;
    const source = `${root}/source`;
    const clone = `${root}/clone`;
    await gitOutput(root, ["init", "--bare", origin]);
    await gitOutput(root, ["init", source]);
    await gitOutput(source, ["config", "user.name", "Validation"]);
    await gitOutput(source, [
      "config",
      "user.email",
      "validation@example.invalid",
    ]);
    await Deno.writeTextFile(`${source}/base.txt`, "base\n");
    await gitOutput(source, ["add", "base.txt"]);
    await gitOutput(source, ["commit", "-m", "base"]);
    const base = await gitOutput(source, ["rev-parse", "HEAD"]);
    await gitOutput(source, ["branch", "-M", "main"]);
    await gitOutput(source, ["remote", "add", "origin", origin]);
    await gitOutput(source, ["push", "-u", "origin", "main"]);
    await gitOutput(source, ["checkout", "-b", "feature"]);
    await Deno.mkdir(`${source}/frontend/src`, { recursive: true });
    await Deno.writeTextFile(
      `${source}/frontend/src/Changed.ts`,
      "export const changed = true;\n",
    );
    await gitOutput(source, ["add", "frontend/src/Changed.ts"]);
    await gitOutput(source, ["commit", "-m", "feature"]);
    await gitOutput(source, ["push", "-u", "origin", "feature"]);
    await gitOutput(root, [
      "clone",
      "--depth=1",
      "--branch",
      "feature",
      `file://${origin}`,
      clone,
    ]);
    await gitOutput(clone, ["fetch", "--no-tags", "--depth=1", "origin", base]);

    const mergeBase = await new Deno.Command("git", {
      args: ["merge-base", base, "HEAD"],
      cwd: clone,
      stdout: "piped",
      stderr: "null",
    }).output();
    assertEquals(mergeBase.success, false);
    assertEquals(
      await gitOutput(clone, [
        "diff",
        changedSourceDiffRange(base),
        "--name-only",
        "--diff-filter=ACMR",
        "--",
        "frontend/src",
      ]),
      "frontend/src/Changed.ts",
    );
  } finally {
    await Deno.remove(root, { recursive: true });
  }
});
