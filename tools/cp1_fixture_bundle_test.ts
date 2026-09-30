import { assertRejects } from "@std/assert/rejects";
import { assertThrows } from "@std/assert/throws";
import { assertEquals } from "@std/assert/equals";
import { assert } from "@std/assert/assert";
import {
  assertFixtureRunBinding,
  parseFixtureManifest,
  readTarMembers,
  validateCp1ArchiveMembers,
  validateProducerTree,
  validateSeedProfileDocument,
} from "./cp1_fixture_bundle.ts";
import {
  CP1_SEED_MUTATION_BATCH_LIMIT,
  type Cp1Fixture,
  cp1FixtureBySlug,
  cp1FixturesFor,
} from "./cp1_fixture_spec.ts";

const uidFor = (index: number) =>
  `018f0000-0000-7${String(index).padStart(3, "0")}-8000-${
    String(index).padStart(12, "0")
  }`;

function makeReadback(fixture: Cp1Fixture, index: number) {
  const scenarioForms = fixture.formNames.filter((name) => name !== "Entry");
  const base = Math.floor(fixture.entryCount / scenarioForms.length);
  const remainder = fixture.entryCount % scenarioForms.length;
  const form_entry_counts = Object.fromEntries(
    fixture.formNames.map((name) => [
      name,
      name === "Entry"
        ? 0
        : base + (scenarioForms.indexOf(name) < remainder ? 1 : 0),
    ]),
  );
  const owner = fixture.ownerDisplayName === null ? { mode: "none" } : {
    mode: "owner",
    principal_id: uidFor(index + 10),
    display_name: fixture.ownerDisplayName,
    role: "owner",
    active: true,
    authorized_read_verified: true,
    non_owner_denial_verified: true,
  };
  return {
    schema_version: 1,
    space_slug: fixture.slug,
    space_uid: uidFor(index),
    scenario: fixture.scenario,
    entry_count: fixture.entryCount,
    form_names: [...fixture.formNames].sort(),
    form_entry_counts,
    owner,
    integrity: {
      deep: true,
      status: "valid",
      changes_and_audit: "valid",
      authorization: fixture.ownerDisplayName === null ? "incomplete" : "valid",
    },
  };
}

function makeManifest(kind: "query" | "export") {
  const fixtures = cp1FixturesFor(kind);
  const readbacks = fixtures.map(makeReadback);
  return {
    schema_version: 1,
    source_sha: "a".repeat(40),
    ci_run_id: "123456789",
    fixture_kind: kind,
    fixture_spec_schema_version: 1,
    measurement_schema_version: 1,
    generator_fingerprint: "b".repeat(64),
    archive: {
      path: kind === "query"
        ? "query-fixtures.tar.gz"
        : "export-fixtures.tar.gz",
      sha256: "c".repeat(64),
      size: 1234,
      packaging_micros: 567,
    },
    seed_profiles: fixtures.map((fixture) => ({
      slug: fixture.slug,
      profile_path: `.cp1-profiles/${fixture.slug}.json`,
      resource_path: `.cp1-profiles/${fixture.slug}.time.txt`,
      schema_version: 1,
    })),
    fixtures: fixtures.map((fixture, index) => {
      const readback = readbacks[index]!;
      return {
        slug: fixture.slug,
        scenario: fixture.scenario,
        seed: fixture.seed,
        owner_mode: fixture.ownerDisplayName === null ? "none" : "owner",
        owner_display_name: fixture.ownerDisplayName,
        space_uid: readback.space_uid,
        expected_entry_count: fixture.entryCount,
        verified_entry_count: readback.entry_count,
        form_names: readback.form_names,
        form_entry_counts: readback.form_entry_counts,
        canonical_readback: readback,
      };
    }),
  };
}

