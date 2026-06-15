const args = [...Deno.args];

function take(flag: string): string {
  const index = args.indexOf(flag);
  if (index === -1 || !args[index + 1]) {
    throw new Error(`Missing required argument: ${flag}`);
  }
  return args[index + 1];
}

const title = take("--title");
const bodyFile = take("--body-file");
const body = await Deno.readTextFile(bodyFile);

for (
  const section of ["## Summary", "## Related Issue (required)", "## Testing"]
) {
  if (!body.toLowerCase().includes(section.toLowerCase())) {
    throw new Error(`PR body is missing required section: ${section}`);
  }
}
if (!/(?:close:\s*#\d+|closes\s+#\d+)/i.test(body)) {
  throw new Error("PR body must close a related issue");
}

const result = await new Deno.Command("gh", {
  args: [
    "pr",
    "create",
    "--title",
    title,
    "--body-file",
    bodyFile,
    ...(args.includes("--draft") ? ["--draft"] : []),
  ],
  stdin: "inherit",
  stdout: "inherit",
  stderr: "inherit",
}).output();
Deno.exit(result.code);
