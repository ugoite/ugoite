import {
  type Cp1Fixture,
  type Cp1FixtureSet,
  cp1FixturesFor,
} from "./cp1_fixture_spec.ts";
import { containedArtifactPath, sha256File } from "./ci_file_security.ts";
import { createHash } from "node:crypto";

const MANIFEST_SCHEMA_VERSION = 1;
const MEASUREMENT_SCHEMA_VERSION = 1;
const OWNER_MARKER = "ugoite-cp1-fixture-bundle-v1\n";
const SPACE_UID =
  /^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;
const SOURCE_SHA = /^[0-9a-f]{40}$/i;
const SHA_256 = /^[0-9a-f]{64}$/i;
const PROFILE_ROOT = ".cp1-profiles";
const ARCHIVE_NAMES: Record<Cp1FixtureSet, string> = {
  query: "query-fixtures.tar.gz",
  export: "export-fixtures.tar.gz",
};

type VerificationReport = Record<string, unknown> & {
  schema_version: number;
  space_slug: string;
  space_uid: string;
  scenario: string;
  entry_count: number;
  form_names: string[];
  form_entry_counts: Record<string, number>;
  owner: Record<string, unknown>;
};

type FixtureManifestItem = {
  slug: string;
  scenario: string;
  seed: number;
  owner_mode: "owner" | "none";
  owner_display_name: string | null;
  space_uid: string;
  expected_entry_count: number;
  verified_entry_count: number;
  form_names: string[];
  form_entry_counts: Record<string, number>;
  canonical_readback: VerificationReport;
};

type FixtureManifest = {
  schema_version: number;
  source_sha: string;
  ci_run_id: string;
  fixture_kind: Cp1FixtureSet;
  fixture_spec_schema_version: number;
  measurement_schema_version: number;
  generator_fingerprint: string;
  archive: {
    path: string;
    sha256: string;
    size: number;
    packaging_micros: number;
  };
  seed_profiles: Array<{
    slug: string;
    profile_path: string;
    resource_path: string;
    schema_version: number;
  }>;
  fixtures: FixtureManifestItem[];
};

type TarMember = {
  path: string;
  kind: "file" | "directory";
  mode: number;
  size: number;
};

async function main(): Promise<void> {
  const [operation, fixtureKind, ...rawOptions] = Deno.args;
  if (
    (operation !== "create" && operation !== "load") ||
    (fixtureKind !== "query" && fixtureKind !== "export")
  ) {
    throw new Error(
      "usage: cp1_fixture_bundle.ts <create|load> <query|export> --name value ...",
    );
  }
  const options = parseOptions(rawOptions);
  const checkoutSha = await gitHead();
  const sourceSha = options.get("source-sha") ??
    Deno.env.get("UGOITE_SOURCE_SHA")?.trim() ?? checkoutSha;
  if (!SOURCE_SHA.test(sourceSha) || sourceSha !== checkoutSha) {
    throw new Error("CP1 fixture source SHA must match the checked out commit");
  }
  const expectedRunId = options.get("run-id") ??
    Deno.env.get("UGOITE_CI_RUN_ID")?.trim() ??
    Deno.env.get("GITHUB_RUN_ID")?.trim() ?? "";

  if (operation === "create") {
    const root = required(options, "root");
    const output = required(options, "out");
    const xtask = await verifiedXtask(options.get("xtask"), sourceSha);
    const runId = expectedRunId || `local-${Date.now()}-${Deno.pid}`;
    const manifest = await createFixtureBundle(
      fixtureKind,
      root,
      output,
      xtask,
      sourceSha,
      runId,
    );
    console.log(JSON.stringify({
      fixture_kind: fixtureKind,
      source_sha: sourceSha,
      ci_run_id: runId,
      archive_size: manifest.archive.size,
      archive_sha256: manifest.archive.sha256,
      packaging_micros: manifest.archive.packaging_micros,
    }));
    return;
  }

  const bundleDir = required(options, "bundle-dir");
  const destination = required(options, "destination");
  const xtask = await verifiedXtask(options.get("xtask"), sourceSha);
  const loaded = await loadFixtureBundle(
    fixtureKind,
    bundleDir,
    destination,
    xtask,
    sourceSha,
    expectedRunId || null,
  );
  console.log(JSON.stringify({
    fixture_kind: fixtureKind,
    source_sha: sourceSha,
    ci_run_id: loaded.ci_run_id,
    destination: await Deno.realPath(destination),
    verified_space_uids: loaded.fixtures.map((fixture) => fixture.space_uid),
  }));
}

function parseOptions(args: string[]): Map<string, string> {
  const options = new Map<string, string>();
  for (let index = 0; index < args.length; index += 2) {
    const key = args[index];
    const value = args[index + 1];
    if (!key?.startsWith("--") || !value || value.startsWith("--")) {
      throw new Error(`expected --name value, got ${key ?? "<end>"}`);
    }
    const name = key.slice(2);
    if (options.has(name)) throw new Error(`duplicate option --${name}`);
    options.set(name, value);
  }
  return options;
}

function required(options: Map<string, string>, name: string): string {
  const value = options.get(name)?.trim();
  if (!value) throw new Error(`--${name} is required`);
  return value;
}