function makeArchiveMembers(kind: "query" | "export") {
  const fixtures = cp1FixturesFor(kind);
  const readbacks = fixtures.map(makeReadback);
  const members: Array<{
    path: string;
    kind: "directory" | "file";
    mode: number;
    size: number;
  }> = [
    { path: "spaces", kind: "directory" as const, mode: 0o700, size: 0 },
    {
      path: "spaces/.ugoite-atomic-writes",
      kind: "directory" as const,
      mode: 0o700,
      size: 0,
    },
    { path: ".cp1-profiles", kind: "directory" as const, mode: 0o700, size: 0 },
    {
      path: "spaces/.ugoite-space-slug-claims",
      kind: "directory" as const,
      mode: 0o700,
      size: 0,
    },
  ];
  for (const [index, fixture] of fixtures.entries()) {
    const uid = readbacks[index]!.space_uid;
    members.push(
      {
        path: `spaces/${uid}`,
        kind: "directory" as const,
        mode: 0o700,
        size: 0,
      },
      {
        path: `spaces/${uid}/meta.json`,
        kind: "file" as const,
        mode: 0o600,
        size: 20,
      },
      {
        path: `spaces/.ugoite-space-slug-claims/${fixture.slug}.json`,
        kind: "file" as const,
        mode: 0o600,
        size: 100,
      },
      {
        path: `spaces/.ugoite-space-slug-claims/${fixture.slug}.committed`,
        kind: "file" as const,
        mode: 0o600,
        size: 100,
      },
      {
        path: `spaces/.ugoite-space-slug-claims/${fixture.slug}.lock`,
        kind: "file" as const,
        mode: 0o600,
        size: 0,
      },
      {
        path: `.cp1-profiles/${fixture.slug}.json`,
        kind: "file" as const,
        mode: 0o600,
        size: 10,
      },
      {
        path: `.cp1-profiles/${fixture.slug}.time.txt`,
        kind: "file" as const,
        mode: 0o600,
        size: 10,
      },
    );
  }
  return { members, readbacks, fixtures };
}

Deno.test("CP1 fixture manifests bind exact fixture specifications and verified readbacks", () => {
  for (const kind of ["query", "export"] as const) {
    parseFixtureManifest(makeManifest(kind), kind);
  }
});

Deno.test("CP1 fixture manifests reject unknown schema, malformed identity, and count drift", () => {
  const unsupported = makeManifest("query");
  unsupported.schema_version = 9;
  assertThrows(
    () => parseFixtureManifest(unsupported, "query"),
    Error,
    "unsupported",
  );

  const malformedSha = makeManifest("query");
  malformedSha.source_sha = "not-a-commit";
  assertThrows(
    () => parseFixtureManifest(malformedSha, "query"),
    Error,
    "source SHA",
  );

  const changedCount = makeManifest("query");
  changedCount.fixtures[0]!.verified_entry_count -= 1;
  assertThrows(
    () => parseFixtureManifest(changedCount, "query"),
    Error,
    "verified count",
  );

  const changedOwner = makeManifest("query");
  changedOwner.fixtures[0]!.canonical_readback.owner.authorized_read_verified =
    false;
  assertThrows(
    () => parseFixtureManifest(changedOwner, "query"),
    Error,
    "persisted owner",
  );
});

Deno.test("CP1 fixture archive member validation permits only expected Spaces and profiles", () => {
  const { members, readbacks, fixtures } = makeArchiveMembers("query");
  validateCp1ArchiveMembers(members, fixtures, readbacks);
});

function makeSeedProfile(fixture: Cp1Fixture, batchEntries: unknown[]) {
  return {
    schema_version: 1,
    space_slug: fixture.slug,
    scenario: fixture.scenario,
    seed: fixture.seed,
    entry_count: fixture.entryCount,
    form_count: fixture.formNames.length,
    space_creation_micros: 2,
    owner_initialization_micros: fixture.ownerDisplayName === null ? null : 3,
    form_upsert_micros: 4,
    markdown_render_micros: 5,
    markdown_render_count: 7,
    draft_conversion_micros: 6,
    draft_conversion_count: 8,
    mutation_batch_micros: batchEntries.map(() => 9),
    mutation_batch_entry_counts: batchEntries,
    total_wall_micros: 50,
  };
}

function validSeedBatches(entryCount: number): number[] {
  const batches: number[] = [];
  let remaining = entryCount;
  while (remaining > 0) {
    const next = Math.min(CP1_SEED_MUTATION_BATCH_LIMIT, remaining);
    batches.push(next);
    remaining -= next;
  }
  return batches;
}

Deno.test("CP1 seed profiles accept fully batched entry distributions", () => {
  for (const kind of ["query", "export"] as const) {
    for (const fixture of cp1FixturesFor(kind)) {
      const batches = validSeedBatches(fixture.entryCount);
      assert(
        batches.every((count) =>
          Number.isSafeInteger(count) && count > 0 &&
          count <= CP1_SEED_MUTATION_BATCH_LIMIT
        ),
      );
      assertEquals(
        batches.reduce((sum, count) => sum + count, 0),
        fixture.entryCount,
      );
      validateSeedProfileDocument(makeSeedProfile(fixture, batches), fixture);
    }
  }
});

