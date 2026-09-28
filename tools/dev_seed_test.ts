import { assertEquals } from "@std/assert/equals";

const repoRoot = decodeURIComponent(new URL("../", import.meta.url).pathname);
const scriptPath = new URL("../scripts/dev-seed.sh", import.meta.url).pathname;
const mockSpaceUid = "019f0000-0000-7000-8000-000000000001";

async function checkoutSha(): Promise<string> {
  const result = await new Deno.Command("git", {
    args: ["-C", repoRoot, "rev-parse", "HEAD"],
    stdout: "piped",
  }).output();
  if (!result.success) throw new Error("could not resolve test checkout SHA");
  return new TextDecoder().decode(result.stdout).trim();
}

async function writeMockSeeder(
  directory: string,
  argsOutputPath: string,
): Promise<string> {
  const path = directory + "/mock-xtask";
  const script = [
    "#!/usr/bin/env bash",
    "set -euo pipefail",
    'printf \'%s\\n\' "$@" >"$MOCK_XTASK_ARGS"',
    'root=""',
    'slug=""',
    "while (($# > 0)); do",
    '  case "$1" in',
    "    seed) shift ;;",
    '    --root) root="$2"; shift 2 ;;',
    '    --space-id) slug="$2"; shift 2 ;;',
    "    *) shift ;;",
    "  esac",
    "done",
    'mkdir -p "$root/spaces/' + mockSpaceUid + '"',
    'printf \'{"slug":"%s","space_uid":"' + mockSpaceUid +
    '"}\\n\' "$slug" >"$root/spaces/' + mockSpaceUid + '/meta.json"',
    "",
  ].join("\n");
  await Deno.writeTextFile(path, script);
  await Deno.chmod(path, 0o755);
  await Deno.writeTextFile(
    path + ".source-sha",
    (await checkoutSha()) + "\n",
  );
  await Deno.writeTextFile(directory + "/mock-args-path", argsOutputPath);
  return path;
}

async function writeFailingCargo(directory: string): Promise<void> {
  const path = directory + "/cargo";
  await Deno.writeTextFile(
    path,
    '#!/usr/bin/env bash\nprintf called >"$CARGO_MARKER"\nexit 82\n',
  );
  await Deno.chmod(path, 0o755);
}

async function writeDelegatingCargo(directory: string): Promise<void> {
  const path = directory + "/cargo";
  const script = [
    "#!/usr/bin/env bash",
    "set -euo pipefail",
    'printf \'%s\\n\' "$@" >"$CARGO_ARGS_OUTPUT"',
    "while (($# > 0)); do",
    '  if [[ "$1" == "--" ]]; then shift; break; fi',
    "  shift",
    "done",
    'exec "$MOCK_XTASK" "$@"',
    "",
  ].join("\n");
  await Deno.writeTextFile(path, script);
  await Deno.chmod(path, 0o755);
}

function testPath(binDirectory: string): string {
  const denoDirectory = Deno.execPath().replace(/\/[^/]+$/, "");
  return binDirectory + ":" + denoDirectory + ":" +
    (Deno.env.get("PATH") ?? "");
}

async function readFileIfPresent(path: string): Promise<string | null> {
  try {
    return await Deno.readTextFile(path);
  } catch {
    return null;
  }
}

async function runSeed(
  root: string,
  env: Record<string, string>,
): Promise<Deno.CommandOutput> {
  return await new Deno.Command("bash", {
    args: [
      scriptPath,
      "--root",
      root,
      "--space-id",
      "test-space",
      "--scenario",
      "renewable-ops",
      "--entry-count",
      "1",
      "--seed",
      "7",
    ],
    cwd: repoRoot,
    env: { ...Deno.env.toObject(), ...env },
    stdout: "piped",
    stderr: "piped",
  }).output();
}