async function createFixtureBundle(
  kind: Cp1FixtureSet,
  rootInput: string,
  outputInput: string,
  xtask: string,
  sourceSha: string,
  runId: string,
): Promise<FixtureManifest> {
  validateRunId(runId);
  const root = await Deno.realPath(rootInput);
  const fixtures = cp1FixturesFor(kind);
  const readbacks: VerificationReport[] = [];
  for (const fixture of fixtures) {
    readbacks.push(await verifyFixture(xtask, root, fixture));
  }

  const rootMembers = await walkRegularTree(root);
  const uidBySlug = new Map(readbacks.map((readback) => [
    readback.space_slug,
    readback.space_uid,
  ]));
  const expectedSpaceUids = new Set(uidBySlug.values());
  if (expectedSpaceUids.size !== fixtures.length) {
    throw new Error("CP1 fixture Spaces must have distinct immutable UIDs");
  }
  validateProducerTree(rootMembers, fixtures, expectedSpaceUids);
  await validateSeedProfiles(root, fixtures);

  for (const member of rootMembers) {
    await Deno.chmod(member.path, member.isDirectory ? 0o700 : 0o600);
  }
  await validateSlugClaims(root, fixtures, readbacks);

  const output = await prepareOwnedOutput(outputInput);
  const archiveName = ARCHIVE_NAMES[kind];
  const archivePath = `${output}/${archiveName}`;
  const archiveMembers = [
    "spaces",
    PROFILE_ROOT,
  ];
  const packagingStarted = performance.now();
  const tarArgs = [
    "-czf",
    archivePath,
    "--format=posix",
  ];
  if (Deno.build.os === "darwin") {
    tarArgs.push("--no-xattrs", "--no-mac-metadata");
  } else {
    tarArgs.push("--no-xattrs");
  }
  tarArgs.push(
    "-C",
    root,
    ...archiveMembers,
  );
  await run("tar", tarArgs);
  const packagingMicros = Math.round(
    (performance.now() - packagingStarted) * 1000,
  );
  const archiveIdentity = await sha256File(archivePath);
  const members = await readTarMembers(archivePath);
  validateCp1ArchiveMembers(members, fixtures, readbacks);

  const fixtureItems = fixtures.map((fixture, index): FixtureManifestItem => {
    const report = readbacks[index];
    if (!report) throw new Error(`missing readback for ${fixture.slug}`);
    return {
      slug: fixture.slug,
      scenario: fixture.scenario,
      seed: fixture.seed,
      owner_mode: fixture.ownerDisplayName === null ? "none" : "owner",
      owner_display_name: fixture.ownerDisplayName,
      space_uid: report.space_uid,
      expected_entry_count: fixture.entryCount,
      verified_entry_count: report.entry_count,
      form_names: [...report.form_names],
      form_entry_counts: report.form_entry_counts,
      canonical_readback: report,
    };
  });
  const manifest: FixtureManifest = {
    schema_version: MANIFEST_SCHEMA_VERSION,
    source_sha: sourceSha,
    ci_run_id: runId,
    fixture_kind: kind,
    fixture_spec_schema_version: 1,
    measurement_schema_version: MEASUREMENT_SCHEMA_VERSION,
    generator_fingerprint: await generatorFingerprint(),
    archive: {
      path: archiveName,
      sha256: archiveIdentity.sha256,
      size: archiveIdentity.size,
      packaging_micros: packagingMicros,
    },
    seed_profiles: fixtures.map((fixture) => ({
      slug: fixture.slug,
      profile_path: `${PROFILE_ROOT}/${fixture.slug}.json`,
      resource_path: `${PROFILE_ROOT}/${fixture.slug}.time.txt`,
      schema_version: 1,
    })),
    fixtures: fixtureItems,
  };
  await Deno.writeTextFile(
    `${output}/manifest.json`,
    `${JSON.stringify(manifest, null, 2)}\n`,
    { createNew: true },
  );
  return manifest;
}

async function loadFixtureBundle(
  kind: Cp1FixtureSet,
  bundleDirInput: string,
  destinationInput: string,
  xtask: string,
  sourceSha: string,
  expectedRunId: string | null,
): Promise<FixtureManifest> {
  const bundleDir = await Deno.realPath(bundleDirInput);
  const manifestPath = await containedArtifactPath(bundleDir, "manifest.json");
  const manifest = parseFixtureManifest(
    JSON.parse(await Deno.readTextFile(manifestPath)),
    kind,
  );
  if (manifest.source_sha !== sourceSha) {
    throw new Error("CP1 fixture manifest source SHA does not match checkout");
  }
  if (manifest.generator_fingerprint !== await generatorFingerprint()) {
    throw new Error(
      "CP1 fixture generator fingerprint does not match checkout",
    );
  }
  if (expectedRunId !== null && manifest.ci_run_id !== expectedRunId) {
    throw new Error("CP1 fixture manifest run ID does not match workflow run");
  }
  if (expectedRunId === null && !manifest.ci_run_id.startsWith("local-")) {
    throw new Error("CP1 fixture load requires the current CI run ID");
  }
  const archivePath = await containedArtifactPath(
    bundleDir,
    manifest.archive.path,
  );
  const archiveIdentity = await sha256File(archivePath);
  if (archiveIdentity.sha256 !== manifest.archive.sha256) {
    throw new Error("CP1 fixture archive checksum mismatch");
  }
  if (archiveIdentity.size !== manifest.archive.size) {
    throw new Error("CP1 fixture archive size mismatch");
  }
  const members = await readTarMembers(archivePath);
  validateCp1ArchiveMembers(
    members,
    cp1FixturesFor(kind),
    manifest.fixtures.map((item) => item.canonical_readback),
  );

  const destination = await prepareEmptyDestination(destinationInput);
  try {
    await run("tar", [
      "-xzf",
      archivePath,
      "-C",
      destination,
      "--no-same-owner",
      "--no-same-permissions",
    ]);
    const extractedMembers = await walkRegularTree(destination);
    for (const member of extractedMembers) {
      if ((member.mode & 0o077) !== 0 || (member.mode & 0o7000) !== 0) {
        throw new Error(
          `extracted CP1 fixture has unsafe permissions: ${member.relativePath}`,
        );
      }
    }
    validateProducerTree(
      extractedMembers,
      cp1FixturesFor(kind),
      new Set(manifest.fixtures.map((fixture) => fixture.space_uid)),
    );
    await validateSeedProfiles(destination, cp1FixturesFor(kind));
    await validateSlugClaims(
      destination,
      cp1FixturesFor(kind),
      manifest.fixtures.map((fixture) => fixture.canonical_readback),
    );
    for (const fixture of cp1FixturesFor(kind)) {
      const actual = await verifyFixture(xtask, destination, fixture);
      const expected = manifest.fixtures.find((item) =>
        item.slug === fixture.slug
      );
      if (
        !expected ||
        canonicalJson(actual) !== canonicalJson(expected.canonical_readback)
      ) {
        throw new Error(
          `restored CP1 fixture canonical readback differs for ${fixture.slug}`,
        );
      }
    }
  } catch (error) {
    for (const name of ["spaces", PROFILE_ROOT]) {
      await Deno.remove(`${destination}/${name}`, { recursive: true }).catch(
        () => {},
      );
    }
    throw error;
  }
  return manifest;
}