Deno.test("CP1 seed profiles reject invalid or compensating batch entry counts", () => {
  const fixture = cp1FixtureBySlug("query-space-b");
  const total = fixture.entryCount;
  assert(total > CP1_SEED_MUTATION_BATCH_LIMIT);
  // A filler that sums exactly to the fixture total; appending a zero keeps
  // the sum but must still be rejected because batch counts start at 1.
  const filler = [...validSeedBatches(total - 50), 50];
  assertEquals(
    filler.reduce((sum, count) => sum + count, 0),
    total,
  );
  const cases: Array<[string, unknown[]]> = [
    ["string count", [...filler.slice(0, -1), "50"]],
    ["compensating string count", [...validSeedBatches(total), "stale"]],
    ["fractional count", [128.5, 127.5, ...validSeedBatches(total - 256)]],
    [
      "compensating fractional count",
      [...filler.slice(0, -1), 49.5],
    ],
    ["negative count", [...validSeedBatches(total - 200), 210, -10]],
    ["compensating zero count", [...filler, 0]],
    ["over-limit count", [257, ...validSeedBatches(total - 257)]],
    ["unsafe integer count", [
      ...validSeedBatches(total),
      Number.MAX_SAFE_INTEGER + 1,
    ]],
    ["short total", [100]],
  ];
  for (const [name, batchEntries] of cases) {
    assertThrows(
      () =>
        validateSeedProfileDocument(
          makeSeedProfile(fixture, batchEntries),
          fixture,
        ),
      Error,
      "incomplete or mismatched",
      `seed profile must reject ${name}`,
    );
  }
  assertThrows(
    () =>
      validateSeedProfileDocument(
        {
          ...makeSeedProfile(fixture, validSeedBatches(total)),
          seed: fixture.seed + 1,
        },
        fixture,
      ),
    Error,
    "incomplete or mismatched",
  );
});

Deno.test("CP1 fixture loads bind local manifests to the expected producer run ID", () => {
  assertFixtureRunBinding("123456789", "123456789");
  assertFixtureRunBinding(
    "local-20260929T000000Z-42",
    "local-20260929T000000Z-42",
  );
  assertThrows(
    () => assertFixtureRunBinding(null, "local-20260929T000000Z-42"),
    Error,
    "requires the current CI run ID",
  );
  assertThrows(
    () => assertFixtureRunBinding(null, "123456789"),
    Error,
    "requires the current CI run ID",
  );
  assertThrows(
    () => assertFixtureRunBinding("999", "123456789"),
    Error,
    "does not match workflow run",
  );
  assertThrows(
    () => assertFixtureRunBinding("local-aaa", "local-bbb"),
    Error,
    "does not match workflow run",
  );
});

Deno.test("CP1 fixture tree accepts only known Node-local metadata outside bundled Spaces", () => {
  const { fixtures, readbacks } = makeArchiveMembers("query");
  const uids = new Set(readbacks.map((readback) => readback.space_uid));
  const members = [
    "spaces",
    "spaces/.ugoite-atomic-writes",
    "spaces/.ugoite-space-slug-claims",
    "_ugoite",
    "_ugoite/space-bindings",
    "_ugoite/space-patches",
    ".cp1-profiles",
    ...fixtures.flatMap((fixture, index) => [
      `spaces/${readbacks[index]!.space_uid}`,
      `spaces/.ugoite-space-slug-claims/${fixture.slug}.json`,
      `spaces/.ugoite-space-slug-claims/${fixture.slug}.committed`,
      `spaces/.ugoite-space-slug-claims/${fixture.slug}.lock`,
      `_ugoite/space-bindings/${readbacks[index]!.space_uid}.json`,
      `_ugoite/space-patches/${readbacks[index]!.space_uid}.lock`,
      `.cp1-profiles/${fixture.slug}.json`,
      `.cp1-profiles/${fixture.slug}.time.txt`,
    ]),
  ].map((relativePath) => ({
    path: `/fixture/${relativePath}`,
    relativePath,
    isDirectory: !relativePath.endsWith(".json") &&
      !relativePath.endsWith(".committed") &&
      !relativePath.endsWith(".lock") &&
      !relativePath.endsWith(".time.txt"),
    mode: 0o600,
  }));
  validateProducerTree(members, fixtures, uids);
  assertThrows(
    () =>
      validateProducerTree(
        [...members, {
          path: "/fixture/_ugoite/other.json",
          relativePath: "_ugoite/other.json",
          isDirectory: false,
          mode: 0o600,
        }],
        fixtures,
        uids,
      ),
    Error,
    "unexpected path",
  );
});

