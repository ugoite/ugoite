const repoRoot = new URL("..", import.meta.url).pathname;

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
  new Deno.Command("deno", {
    cwd: `${repoRoot}frontend`,
    args: ["task", "dev"],
    env: {
      BACKEND_URL: Deno.env.get("BACKEND_URL") ?? "http://localhost:8000",
      UGOITE_STATIC_SPA: Deno.env.get("UGOITE_STATIC_SPA") ?? "true",
      VITE_API_PROXY: Deno.env.get("VITE_API_PROXY") ?? "true",
    },
  }),
  new Deno.Command("deno", {
    cwd: `${repoRoot}docsite`,
    args: ["task", "dev"],
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
