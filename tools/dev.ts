const repoRoot = new URL("..", import.meta.url).pathname;
const devSecretPath = `${repoRoot}target/dev-node-secret`;

await Deno.mkdir(`${repoRoot}target`, { recursive: true, mode: 0o700 });
try {
  const file = await Deno.open(devSecretPath, {
    createNew: true,
    write: true,
    mode: 0o600,
  });
  try {
    await file.write(crypto.getRandomValues(new Uint8Array(32)));
    await file.sync();
  } finally {
    file.close();
  }
} catch (error) {
  if (!(error instanceof Deno.errors.AlreadyExists)) throw error;
}

const commands = [
  new Deno.Command("cargo", {
    cwd: repoRoot,
    env: {
      UGOITE_ROOT: Deno.env.get("UGOITE_ROOT") ?? repoRoot,
      UGOITE_PUBLIC_ORIGIN: Deno.env.get("UGOITE_PUBLIC_ORIGIN") ??
        "http://localhost:3000",
      UGOITE_API_BASE_URL: Deno.env.get("UGOITE_API_BASE_URL") ??
        "http://localhost:3000/api",
      UGOITE_WEBAUTHN_RP_ID: Deno.env.get("UGOITE_WEBAUTHN_RP_ID") ??
        "localhost",
      UGOITE_NODE_SECRET_FILE: Deno.env.get("UGOITE_NODE_SECRET_FILE") ??
        devSecretPath,
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
