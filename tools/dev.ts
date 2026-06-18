const repoRoot = new URL("..", import.meta.url).pathname;
const frontendUrl = "http://localhost:3000";
const backendUrl = "http://localhost:8000";

const shellQuote = (value: string): string =>
  `'${value.replaceAll("'", `'\\''`)}'`;

const commands = [
  new Deno.Command("cargo", {
    cwd: repoRoot,
    env: {
      UGOITE_ROOT: Deno.env.get("UGOITE_ROOT") ?? repoRoot,
      UGOITE_BOOTSTRAP_DEFAULT_SPACE:
        Deno.env.get("UGOITE_BOOTSTRAP_DEFAULT_SPACE") ?? "true",
      UGOITE_BOOTSTRAP_TOKEN: Deno.env.get("UGOITE_BOOTSTRAP_TOKEN") ??
        "dev-token",
      UGOITE_DEV_AUTH_MODE: Deno.env.get("UGOITE_DEV_AUTH_MODE") ??
        "mock-oauth",
      UGOITE_DEV_USER_ID: Deno.env.get("UGOITE_DEV_USER_ID") ??
        "dev-local-user",
    },
    args: ["run", "-p", "ugoite-server"],
  }),
  new Deno.Command("sh", {
    cwd: `${repoRoot}frontend`,
    args: [
      "-lc",
      [
        `BACKEND_URL=${shellQuote(Deno.env.get("BACKEND_URL") ?? backendUrl)}`,
        `UGOITE_STATIC_SPA=${
          shellQuote(Deno.env.get("UGOITE_STATIC_SPA") ?? "true")
        }`,
        `VITE_API_PROXY=${
          shellQuote(Deno.env.get("VITE_API_PROXY") ?? "true")
        }`,
        "node ./node_modules/vinxi/bin/cli.mjs dev --host 127.0.0.1 --strictPort --port 3000",
      ].join(" "),
    ],
  }),
  new Deno.Command("node", {
    cwd: `${repoRoot}docsite`,
    args: [
      "./node_modules/astro/bin/astro.mjs",
      "dev",
      "--host",
      "127.0.0.1",
      "--strictPort",
      "--port",
      "4321",
    ],
  }),
];

const children = commands.map((command) => command.spawn());

const shutdown = () => {
  for (const child of children) {
    try {
      child.kill("SIGTERM");
    } catch {
      // The process already exited.
    }
  }
};

for (const signal of ["SIGINT", "SIGTERM"] as const) {
  Deno.addSignalListener(signal, shutdown);
}

const statuses = await Promise.all(children.map((child) => child.status));
shutdown();

if (statuses.some((status) => !status.success)) {
  Deno.exit(1);
}