Deno.test("CP1 fixture archive member validation rejects traversal, extra paths, and broad modes", () => {
  const { members, readbacks, fixtures } = makeArchiveMembers("query");
  assertThrows(
    () =>
      validateCp1ArchiveMembers(
        [{ ...members[2]!, path: "../escape" }, ...members.slice(1)],
        fixtures,
        readbacks,
      ),
    Error,
    "unsafe",
  );
  assertThrows(
    () =>
      validateCp1ArchiveMembers(
        [{
          ...members[2]!,
          path: "spaces/018f0000-0000-7abc-8000-000000000000/extra",
        }, ...members.slice(1)],
        fixtures,
        readbacks,
      ),
    Error,
    "unexpected path",
  );
  assertThrows(
    () =>
      validateCp1ArchiveMembers(
        [{ ...members[2]!, mode: 0o755 }, ...members.slice(1)],
        fixtures,
        readbacks,
      ),
    Error,
    "unsafe directory",
  );
});

function writeTarHeader(
  name: string,
  type: string,
  mode: number,
  size: number,
): Uint8Array {
  const header = new Uint8Array(512);
  const text = (offset: number, length: number, value: string) => {
    new TextEncoder().encode(value).forEach((byte, index) => {
      if (index < length) header[offset + index] = byte;
    });
  };
  const octal = (offset: number, length: number, value: number) => {
    text(offset, length, `${value.toString(8).padStart(length - 1, "0")}\0`);
  };
  text(0, 100, name);
  octal(100, 8, mode);
  octal(108, 8, 0);
  octal(116, 8, 0);
  octal(124, 12, size);
  octal(136, 12, 0);
  header.fill(32, 148, 156);
  text(156, 1, type);
  text(257, 6, "ustar\0");
  text(263, 2, "00");
  const checksum = header.reduce((sum, byte) => sum + byte, 0);
  text(148, 8, `${checksum.toString(8).padStart(6, "0")}\0 `);
  return header;
}

async function writeTarGzip(
  path: string,
  name: string,
  type: string,
): Promise<void> {
  const content = new TextEncoder().encode("{}");
  const header = writeTarHeader(name, type, 0o600, content.length);
  const payload = new Uint8Array(512 * 3);
  payload.set(header, 0);
  payload.set(content, 512);
  const compressed = new Blob([payload]).stream().pipeThrough(
    new CompressionStream("gzip"),
  );
  await Deno.writeFile(
    path,
    new Uint8Array(await new Response(compressed).arrayBuffer()),
  );
}

function paxRecord(key: string, value: string): Uint8Array {
  const encoder = new TextEncoder();
  const suffix = ` ${key}=${value}\n`;
  let length = encoder.encode(suffix).byteLength + 1;
  while (true) {
    const record = encoder.encode(`${length}${suffix}`);
    if (record.byteLength === length) return record;
    length = record.byteLength;
  }
}

async function writePaxTarGzip(
  path: string,
  memberPath: string,
  paxHeaderPath = "./PaxHeaders/entry",
): Promise<void> {
  const encoder = new TextEncoder();
  const attributes = new Uint8Array([
    ...paxRecord("path", memberPath),
    ...paxRecord("mtime", "1.0"),
  ]);
  const payload = encoder.encode("{}");
  const paxHeader = writeTarHeader(
    paxHeaderPath,
    "x",
    0o600,
    attributes.length,
  );
  const fileHeader = writeTarHeader(
    "PaxFiles/entry",
    "0",
    0o600,
    payload.length,
  );
  const tarBytes = new Uint8Array(
    512 + Math.ceil(attributes.length / 512) * 512 + 512 + 512 + 1024,
  );
  let offset = 0;
  tarBytes.set(paxHeader, offset);
  offset += 512;
  tarBytes.set(attributes, offset);
  offset += Math.ceil(attributes.length / 512) * 512;
  tarBytes.set(fileHeader, offset);
  offset += 512;
  tarBytes.set(payload, offset);
  const compressed = new Blob([tarBytes]).stream().pipeThrough(
    new CompressionStream("gzip"),
  );
  await Deno.writeFile(
    path,
    new Uint8Array(await new Response(compressed).arrayBuffer()),
  );
}