export function parseFixtureManifest(
  value: unknown,
  kind: Cp1FixtureSet,
): FixtureManifest {
  const manifest = asRecord(value, "CP1 fixture manifest");
  requireKeys(manifest, [
    "schema_version",
    "source_sha",
    "ci_run_id",
    "fixture_kind",
    "fixture_spec_schema_version",
    "measurement_schema_version",
    "generator_fingerprint",
    "archive",
    "seed_profiles",
    "fixtures",
  ], "CP1 fixture manifest");
  if (manifest.schema_version !== MANIFEST_SCHEMA_VERSION) {
    throw new Error(
      `unsupported CP1 fixture manifest schema ${manifest.schema_version}`,
    );
  }
  if (manifest.fixture_kind !== kind) {
    throw new Error(
      "CP1 fixture manifest kind does not match requested consumer",
    );
  }
  if (
    manifest.fixture_spec_schema_version !== 1 ||
    manifest.measurement_schema_version !== MEASUREMENT_SCHEMA_VERSION
  ) {
    throw new Error("unsupported CP1 fixture spec or measurement schema");
  }
  if (
    typeof manifest.source_sha !== "string" ||
    !SOURCE_SHA.test(manifest.source_sha)
  ) {
    throw new Error("CP1 fixture manifest has an invalid source SHA");
  }
  if (typeof manifest.ci_run_id !== "string") {
    throw new Error("CP1 fixture manifest has no run ID");
  }
  validateRunId(manifest.ci_run_id);
  if (
    typeof manifest.generator_fingerprint !== "string" ||
    !SHA_256.test(manifest.generator_fingerprint)
  ) {
    throw new Error(
      "CP1 fixture manifest has an invalid generator fingerprint",
    );
  }
  const archive = asRecord(manifest.archive, "CP1 fixture archive");
  requireKeys(
    archive,
    ["path", "sha256", "size", "packaging_micros"],
    "CP1 fixture archive",
  );
  if (archive.path !== ARCHIVE_NAMES[kind]) {
    throw new Error("CP1 fixture manifest names an unexpected archive");
  }
  if (typeof archive.sha256 !== "string" || !SHA_256.test(archive.sha256)) {
    throw new Error("CP1 fixture archive digest is invalid");
  }
  if (!Number.isSafeInteger(archive.size) || (archive.size as number) <= 0) {
    throw new Error("CP1 fixture archive size is invalid");
  }
  if (
    !Number.isSafeInteger(archive.packaging_micros) ||
    (archive.packaging_micros as number) < 0
  ) throw new Error("CP1 fixture packaging duration is invalid");

  const fixtures = cp1FixturesFor(kind);
  if (
    !Array.isArray(manifest.fixtures) ||
    manifest.fixtures.length !== fixtures.length
  ) {
    throw new Error("CP1 fixture manifest has the wrong fixture count");
  }
  const fixtureItems = manifest.fixtures.map((item, index) =>
    validateFixtureItem(item, fixtures[index]!, kind)
  );
  const uidSet = new Set(fixtureItems.map((fixture) => fixture.space_uid));
  if (uidSet.size !== fixtures.length) {
    throw new Error("CP1 fixture Space UIDs are duplicated");
  }

  if (
    !Array.isArray(manifest.seed_profiles) ||
    manifest.seed_profiles.length !== fixtures.length
  ) {
    throw new Error("CP1 fixture manifest has an incomplete seed profile list");
  }
  for (let index = 0; index < fixtures.length; index++) {
    const fixture = fixtures[index]!;
    const profile = asRecord(
      manifest.seed_profiles[index],
      "CP1 seed profile descriptor",
    );
    requireKeys(profile, [
      "slug",
      "profile_path",
      "resource_path",
      "schema_version",
    ], "CP1 seed profile descriptor");
    if (
      profile.slug !== fixture.slug ||
      profile.profile_path !== `${PROFILE_ROOT}/${fixture.slug}.json` ||
      profile.resource_path !== `${PROFILE_ROOT}/${fixture.slug}.time.txt` ||
      profile.schema_version !== 1
    ) {
      throw new Error(
        `CP1 seed profile descriptor is invalid for ${fixture.slug}`,
      );
    }
  }
  return manifest as unknown as FixtureManifest;
}

function validateFixtureItem(
  value: unknown,
  fixture: Cp1Fixture,
  kind: Cp1FixtureSet,
): FixtureManifestItem {
  const item = asRecord(value, `fixture manifest entry ${fixture.slug}`);
  requireKeys(item, [
    "slug",
    "scenario",
    "seed",
    "owner_mode",
    "owner_display_name",
    "space_uid",
    "expected_entry_count",
    "verified_entry_count",
    "form_names",
    "form_entry_counts",
    "canonical_readback",
  ], `fixture manifest entry ${fixture.slug}`);
  const ownerMode = fixture.ownerDisplayName === null ? "none" : "owner";
  if (
    item.slug !== fixture.slug || item.scenario !== fixture.scenario ||
    item.seed !== fixture.seed ||
    item.owner_mode !== ownerMode ||
    item.owner_display_name !== fixture.ownerDisplayName ||
    item.expected_entry_count !== fixture.entryCount ||
    item.verified_entry_count !== fixture.entryCount
  ) {
    throw new Error(
      `CP1 fixture identity or verified count mismatch for ${fixture.slug}`,
    );
  }
  if (typeof item.space_uid !== "string" || !SPACE_UID.test(item.space_uid)) {
    throw new Error(
      `CP1 fixture has an invalid UUIDv7 Space UID for ${fixture.slug}`,
    );
  }
  const names = item.form_names;
  if (
    !Array.isArray(names) ||
    canonicalJson(names) !== canonicalJson([...fixture.formNames].sort())
  ) {
    throw new Error(`CP1 fixture Form set mismatch for ${fixture.slug}`);
  }
  const counts = asRecord(
    item.form_entry_counts,
    `Form counts for ${fixture.slug}`,
  );
  requireKeys(
    counts,
    [...fixture.formNames].sort(),
    `Form counts for ${fixture.slug}`,
  );
  let total = 0;
  for (const formName of fixture.formNames) {
    const count = counts[formName];
    if (
      !Number.isSafeInteger(count) || (count as number) < 0 ||
      (formName !== "Entry" && count === 0)
    ) {
      throw new Error(
        `CP1 fixture has an invalid Entry count for Form ${formName}`,
      );
    }
    total += count as number;
  }
  if (total !== fixture.entryCount) {
    throw new Error(
      `CP1 fixture Form distribution does not add up for ${fixture.slug}`,
    );
  }
  const readback = asRecord(
    item.canonical_readback,
    `canonical readback for ${fixture.slug}`,
  ) as VerificationReport;
  validateReadback(readback, fixture, item.space_uid);
  if (canonicalJson(readback.form_entry_counts) !== canonicalJson(counts)) {
    throw new Error(
      `CP1 fixture manifest Form counts disagree with readback for ${fixture.slug}`,
    );
  }
  return item as unknown as FixtureManifestItem;
}

