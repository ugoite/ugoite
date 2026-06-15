const commands = [
  new Deno.Command("uv", {
    cwd: "backend",
    args: [
      "run",
      "uvicorn",
      "src.app.main:app",
      "--reload",
      "--host",
      "0.0.0.0",
      "--port",
      "8000",
    ],
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
