const commands = [
  new Deno.Command("cargo", {
    env: {
      UGOITE_BOOTSTRAP_TOKEN: Deno.env.get("UGOITE_BOOTSTRAP_TOKEN") ??
        "dev-token",
      UGOITE_DEV_USER_ID: Deno.env.get("UGOITE_DEV_USER_ID") ??
        "dev-local-user",
    },
    args: ["run", "-p", "ugoite-server"],
  }),
  new Deno.Command("deno", {
    args: ["task", "frontend:dev"],
  }),
  new Deno.Command("deno", {
    args: ["task", "docsite:dev"],
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
