import { validatePrBody } from "./pr_body_gate.ts";

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

const errors = validatePrBody(body);
if (errors.length > 0) throw new Error(errors.join("\n"));

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