Deno.test("CP1 tar reader rejects traversal and links and resolves safe PAX paths", async () => {
  const dir = await Deno.makeTempDir({ prefix: "ugoite-cp1-tar-" });
  try {
    const traversal = `${dir}/traversal.tar.gz`;
    await writeTarGzip(traversal, "../escape", "0");
    await assertRejects(() => readTarMembers(traversal), Error, "unsafe");
    const link = `${dir}/link.tar.gz`;
    await writeTarGzip(link, "spaces/link", "2");
    await assertRejects(() => readTarMembers(link), Error, "link or special");

    const longPath = `spaces/${"long-長-component-".repeat(6)}entry.json`;
    const pax = `${dir}/pax.tar.gz`;
    await writePaxTarGzip(pax, longPath);
    assertEquals(
      (await readTarMembers(pax)).map((member) => member.path),
      [longPath],
    );

    const nestedPax = `${dir}/nested-pax.tar.gz`;
    await writePaxTarGzip(
      nestedPax,
      longPath,
      "spaces/PaxHeaders/.ugoite-space-slug-claims",
    );
    assertEquals(
      (await readTarMembers(nestedPax)).map((member) => member.path),
      [longPath],
    );

    const libarchivePax = `${dir}/libarchive-pax.tar.gz`;
    await writePaxTarGzip(
      libarchivePax,
      longPath,
      "PaxHeader/spaces",
    );
    assertEquals(
      (await readTarMembers(libarchivePax)).map((member) => member.path),
      [longPath],
    );

    const posixPaxMarker = `${dir}/posix-pax-marker.tar.gz`;
    await writePaxTarGzip(
      posixPaxMarker,
      longPath,
      "spaces/metadata/Pax",
    );
    assertEquals(
      (await readTarMembers(posixPaxMarker)).map((member) => member.path),
      [longPath],
    );

    const gnuPaxMarker = `${dir}/gnu-pax-marker.tar.gz`;
    await writePaxTarGzip(
      gnuPaxMarker,
      longPath,
      "spaces/01a0eb2b-5500-7551-82e3-ba6f7ca42bf7/forms/form_01a0eb2b55247245856b4d9e7ce341eb/data/PaxHead",
    );
    assertEquals(
      (await readTarMembers(gnuPaxMarker)).map((member) => member.path),
      [longPath],
    );

    const paxTraversal = `${dir}/pax-traversal.tar.gz`;
    await writePaxTarGzip(paxTraversal, "../escape");
    await assertRejects(
      () => readTarMembers(paxTraversal),
      Error,
      "unsafe",
    );

    const paxHeaderTraversal = `${dir}/pax-header-traversal.tar.gz`;
    await writePaxTarGzip(
      paxHeaderTraversal,
      "spaces/entry.json",
      "spaces/PaxHeaders/../escape",
    );
    await assertRejects(
      () => readTarMembers(paxHeaderTraversal),
      Error,
      "unsafe",
    );

    const unrecognizedPaxMarker = `${dir}/unrecognized-pax-marker.tar.gz`;
    await writePaxTarGzip(
      unrecognizedPaxMarker,
      "spaces/entry.json",
      "spaces/metadata/PaxHeadx",
    );
    await assertRejects(
      () => readTarMembers(unrecognizedPaxMarker),
      Error,
      "unsafe",
    );
  } finally {
    await Deno.remove(dir, { recursive: true });
  }
});

Deno.test({
  name: "CP1 tar reader round-trips native POSIX tar metadata headers",
  ignore: Deno.build.os === "windows",
  fn: async () => {
    const dir = await Deno.makeTempDir({ prefix: "ugoite-cp1-native-tar-" });
    try {
      const source = `${dir}/source`;
      const archive = `${dir}/native.tar.gz`;
      const destination = `${dir}/destination`;
      await Deno.mkdir(`${source}/spaces`, { recursive: true });
      await Deno.writeTextFile(`${source}/spaces/short.txt`, "fixture\n");
      await Deno.mkdir(destination);
      const tarArgs = [
        "-czf",
        archive,
        "--format=posix",
        "--no-xattrs",
      ];
      if (Deno.build.os === "darwin") tarArgs.push("--no-mac-metadata");
      tarArgs.push("-C", source, "spaces");
      const created = await new Deno.Command("tar", {
        args: tarArgs,
        stdout: "piped",
        stderr: "piped",
      }).output();
      assert(
        created.success,
        new TextDecoder().decode(created.stderr),
      );
      assertEquals(
        (await readTarMembers(archive)).map((member) => member.path),
        ["spaces", "spaces/short.txt"],
      );

      const extracted = await new Deno.Command("tar", {
        args: [
          "-xzf",
          archive,
          "-C",
          destination,
          "--no-same-owner",
          "--no-same-permissions",
        ],
        stdout: "piped",
        stderr: "piped",
      }).output();
      assert(
        extracted.success,
        new TextDecoder().decode(extracted.stderr),
      );
      assertEquals(
        await Deno.readTextFile(`${destination}/spaces/short.txt`),
        "fixture\n",
      );
      const extractedPaths: string[] = [];
      for await (const entry of Deno.readDir(`${destination}/spaces`)) {
        extractedPaths.push(entry.name);
      }
      assertEquals(extractedPaths, ["short.txt"]);
    } finally {
      await Deno.remove(dir, { recursive: true });
    }
  },
});