function validateReadback(
  report: VerificationReport,
  fixture: Cp1Fixture,
  spaceUid: unknown,
): void {
  if (
    report.schema_version !== 1 || report.space_slug !== fixture.slug ||
    report.space_uid !== spaceUid ||
    report.scenario !== fixture.scenario ||
    report.entry_count !== fixture.entryCount ||
    canonicalJson(report.form_names) !==
      canonicalJson([...fixture.formNames].sort())
  ) {
    throw new Error(
      `CP1 fixture canonical readback identity mismatch for ${fixture.slug}`,
    );
  }
  const integrity = asRecord(
    report.integrity,
    `integrity report for ${fixture.slug}`,
  );
  if (
    integrity.deep !== true ||
    (integrity.status !== "valid" &&
      integrity.status !== "valid_with_rebuildable_derived_state") ||
    integrity.changes_and_audit !== "valid"
  ) {
    throw new Error(
      `CP1 fixture integrity evidence is incomplete for ${fixture.slug}`,
    );
  }
  const owner = asRecord(report.owner, `owner report for ${fixture.slug}`);
  if (fixture.ownerDisplayName === null) {
    if (owner.mode !== "none" || integrity.authorization !== "incomplete") {
      throw new Error(
        `ownerless fixture unexpectedly has owner authorization: ${fixture.slug}`,
      );
    }
  } else if (
    owner.mode !== "owner" || owner.display_name !== fixture.ownerDisplayName ||
    owner.role !== "owner" || owner.active !== true ||
    owner.authorized_read_verified !== true ||
    owner.non_owner_denial_verified !== true ||
    integrity.authorization !== "valid"
  ) {
    throw new Error(
      `persisted owner readback is incomplete for ${fixture.slug}`,
    );
  }
}

async function verifyFixture(
  xtask: string,
  root: string,
  fixture: Cp1Fixture,
): Promise<VerificationReport> {
  const args = [
    "verify-seed",
    "--root",
    root,
    "--space-id",
    fixture.slug,
    "--scenario",
    fixture.scenario,
    "--entry-count",
    String(fixture.entryCount),
  ];
  if (fixture.ownerDisplayName !== null) {
    args.push("--owner", fixture.ownerDisplayName);
  }
  for (const formName of fixture.formNames) args.push("--form-name", formName);
  const output = await run(xtask, args);
  let report: VerificationReport;
  try {
    report = JSON.parse(output) as VerificationReport;
  } catch (error) {
    throw new Error(
      `xtask verify-seed returned invalid JSON for ${fixture.slug}`,
      { cause: error },
    );
  }
  validateReadback(report, fixture, report.space_uid);
  if (
    typeof report.space_uid !== "string" || !SPACE_UID.test(report.space_uid)
  ) {
    throw new Error(
      `xtask verify-seed returned an invalid Space UID for ${fixture.slug}`,
    );
  }
  const counts = asRecord(
    report.form_entry_counts,
    `Core API Form distribution for ${fixture.slug}`,
  );
  requireKeys(
    counts,
    [...fixture.formNames].sort(),
    `Core API Form distribution for ${fixture.slug}`,
  );
  if (
    Object.values(counts).reduce<number>(
      (sum, count) => sum + Number(count),
      0,
    ) !== fixture.entryCount
  ) {
    throw new Error(
      `Core API Form distribution count mismatch for ${fixture.slug}`,
    );
  }
  return report;
}

async function validateSeedProfiles(
  root: string,
  fixtures: readonly Cp1Fixture[],
): Promise<void> {
  for (const fixture of fixtures) {
    const profilePath = `${root}/${PROFILE_ROOT}/${fixture.slug}.json`;
    const resourcePath = `${root}/${PROFILE_ROOT}/${fixture.slug}.time.txt`;
    const profileInfo = await Deno.lstat(profilePath);
    const resourceInfo = await Deno.lstat(resourcePath);
    if (
      !profileInfo.isFile || profileInfo.isSymlink || !resourceInfo.isFile ||
      resourceInfo.isSymlink
    ) {
      throw new Error(
        `CP1 seed profile files are not regular files for ${fixture.slug}`,
      );
    }
    const profile = asRecord(
      JSON.parse(await Deno.readTextFile(profilePath)),
      `seed profile for ${fixture.slug}`,
    );
    const batchTimes = profile.mutation_batch_micros;
    const batchEntries = profile.mutation_batch_entry_counts;
    if (
      profile.schema_version !== 1 || profile.space_slug !== fixture.slug ||
      profile.scenario !== fixture.scenario || profile.seed !== fixture.seed ||
      profile.entry_count !== fixture.entryCount ||
      !Number.isSafeInteger(profile.form_count) ||
      !Number.isSafeInteger(profile.space_creation_micros) ||
      !(profile.owner_initialization_micros === null ||
        Number.isSafeInteger(profile.owner_initialization_micros)) ||
      !Number.isSafeInteger(profile.form_upsert_micros) ||
      !Number.isSafeInteger(profile.markdown_render_micros) ||
      !Number.isSafeInteger(profile.markdown_render_count) ||
      !Number.isSafeInteger(profile.draft_conversion_micros) ||
      !Number.isSafeInteger(profile.draft_conversion_count) ||
      !Number.isSafeInteger(profile.total_wall_micros) ||
      !Array.isArray(batchTimes) || !Array.isArray(batchEntries) ||
      batchTimes.length === 0 || batchTimes.length !== batchEntries.length ||
      batchTimes.some((value) => !Number.isSafeInteger(value)) ||
      batchEntries.reduce(
          (sum, value) =>
            sum + (Number.isSafeInteger(value) ? Number(value) : 0),
          0,
        ) !== fixture.entryCount
    ) {
      throw new Error(
        `CP1 seed profile is incomplete or mismatched for ${fixture.slug}`,
      );
    }
    if (resourceInfo.size === 0) {
      throw new Error(`CP1 seed resource profile is empty for ${fixture.slug}`);
    }
  }
}

