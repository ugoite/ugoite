import { assertEquals } from "@std/assert/equals";

const scriptPath = new URL(
  "../scripts/measure-sql-export.sh",
  import.meta.url,
).pathname;

Deno.test("an unusable explicit SQL export CLI fails before a Cargo fallback", async () => {
  const directory = await Deno.makeTempDir({ prefix: "ugoite-sql-cli-test-" });
  try {
    const binDirectory = `${directory}/bin`;
    await Deno.mkdir(binDirectory);
    const cargoMarker = `${directory}/cargo-called`;
    const cargoStub = `${binDirectory}/cargo`;
    await Deno.writeTextFile(
      cargoStub,
      '#!/usr/bin/env bash\nprintf called >>"$CARGO_MARKER"\nexit 0\n',
    );
    await Deno.chmod(cargoStub, 0o755);

    const denoDirectory = Deno.execPath().replace(/\/[^/]+$/, "");
    const result = await new Deno.Command("bash", {
      args: [scriptPath],
      env: {
        PATH: `${binDirectory}:${denoDirectory}:${Deno.env.get("PATH") ?? ""}`,
        CARGO_MARKER: cargoMarker,
        UGOITE_SQL_EXPORT_MEASURE_OUTPUT: `${directory}/output`,
        UGOITE_CP1_PROFILE_DIR: `${directory}/profiles`,
        UGOITE_SQL_EXPORT_MEASURE_ROOT: `${directory}/root`,
        UGOITE_SQL_EXPORT_CLI_BINARY: `${directory}/missing-ugoite`,
      },
      stdout: "piped",
      stderr: "piped",
    }).output();

    assertEquals(result.code, 1);
    assertEquals(
      new TextDecoder().decode(result.stderr).includes(
        "UGOITE_SQL_EXPORT_CLI_BINARY must name an executable regular file",
      ),
      true,
    );
    let cargoWasCalled = false;
    try {
      await Deno.stat(cargoMarker);
      cargoWasCalled = true;
    } catch {
      // The marker is absent when the explicit-binary path fails closed.
    }
    assertEquals(cargoWasCalled, false);
  } finally {
    await Deno.remove(directory, { recursive: true });
  }
});