Deno.test("dev-seed uses an explicit source-matched xtask without a Cargo fallback", async () => {
  const directory = await Deno.makeTempDir({ prefix: "ugoite-seed-binary-" });
  try {
    const binDirectory = directory + "/bin";
    await Deno.mkdir(binDirectory);
    const seederArgs = directory + "/xtask-args.txt";
    const cargoMarker = directory + "/cargo-called";
    const seeder = await writeMockSeeder(directory, seederArgs);
    await writeFailingCargo(binDirectory);

    const result = await runSeed(directory + "/root", {
      PATH: testPath(binDirectory),
      MOCK_XTASK_ARGS: seederArgs,
      CARGO_MARKER: cargoMarker,
      UGOITE_SEED_XTASK_BINARY: seeder,
    });
    assertEquals(result.code, 0, new TextDecoder().decode(result.stderr));
    assertEquals(await readFileIfPresent(cargoMarker), null);
    assertEquals(
      (await Deno.readTextFile(seederArgs)).trim().split("\n"),
      [
        "seed",
        "--root",
        directory + "/root",
        "--space-id",
        "test-space",
        "--scenario",
        "renewable-ops",
        "--entry-count",
        "1",
        "--seed",
        "7",
      ],
    );
    const metadata = JSON.parse(
      await Deno.readTextFile(
        directory + "/root/spaces/" + mockSpaceUid + "/meta.json",
      ),
    );
    assertEquals(metadata, { slug: "test-space", space_uid: mockSpaceUid });
  } finally {
    await Deno.remove(directory, { recursive: true });
  }
});

Deno.test("dev-seed rejects a missing explicit xtask without invoking Cargo", async () => {
  const directory = await Deno.makeTempDir({ prefix: "ugoite-seed-missing-" });
  try {
    const binDirectory = directory + "/bin";
    await Deno.mkdir(binDirectory);
    const cargoMarker = directory + "/cargo-called";
    await writeFailingCargo(binDirectory);

    const result = await runSeed(directory + "/root", {
      PATH: testPath(binDirectory),
      CARGO_MARKER: cargoMarker,
      UGOITE_SEED_XTASK_BINARY: directory + "/missing-xtask",
    });
    assertEquals(result.code, 1);
    assertEquals(
      new TextDecoder().decode(result.stderr).includes(
        "UGOITE_SEED_XTASK_BINARY must name an executable regular file",
      ),
      true,
    );
    assertEquals(await readFileIfPresent(cargoMarker), null);
  } finally {
    await Deno.remove(directory, { recursive: true });
  }
});

Deno.test("dev-seed rejects a stale explicit xtask source SHA without Cargo fallback", async () => {
  const directory = await Deno.makeTempDir({ prefix: "ugoite-seed-stale-" });
  try {
    const binDirectory = directory + "/bin";
    await Deno.mkdir(binDirectory);
    const cargoMarker = directory + "/cargo-called";
    const seederArgs = directory + "/xtask-args.txt";
    const seeder = await writeMockSeeder(directory, seederArgs);
    await Deno.writeTextFile(
      seeder + ".source-sha",
      "0".repeat(40) + "\n",
    );
    await writeFailingCargo(binDirectory);

    const result = await runSeed(directory + "/root", {
      PATH: testPath(binDirectory),
      CARGO_MARKER: cargoMarker,
      MOCK_XTASK_ARGS: seederArgs,
      UGOITE_SEED_XTASK_BINARY: seeder,
    });
    assertEquals(result.code, 1);
    assertEquals(
      new TextDecoder().decode(result.stderr).includes(
        "Explicit xtask binary source SHA does not match this checkout",
      ),
      true,
    );
    assertEquals(await readFileIfPresent(cargoMarker), null);
    assertEquals(await readFileIfPresent(seederArgs), null);
  } finally {
    await Deno.remove(directory, { recursive: true });
  }
});

Deno.test("dev-seed keeps the local Cargo execution path when no binary is selected", async () => {
  const directory = await Deno.makeTempDir({ prefix: "ugoite-seed-cargo-" });
  try {
    const binDirectory = directory + "/bin";
    await Deno.mkdir(binDirectory);
    const cargoArgs = directory + "/cargo-args.txt";
    const seederArgs = directory + "/xtask-args.txt";
    const seeder = await writeMockSeeder(directory, seederArgs);
    await writeDelegatingCargo(binDirectory);

    const result = await runSeed(directory + "/root", {
      PATH: testPath(binDirectory),
      CARGO_ARGS_OUTPUT: cargoArgs,
      MOCK_XTASK_ARGS: seederArgs,
      MOCK_XTASK: seeder,
      CARGO_TARGET_DIR: "target/rust",
    });
    assertEquals(result.code, 0, new TextDecoder().decode(result.stderr));
    assertEquals(
      (await Deno.readTextFile(cargoArgs)).trim().split("\n").slice(0, 5),
      ["run", "-q", "-p", "xtask", "--"],
    );
    assertEquals(
      (await Deno.readTextFile(seederArgs)).trim().split("\n")[0],
      "seed",
    );
  } finally {
    await Deno.remove(directory, { recursive: true });
  }
});