export function validateProducerTree(
  members: Array<
    { path: string; relativePath: string; isDirectory: boolean; mode: number }
  >,
  fixtures: readonly Cp1Fixture[],
  spaceUids: Set<string>,
): void {
  const profiles = new Set(fixtures.flatMap((fixture) => [
    `${PROFILE_ROOT}/${fixture.slug}.json`,
    `${PROFILE_ROOT}/${fixture.slug}.time.txt`,
  ]));
  const presentSpaces = new Set<string>();
  const presentProfiles = new Set<string>();
  const presentClaims = new Set<string>();
  const expectedClaimFiles = new Set(fixtures.flatMap((fixture) => [
    `spaces/.ugoite-space-slug-claims/${fixture.slug}.json`,
    `spaces/.ugoite-space-slug-claims/${fixture.slug}.committed`,
    `spaces/.ugoite-space-slug-claims/${fixture.slug}.lock`,
  ]));
  const expectedLocalBindingFiles = new Set(
    [...spaceUids].map((uid) => `_ugoite/space-bindings/${uid}.json`),
  );
  const expectedPatchLocks = new Set(
    [...spaceUids].map((uid) => `_ugoite/space-patches/${uid}.lock`),
  );
  let hasSpacesDirectory = false;
  let hasProfileDirectory = false;
  for (const member of members) {
    const path = member.relativePath;
    if (path === "spaces") {
      if (!member.isDirectory) {
        throw new Error("fixture root spaces path is not a directory");
      }
      hasSpacesDirectory = true;
      continue;
    }
    if (path === "spaces/.ugoite-atomic-writes") {
      if (!member.isDirectory) {
        throw new Error(
          "fixture root atomic-write lock path is not a directory",
        );
      }
      continue;
    }
    if (path === "spaces/.ugoite-space-slug-claims") {
      if (!member.isDirectory) {
        throw new Error(
          "fixture root Space slug claims path is not a directory",
        );
      }
      continue;
    }
    if (path.startsWith("spaces/.ugoite-space-slug-claims/")) {
      if (!expectedClaimFiles.has(path) || member.isDirectory) {
        throw new Error(
          `fixture root contains unexpected Space slug claim: ${path}`,
        );
      }
      presentClaims.add(path);
      continue;
    }
    if (path === "spaces" || path.startsWith("spaces/")) {
      const parts = path.split("/");
      if (parts.length >= 2 && spaceUids.has(parts[1]!)) {
        if (parts.length === 2) {
          if (!member.isDirectory) {
            throw new Error(`fixture Space root is not a directory: ${path}`);
          }
          presentSpaces.add(parts[1]!);
        }
      } else if (parts.length > 1) {
        throw new Error(`fixture root contains an unexpected Space: ${path}`);
      }
      continue;
    }
    if (path === "_ugoite" || path === "_ugoite/space-bindings") {
      if (!member.isDirectory) {
        throw new Error(
          `Node-local fixture state path is not a directory: ${path}`,
        );
      }
      continue;
    }
    if (path === "_ugoite/space-patches") {
      if (!member.isDirectory) {
        throw new Error(
          `Node-local fixture state path is not a directory: ${path}`,
        );
      }
      continue;
    }
    if (expectedLocalBindingFiles.has(path) || expectedPatchLocks.has(path)) {
      if (member.isDirectory) {
        throw new Error(`Node-local fixture state path is not a file: ${path}`);
      }
      continue;
    }
    if (path === PROFILE_ROOT) {
      if (!member.isDirectory) {
        throw new Error("fixture profile path is not a directory");
      }
      hasProfileDirectory = true;
      continue;
    }
    if (profiles.has(path) && !member.isDirectory) {
      presentProfiles.add(path);
      continue;
    }
    throw new Error(`fixture root contains an unexpected path: ${path}`);
  }
  if (presentSpaces.size !== spaceUids.size) {
    throw new Error("fixture root is missing an expected Space");
  }
  if (presentProfiles.size !== profiles.size) {
    throw new Error(
      "fixture root is missing a seed profile or resource record",
    );
  }
  if (!hasSpacesDirectory || !hasProfileDirectory) {
    throw new Error("fixture root is missing a required directory");
  }
  const requiredClaims = fixtures.flatMap((fixture) => [
    `spaces/.ugoite-space-slug-claims/${fixture.slug}.json`,
    `spaces/.ugoite-space-slug-claims/${fixture.slug}.committed`,
  ]);
  if (requiredClaims.some((path) => !presentClaims.has(path))) {
    throw new Error("fixture root is missing a committed Space slug claim");
  }
}

async function validateSlugClaims(
  root: string,
  fixtures: readonly Cp1Fixture[],
  readbacks: readonly VerificationReport[],
): Promise<void> {
  const claimsRoot = `${root}/spaces/.ugoite-space-slug-claims`;
  for (const fixture of fixtures) {
    const readback = readbacks.find((item) => item.space_slug === fixture.slug);
    if (!readback) {
      throw new Error(`missing canonical readback for ${fixture.slug}`);
    }
    const claimPath = `${claimsRoot}/${fixture.slug}.json`;
    const markerPath = `${claimsRoot}/${fixture.slug}.committed`;
    const claimText = await Deno.readTextFile(claimPath);
    if (await Deno.readTextFile(markerPath) !== claimText) {
      throw new Error(
        `Space slug commit marker differs from its claim for ${fixture.slug}`,
      );
    }
    const claim = asRecord(
      JSON.parse(claimText),
      `Space slug claim for ${fixture.slug}`,
    );
    if (
      claim.slug !== fixture.slug || claim.space_name !== fixture.slug ||
      claim.space_id !== readback.space_uid || claim.state !== "committed" ||
      typeof claim.claim_id !== "string" || !SPACE_UID.test(claim.claim_id) ||
      typeof claim.created_at !== "string" ||
      typeof claim.heartbeat_at !== "string" ||
      typeof claim.expires_at !== "string"
    ) {
      throw new Error(
        `Space slug claim identity is invalid for ${fixture.slug}`,
      );
    }
    // The sample seeder initializes authorization after Space creation, so
    // owner state is verified from the canonical authorization store above;
    // bootstrap slug claims are not an owner-state authority.
  }
}

export function validateCp1ArchiveMembers(
  members: readonly TarMember[],
  fixtures: readonly Cp1Fixture[],
  readbacks: readonly VerificationReport[],
): void {
  if (members.length === 0) throw new Error("CP1 fixture archive is empty");
  if (readbacks.length !== fixtures.length) {
    throw new Error("CP1 fixture archive validation has incomplete readbacks");
  }
  const expectedUids = new Set(readbacks.map((item) => item.space_uid));
  if (expectedUids.size !== fixtures.length) {
    throw new Error("CP1 fixture Space UIDs are duplicated");
  }
  const seen = new Set<string>();
  const requiredFiles = new Set<string>();
  for (const fixture of fixtures) {
    const readback = readbacks.find((item) => item.space_slug === fixture.slug);
    if (!readback) throw new Error(`missing readback for ${fixture.slug}`);
    requiredFiles.add(`spaces/${readback.space_uid}/meta.json`);
    requiredFiles.add(
      `spaces/.ugoite-space-slug-claims/${fixture.slug}.json`,
    );
    requiredFiles.add(
      `spaces/.ugoite-space-slug-claims/${fixture.slug}.committed`,
    );
    requiredFiles.add(`${PROFILE_ROOT}/${fixture.slug}.json`);
    requiredFiles.add(`${PROFILE_ROOT}/${fixture.slug}.time.txt`);
  }
  for (const member of members) {
    const path = validateTarPath(member.path, member.kind);
    if (seen.has(path)) {
      throw new Error(`CP1 fixture archive has a duplicate member: ${path}`);
    }
    seen.add(path);
    if (member.kind === "directory") {
      if (member.size !== 0 || member.mode !== 0o700) {
        throw new Error(`CP1 fixture archive has an unsafe directory: ${path}`);
      }
    } else if (member.mode !== 0o600) {
      throw new Error(
        `CP1 fixture archive has unsafe file permissions: ${path}`,
      );
    }
    if (path === "spaces") continue;
    if (path === "spaces/.ugoite-atomic-writes") {
      if (member.kind !== "directory") {
        throw new Error(
          "CP1 fixture archive atomic-write lock path is not a directory",
        );
      }
      continue;
    }
    if (path === "spaces/.ugoite-space-slug-claims") {
      if (member.kind !== "directory") {
        throw new Error(
          "CP1 fixture archive slug claims path is not a directory",
        );
      }
      continue;
    }
    if (path.startsWith("spaces/.ugoite-space-slug-claims/")) {
      const expectedClaimFiles = new Set(fixtures.flatMap((fixture) => [
        `spaces/.ugoite-space-slug-claims/${fixture.slug}.json`,
        `spaces/.ugoite-space-slug-claims/${fixture.slug}.committed`,
        `spaces/.ugoite-space-slug-claims/${fixture.slug}.lock`,
      ]));
      if (!expectedClaimFiles.has(path) || member.kind !== "file") {
        throw new Error(
          `CP1 fixture archive has an unexpected slug claim: ${path}`,
        );
      }
      continue;
    }
    if (path === PROFILE_ROOT) continue;
    if (path.startsWith(`${PROFILE_ROOT}/`)) {
      const profile = requiredFiles.has(path);
      if (!profile || member.kind !== "file") {
        throw new Error(
          `CP1 fixture archive has an unexpected profile member: ${path}`,
        );
      }
      continue;
    }
    const parts = path.split("/");
    if (
      parts[0] !== "spaces" || parts.length < 2 || !expectedUids.has(parts[1]!)
    ) {
      throw new Error(
        `CP1 fixture archive contains an unexpected path: ${path}`,
      );
    }
  }
  for (const path of requiredFiles) {
    if (!seen.has(path)) {
      throw new Error(`CP1 fixture archive is missing ${path}`);
    }
  }
  for (const uid of expectedUids) {
    if (!seen.has(`spaces/${uid}`)) {
      throw new Error(`CP1 fixture archive is missing Space root ${uid}`);
    }
  }
  if (!seen.has("spaces") || !seen.has("spaces/.ugoite-space-slug-claims")) {
    throw new Error(
      "CP1 fixture archive is missing required storage directories",
    );
  }
}

function validateTarPath(path: string, kind: TarMember["kind"]): string {
  const normalized = kind === "directory" ? path.replace(/\/$/, "") : path;
  const hasControlCharacter = Array.from(normalized).some((character) => {
    const codePoint = character.codePointAt(0)!;
    return codePoint < 0x20 || codePoint === 0x7f;
  });
  if (
    !normalized || normalized.startsWith("/") || normalized.includes("\\") ||
    hasControlCharacter ||
    normalized.split("/").some((part) =>
      part === "" || part === "." || part === ".."
    )
  ) throw new Error(`unsafe CP1 fixture archive path: ${path}`);
  return normalized;
}

function validatePaxHeaderPath(path: string): void {
  // POSIX tar writers commonly store per-entry metadata under
  // ./PaxHeaders/<member>, <parent>/PaxHeader(s)/<member>, or <parent>/Pax.
  // GNU tar can truncate its synthetic PaxHeaders marker to "PaxHead" when
  // the parent prefix leaves only seven bytes in the USTAR name field. This
  // header is metadata, not an extracted member, but constrain its path too so
  // malformed archives fail closed.
  const normalized = path.startsWith("./") ? path.slice(2) : path;
  const segments = normalized.split("/");
  const hasPaxHeaderDirectory = segments.slice(0, -1).some((segment) =>
    /^PaxHeader(?:s)?(?:\.[A-Za-z0-9_-]+)?$/.test(segment)
  );
  const hasPaxMarkerName = segments.at(-1) === "Pax";
  const hasTruncatedGnuMarkerName = segments.at(-1) === "PaxHead";
  if (
    !normalized ||
    (
      !hasPaxHeaderDirectory && !hasPaxMarkerName &&
      !hasTruncatedGnuMarkerName
    )
  ) {
    throw new Error(`unsafe CP1 fixture PAX header path: ${path}`);
  }
  validateTarPath(normalized, "file");
}

export async function readTarMembers(
  archivePath: string,
): Promise<TarMember[]> {
  const file = await Deno.open(archivePath, { read: true });
  const stream = file.readable.pipeThrough(new DecompressionStream("gzip"));
  const reader = new TarStreamReader(stream.getReader());
  const members: TarMember[] = [];
  let archiveEnded = false;
  let totalSize = 0;
  let pendingPaxAttributes: Map<string, string> | null = null;
  try {
    while (true) {
      const header = await reader.readExact(512, true);
      if (header === null) {
        throw new Error("CP1 fixture archive has no tar terminator");
      }
      if (header.every((byte) => byte === 0)) {
        const secondTerminator = await reader.readExact(512);
        if (
          secondTerminator === null ||
          secondTerminator.some((byte) => byte !== 0)
        ) {
          throw new Error("CP1 fixture archive has a malformed tar terminator");
        }
        if (pendingPaxAttributes !== null) {
          throw new Error("CP1 fixture archive has an orphaned PAX header");
        }
        archiveEnded = true;
        await reader.requireZeroPadding();
        break;
      }
      validateTarChecksum(header);
      if (decodeTarString(header.subarray(257, 263)) !== "ustar") {
        throw new Error("CP1 fixture archive must use the USTAR format");
      }
      const rawName = decodeTarString(header.subarray(0, 100));
      const prefix = decodeTarString(header.subarray(345, 500));
      const name = prefix ? `${prefix}/${rawName}` : rawName;
      const typeFlag = String.fromCharCode(header[156] ?? 0);
      if (typeFlag === "x") {
        if (pendingPaxAttributes !== null) {
          throw new Error("CP1 fixture archive has nested PAX headers");
        }
        validatePaxHeaderPath(name);
        const size = parseTarOctal(
          header.subarray(124, 136),
          "PAX header size",
        );
        if (size > 64 * 1024) {
          throw new Error("CP1 fixture PAX header exceeds its size limit");
        }
        const payload = await reader.readExact(size);
        if (payload === null) {
          throw new Error("CP1 fixture PAX header payload is truncated");
        }
        pendingPaxAttributes = parsePaxAttributes(payload);
        await reader.skip(Math.ceil(size / 512) * 512 - size);
        continue;
      }
      if (typeFlag === "g") {
        throw new Error("CP1 fixture archive must not use global PAX headers");
      }
      const kind = typeFlag === "5"
        ? "directory"
        : typeFlag === "0" || typeFlag === "\0"
        ? "file"
        : null;
      if (kind === null) {
        throw new Error(
          `CP1 fixture archive contains a link or special file: ${name}`,
        );
      }
      const mode = parseTarOctal(header.subarray(100, 108), "mode");
      const size = parseTarOctal(header.subarray(124, 136), "size");
      if (kind === "directory" && size !== 0) {
        throw new Error(`CP1 fixture directory has data: ${name}`);
      }
      totalSize += size;
      if (totalSize > 20 * 1024 * 1024 * 1024) {
        throw new Error(
          "CP1 fixture archive exceeds the uncompressed size limit",
        );
      }
      const path = validateTarPath(
        pendingPaxAttributes?.get("path") ?? name,
        kind,
      );
      pendingPaxAttributes = null;
      members.push({ path, kind, mode, size });
      if (members.length > 1_000_000) {
        throw new Error("CP1 fixture archive has too many members");
      }
      await reader.skip(Math.ceil(size / 512) * 512);
    }
  } finally {
    await reader.cancel();
    reader.release();
  }
  if (!archiveEnded) throw new Error("CP1 fixture archive is truncated");
  return members;
}

function parsePaxAttributes(payload: Uint8Array): Map<string, string> {
  const attributes = new Map<string, string>();
  let offset = 0;
  const decoder = new TextDecoder("utf-8", { fatal: true });
  while (offset < payload.length) {
    let space = offset;
    while (space < payload.length && payload[space] !== 0x20) space++;
    if (space === payload.length) {
      throw new Error("CP1 fixture PAX record has no length");
    }
    const lengthText = decoder.decode(payload.subarray(offset, space));
    if (!/^[1-9][0-9]*$/.test(lengthText)) {
      throw new Error("CP1 fixture PAX record length is invalid");
    }
    const length = Number(lengthText);
    if (!Number.isSafeInteger(length) || length <= space - offset + 1) {
      throw new Error("CP1 fixture PAX record length is out of range");
    }
    const end = offset + length;
    if (end > payload.length || payload[end - 1] !== 0x0a) {
      throw new Error("CP1 fixture PAX record is truncated");
    }
    const record = decoder.decode(payload.subarray(space + 1, end - 1));
    const equals = record.indexOf("=");
    if (equals <= 0) throw new Error("CP1 fixture PAX record is malformed");
    const key = record.slice(0, equals);
    const value = record.slice(equals + 1);
    if (!/^[A-Za-z0-9_.-]+$/.test(key) || attributes.has(key)) {
      throw new Error("CP1 fixture PAX record key is invalid or duplicated");
    }
    if (key === "path") {
      if (!value) throw new Error("CP1 fixture PAX path is empty");
    } else if (
      key === "mtime" || key === "atime" || key === "ctime" ||
      key === "LIBARCHIVE.creationtime"
    ) {
      if (
        !/^-?[0-9]+(?:\.[0-9]+)?$/.test(value) ||
        !Number.isFinite(Number(value))
      ) {
        throw new Error(`CP1 fixture PAX timestamp is invalid: ${key}`);
      }
    } else {
      throw new Error(`CP1 fixture PAX record is unsupported: ${key}`);
    }
    attributes.set(key, value);
    offset = end;
  }
  return attributes;
}

class TarStreamReader {
  #reader: ReadableStreamDefaultReader<Uint8Array>;
  #pending: Uint8Array<ArrayBufferLike> = new Uint8Array(0);
  #done = false;

  constructor(reader: ReadableStreamDefaultReader<Uint8Array>) {
    this.#reader = reader;
  }

  async readExact(size: number, allowEof = false): Promise<Uint8Array | null> {
    const chunks: Uint8Array[] = [];
    let total = 0;
    while (total < size) {
      if (this.#pending.length === 0) {
        if (this.#done) {
          if (total === 0 && allowEof) return null;
          if (total === 0) return null;
          throw new Error("CP1 fixture archive is truncated");
        }
        const next = await this.#reader.read();
        if (next.done) {
          this.#done = true;
          continue;
        }
        this.#pending = next.value;
      }
      const take = Math.min(size - total, this.#pending.length);
      chunks.push(this.#pending.subarray(0, take));
      total += take;
      this.#pending = this.#pending.subarray(take);
    }
    const result = new Uint8Array(size);
    let offset = 0;
    for (const chunk of chunks) {
      result.set(chunk, offset);
      offset += chunk.length;
    }
    return result;
  }

  async skip(size: number): Promise<void> {
    let remaining = size;
    while (remaining > 0) {
      const chunk = await this.readExact(Math.min(remaining, 64 * 1024));
      if (chunk === null) {
        throw new Error("CP1 fixture archive file payload is truncated");
      }
      remaining -= chunk.length;
    }
  }

  async requireZeroPadding(): Promise<void> {
    if (this.#pending.some((byte) => byte !== 0)) {
      throw new Error("CP1 fixture archive has data after its tar terminator");
    }
    this.#pending = new Uint8Array(0);
    while (!this.#done) {
      const next = await this.#reader.read();
      if (next.done) {
        this.#done = true;
      } else if (next.value.some((byte) => byte !== 0)) {
        throw new Error(
          "CP1 fixture archive has data after its tar terminator",
        );
      }
    }
  }

  release(): void {
    this.#reader.releaseLock();
  }

  async cancel(): Promise<void> {
    try {
      await this.#reader.cancel();
    } catch {
      // The stream closes its backing file automatically after full reads.
    }
  }
}

function validateTarChecksum(header: Uint8Array): void {
  const expected = parseTarOctal(header.subarray(148, 156), "checksum");
  let actual = 0;
  for (let index = 0; index < header.length; index++) {
    actual += index >= 148 && index < 156 ? 32 : header[index]!;
  }
  if (actual !== expected) {
    throw new Error("CP1 fixture tar header checksum mismatch");
  }
}

function parseTarOctal(bytes: Uint8Array, name: string): number {
  if (bytes[0]! & 0x80) {
    throw new Error(
      `CP1 fixture tar ${name} uses unsupported base-256 encoding`,
    );
  }
  const value = decodeTarString(bytes).trim();
  if (!/^[0-7]+$/.test(value)) {
    throw new Error(`CP1 fixture tar has an invalid ${name}`);
  }
  const number = Number.parseInt(value, 8);
  if (!Number.isSafeInteger(number)) {
    throw new Error(`CP1 fixture tar ${name} is too large`);
  }
  return number;
}

function decodeTarString(bytes: Uint8Array): string {
  const end = bytes.indexOf(0);
  const data = end === -1 ? bytes : bytes.subarray(0, end);
  return new TextDecoder("utf-8", { fatal: true }).decode(data);
}

async function walkRegularTree(root: string): Promise<
  Array<{
    path: string;
    relativePath: string;
    isDirectory: boolean;
    mode: number;
  }>
> {
  const output: Array<
    { path: string; relativePath: string; isDirectory: boolean; mode: number }
  > = [];
  const pending = [root];
  while (pending.length > 0) {
    const current = pending.pop()!;
    for await (const entry of Deno.readDir(current)) {
      const path = `${current}/${entry.name}`;
      const info = await Deno.lstat(path);
      if (info.isSymlink || (!info.isDirectory && !info.isFile)) {
        throw new Error(
          `CP1 fixture contains a symlink or special file: ${path}`,
        );
      }
      const relativePath = path.slice(root.length + 1).replaceAll("\\", "/");
      output.push({
        path,
        relativePath,
        isDirectory: info.isDirectory,
        mode: modeBits(info.mode),
      });
      if (info.isDirectory) pending.push(path);
    }
  }
  return output;
}

async function prepareOwnedOutput(outputInput: string): Promise<string> {
  const absolute = resolvePath(outputInput);
  let exists = false;
  try {
    const info = await Deno.lstat(absolute);
    if (info.isSymlink || !info.isDirectory) {
      throw new Error("CP1 fixture output is not a regular directory");
    }
    exists = true;
  } catch (error) {
    if (!(error instanceof Deno.errors.NotFound)) throw error;
  }
  if (exists) {
    try {
      const markerInfo = await Deno.lstat(
        `${absolute}/.ugoite-cp1-fixture-bundle`,
      );
      if (
        markerInfo.isSymlink || !markerInfo.isFile ||
        await Deno.readTextFile(`${absolute}/.ugoite-cp1-fixture-bundle`) !==
          OWNER_MARKER
      ) {
        throw new Error(
          "refusing to replace an unowned CP1 fixture output directory",
        );
      }
    } catch (error) {
      if (error instanceof Deno.errors.NotFound) {
        throw new Error(
          "refusing to replace an unowned CP1 fixture output directory",
          { cause: error },
        );
      }
      throw error;
    }
    await Deno.remove(absolute, { recursive: true });
  }
  await Deno.mkdir(resolvePath(absolute.slice(0, absolute.lastIndexOf("/"))), {
    recursive: true,
    mode: 0o700,
  });
  await Deno.mkdir(absolute, { mode: 0o700 });
  await Deno.writeTextFile(
    `${absolute}/.ugoite-cp1-fixture-bundle`,
    OWNER_MARKER,
    { createNew: true },
  );
  return Deno.realPath(absolute);
}

async function prepareEmptyDestination(
  destinationInput: string,
): Promise<string> {
  const absolute = resolvePath(destinationInput);
  try {
    const info = await Deno.lstat(absolute);
    if (info.isSymlink || !info.isDirectory) {
      throw new Error("CP1 fixture destination must be a regular directory");
    }
    if (modeBits(info.mode) & 0o077) {
      throw new Error("CP1 fixture destination permissions must be private");
    }
    for await (const _entry of Deno.readDir(absolute)) {
      throw new Error("CP1 fixture destination must be empty and unique");
    }
  } catch (error) {
    if (!(error instanceof Deno.errors.NotFound)) throw error;
    await Deno.mkdir(absolute, { recursive: true, mode: 0o700 });
  }
  return Deno.realPath(absolute);
}

function resolvePath(path: string): string {
  return path.startsWith("/") ? path : `${Deno.cwd()}/${path}`;
}

function modeBits(mode: number | null): number {
  if (mode === null) {
    throw new Error(
      "CP1 fixture permissions are unavailable on this filesystem",
    );
  }
  return mode & 0o7777;
}

async function verifiedXtask(
  input: string | undefined,
  sourceSha: string,
): Promise<string> {
  const configured = input?.trim() ||
    Deno.env.get("UGOITE_SEED_XTASK_BINARY")?.trim();
  if (!configured) {
    throw new Error(
      "CP1 fixture verification requires UGOITE_SEED_XTASK_BINARY",
    );
  }
  const binaryPath = configured.startsWith("/")
    ? configured
    : `${Deno.cwd()}/${configured}`;
  const info = await Deno.lstat(binaryPath);
  if (!info.isFile || info.isSymlink || !(modeBits(info.mode) & 0o111)) {
    throw new Error("CP1 fixture xtask must be an executable regular file");
  }
  const sidecar = `${binaryPath}.source-sha`;
  const sidecarInfo = await Deno.lstat(sidecar);
  if (!sidecarInfo.isFile || sidecarInfo.isSymlink) {
    throw new Error("CP1 fixture xtask is missing its source SHA sidecar");
  }
  if ((await Deno.readTextFile(sidecar)).trim() !== sourceSha) {
    throw new Error("CP1 fixture xtask source SHA does not match the checkout");
  }
  return Deno.realPath(binaryPath);
}

async function generatorFingerprint(): Promise<string> {
  const hash = createHash("sha256");
  for (
    const path of [
      "Cargo.lock",
      "crates/xtask/src/main.rs",
      "crates/ugoite-iceberg/src/sample_data.rs",
      "tools/cp1_fixture_spec.ts",
    ]
  ) {
    const identity = await sha256File(`${Deno.cwd()}/${path}`);
    hash.update(`${path}:${identity.sha256}\n`);
  }
  return hash.digest("hex");
}

function validateRunId(value: string): void {
  if (!/^(?:[0-9]+|local-[A-Za-z0-9-]+)$/.test(value)) {
    throw new Error("CP1 fixture run ID has an unsupported format");
  }
}

function asRecord(
  value: unknown,
  description: string,
): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new Error(`${description} must be an object`);
  }
  return value as Record<string, unknown>;
}

function requireKeys(
  record: Record<string, unknown>,
  keys: string[],
  description: string,
): void {
  const actual = Object.keys(record).sort();
  const expected = [...keys].sort();
  if (canonicalJson(actual) !== canonicalJson(expected)) {
    throw new Error(`${description} has missing or unknown fields`);
  }
}

function canonicalJson(value: unknown): string {
  if (Array.isArray(value)) return `[${value.map(canonicalJson).join(",")}]`;
  if (typeof value === "object" && value !== null) {
    const object = value as Record<string, unknown>;
    return `{${
      Object.keys(object).sort().map((key) =>
        `${JSON.stringify(key)}:${canonicalJson(object[key])}`
      ).join(",")
    }}`;
  }
  return JSON.stringify(value);
}

async function gitHead(): Promise<string> {
  return (await run("git", ["rev-parse", "HEAD"])).trim();
}

async function run(command: string, args: string[]): Promise<string> {
  const result = await new Deno.Command(command, {
    args,
    stdout: "piped",
    stderr: "piped",
  }).output();
  const stdout = new TextDecoder().decode(result.stdout).trim();
  const stderr = new TextDecoder().decode(result.stderr).trim();
  if (!result.success) {
    throw new Error(
      `${command} ${args.join(" ")} failed${stderr ? `: ${stderr}` : ""}`,
    );
  }
  return stdout;
}

if (import.meta.main) await main();
