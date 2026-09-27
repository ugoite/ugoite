#!/bin/bash
# Create a CLI-owned Space on a separate root, verify it, and copy only its
# operator-owned prefix into the empty root that the fresh Node will open.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
DENO_BIN="$(mise which deno)"
CHECKOUT_SOURCE_SHA="$(git -C "$ROOT_DIR" rev-parse HEAD)"
if [ -n "$(git -C "$ROOT_DIR" status --porcelain)" ]; then
  WORKING_TREE_DIRTY=true
else
  WORKING_TREE_DIRTY=false
fi
if [ -n "${UGOITE_SOURCE_SHA:-}" ] && [ "$UGOITE_SOURCE_SHA" != "$CHECKOUT_SOURCE_SHA" ]; then
  echo "UGOITE_SOURCE_SHA does not match the checkout under test" >&2
  exit 1
fi
DEST_ROOT="${1:?usage: seed-portable-space.sh DESTINATION_ROOT}"
PORTABLE_CLI_CONFIG="${DEST_ROOT}.cli-config.toml"
SOURCE_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/ugoite-portable-source.XXXXXX")"
SOURCE_ROOT_REAL="$(cd "$SOURCE_ROOT" && pwd -P)"
SOURCE_CLI_CONFIG="${SOURCE_ROOT}.cli-config.toml"
PARTIAL_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/ugoite-portable-partial.XXXXXX")"
PARTIAL_CLI_CONFIG="${PARTIAL_ROOT}.cli-config.toml"
AUTH_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/ugoite-portable-auth.XXXXXX")"
PORTABLE_SPACE_SLUG="portable-e2e-$(date +%s)-$$"
PORTABLE_FORM_NAME="PortableAudit"
ASSET_FORM_NAME="PortableAsset"

cleanup() {
  rm -rf "$SOURCE_ROOT" "$PARTIAL_ROOT" "$AUTH_ROOT"
  rm -f "$SOURCE_CLI_CONFIG" "$PARTIAL_CLI_CONFIG" "${AUTH_ROOT}.cli-config.toml"
}
trap cleanup EXIT

run_source_cli() {
  cargo run -q --manifest-path "$ROOT_DIR/Cargo.toml" -p ugoite-cli --locked \
    -- --config "$SOURCE_CLI_CONFIG" "$@"
}

write_fixture_config() {
  local root="$1"
  local output="$2"
  SOURCE_ROOT="$SOURCE_ROOT" SOURCE_ROOT_REAL="$SOURCE_ROOT_REAL" FIXTURE_ROOT="$root" SOURCE_CONFIG="$SOURCE_CLI_CONFIG" OUTPUT_CONFIG="$output" "$DENO_BIN" eval '
    const template = await Deno.readTextFile(Deno.env.get("SOURCE_CONFIG")!);
    const sourceRoot = Deno.env.get("SOURCE_ROOT")!;
    const sourceRootReal = Deno.env.get("SOURCE_ROOT_REAL")!;
    const destinationRoot = Deno.env.get("FIXTURE_ROOT")!;
    const rootInConfig = template.includes(sourceRoot) ? sourceRoot : sourceRootReal;
    if (!template.includes(rootInConfig)) throw new Error("CLI source config does not contain its storage root");
    await Deno.writeTextFile(Deno.env.get("OUTPUT_CONFIG")!, template.replaceAll(rootInConfig, destinationRoot));
  '
}

verify_unclaimed_fixture() {
  local root="$1"
  local config="$2"
  local output="$3"
  set +e
  (cd "$root" && cargo run -q --manifest-path "$ROOT_DIR/Cargo.toml" -p ugoite-cli --locked \
    -- --config "$config" space verify --deep --format json) >"$output" 2>"${output}.stderr"
  local status=$?
  set -e
  # A CLI-owned portable Space is intentionally not yet claimed by a Node.
  # Its Knowledge sections must verify while authorization is reported as
  # incomplete; the command therefore exits nonzero until setup claims it.
  PORTABLE_VERIFY_EXIT="$status" PORTABLE_VERIFY_REPORT="$output" "$DENO_BIN" eval '
    const report = JSON.parse(await Deno.readTextFile(Deno.env.get("PORTABLE_VERIFY_REPORT")!));
    const expected = ["metadata", "catalog", "forms", "entries", "changes_and_audit", "assets", "derived"];
    for (const name of expected) {
      const section = report.sections[name];
      if (section.status !== "valid" && section.status !== "valid_with_rebuildable_derived_state") {
        throw new Error(`${name} verification was ${section.status}: ${section.detail ?? "no detail"}`);
      }
    }
    if (report.sections.authorization.status !== "incomplete") {
      throw new Error(`unclaimed authorization status was ${report.sections.authorization.status}`);
    }
    if (Deno.env.get("PORTABLE_VERIFY_EXIT") === "0") throw new Error("unclaimed verify unexpectedly exited successfully");
  '
}

mkdir -p "$DEST_ROOT"
(
  cd "$SOURCE_ROOT"
  run_source_cli config init >/dev/null
  PORTABLE_SPACE_JSON="$(run_source_cli space -o json create "$PORTABLE_SPACE_SLUG")"

  cat > "$SOURCE_ROOT/portable-form.json" <<EOF
{"name":"$PORTABLE_FORM_NAME","version":1,"template":"# PortableAudit\\n\\n## Subject\\n\\n## Body\\n","fields":{"Subject":{"type":"string","required":true},"Body":{"type":"markdown","required":false}}}
EOF
  run_source_cli form -o json save "$SOURCE_ROOT/portable-form.json" >/dev/null
  PORTABLE_ENTRY_JSON="$(run_source_cli entry -o json create --form "$PORTABLE_FORM_NAME" --field 'Subject=Seeded before Node startup' --field 'Body=portable history survives setup')"

  cat > "$SOURCE_ROOT/portable-asset-form.json" <<EOF
{"name":"$ASSET_FORM_NAME","version":1,"template":"# PortableAsset\\n\\n## Attachment\\n","fields":{"Attachment":{"type":"asset_reference","required":true}}}
EOF
  run_source_cli form -o json save "$SOURCE_ROOT/portable-asset-form.json" >/dev/null
  printf 'portable asset survives prefix copy\n' > "$SOURCE_ROOT/portable-asset.txt"
  PORTABLE_ASSET_JSON="$(run_source_cli asset -o json upload "$SOURCE_ROOT/portable-asset.txt")"
  PORTABLE_ASSET_ENTRY_JSON="$(PORTABLE_ASSET_JSON="$PORTABLE_ASSET_JSON" "$DENO_BIN" eval 'console.log(JSON.stringify({Attachment: JSON.parse(Deno.env.get("PORTABLE_ASSET_JSON")!).asset_reference}));')"
  printf '%s\n' "$PORTABLE_ASSET_ENTRY_JSON" > "$SOURCE_ROOT/portable-asset-fields.json"
  run_source_cli entry -o json create --form "$ASSET_FORM_NAME" --fields-file "$SOURCE_ROOT/portable-asset-fields.json" > "$SOURCE_ROOT/asset-entry-receipt.json"

  PORTABLE_SQL_JSON="$(run_source_cli sql saved create --name portable-recovery --sql 'SELECT 1 AS recovery_check')"
  PORTABLE_CHANGES_JSON="$(run_source_cli change -o json list)"
  printf '%s' "$PORTABLE_SPACE_JSON" > "$SOURCE_ROOT/space.json"
  printf '%s' "$PORTABLE_ENTRY_JSON" > "$SOURCE_ROOT/entry.json"
  printf '%s' "$PORTABLE_SQL_JSON" > "$SOURCE_ROOT/sql.json"
  printf '%s' "$PORTABLE_ASSET_JSON" > "$SOURCE_ROOT/asset.json"
  printf '%s' "$PORTABLE_CHANGES_JSON" > "$SOURCE_ROOT/changes.json"
)

verify_unclaimed_fixture "$SOURCE_ROOT" "$SOURCE_CLI_CONFIG" "$SOURCE_ROOT/verify-source.json"

SPACE_UID="$("$DENO_BIN" eval 'const s=JSON.parse(await Deno.readTextFile(Deno.args[0])); console.log(s.space.space_uid)' "$SOURCE_ROOT/space.json")"
mkdir -p "$DEST_ROOT/spaces"
cp -a "$SOURCE_ROOT/spaces/$SPACE_UID" "$DEST_ROOT/spaces/"

write_fixture_config "$DEST_ROOT" "$PORTABLE_CLI_CONFIG"
verify_unclaimed_fixture "$DEST_ROOT" "$PORTABLE_CLI_CONFIG" "$DEST_ROOT/verify-copy.json"

# A deliberately partial prefix is diagnostic evidence only. Verify must
# reject it without creating or rewriting any objects.
mkdir -p "$PARTIAL_ROOT/spaces/$SPACE_UID"
cp "$SOURCE_ROOT/spaces/$SPACE_UID/meta.json" "$PARTIAL_ROOT/spaces/$SPACE_UID/meta.json"
cp "$PARTIAL_ROOT/spaces/$SPACE_UID/meta.json" "$PARTIAL_ROOT/before-meta.json"
write_fixture_config "$PARTIAL_ROOT" "$PARTIAL_CLI_CONFIG"
set +e
(cd "$PARTIAL_ROOT" && cargo run -q --manifest-path "$ROOT_DIR/Cargo.toml" -p ugoite-cli --locked \
  -- --config "$PARTIAL_CLI_CONFIG" space verify --deep --format json) >"$PARTIAL_ROOT/verify.json" 2>"$PARTIAL_ROOT/verify.stderr"
PARTIAL_VERIFY_EXIT=$?
set -e
if [ "$PARTIAL_VERIFY_EXIT" -eq 0 ]; then
  echo "partial Space prefix unexpectedly verified successfully" >&2
  exit 1
fi
"$DENO_BIN" eval '
  const report = JSON.parse(await Deno.readTextFile(Deno.args[0]));
  if (!report.status || !["invalid", "incomplete"].includes(report.status)) throw new Error(`partial copy status was ${report.status}`);
' "$PARTIAL_ROOT/verify.json"
if [ "$(find "$PARTIAL_ROOT/spaces/$SPACE_UID" -type f | wc -l | tr -d ' ')" -ne 1 ]; then
  echo "space verify wrote into the partial prefix" >&2
  exit 1
fi
cmp -s "$PARTIAL_ROOT/before-meta.json" "$PARTIAL_ROOT/spaces/$SPACE_UID/meta.json"

# Exercise malformed and foreign Space authorization as separate fail-closed
# cases. Neither verification may rewrite the offending state.
for AUTH_CASE in corrupt foreign; do
  CASE_ROOT="$AUTH_ROOT/$AUTH_CASE"
  CASE_CONFIG="${CASE_ROOT}.cli-config.toml"
  mkdir -p "$CASE_ROOT/spaces"
  cp -a "$SOURCE_ROOT/spaces/$SPACE_UID" "$CASE_ROOT/spaces/"
  mkdir -p "$CASE_ROOT/spaces/$SPACE_UID/security"
  if [ "$AUTH_CASE" = "corrupt" ]; then
    printf '{' > "$CASE_ROOT/spaces/$SPACE_UID/security/principals.json"
    EXPECTED_DETAIL="decode Space authorization state"
  else
    cat > "$CASE_ROOT/spaces/$SPACE_UID/security/principals.json" <<EOF
{"schema_version":1,"space_uid":"00000000-0000-0000-0000-000000000001","principals":{"00000000-0000-0000-0000-000000000002":{"principal_id":"00000000-0000-0000-0000-000000000002","kind":"human","display_name":"Foreign owner","state":"active","created_at":"2026-01-01T00:00:00Z"}},"memberships":{"00000000-0000-0000-0000-000000000002":{"principal_id":"00000000-0000-0000-0000-000000000002","role":"owner","created_at":"2026-01-01T00:00:00Z"}},"principal_lifecycle_epochs":{"00000000-0000-0000-0000-000000000002":1},"revision":1}
EOF
    EXPECTED_DETAIL="different space_uid"
  fi
  cp "$CASE_ROOT/spaces/$SPACE_UID/security/principals.json" "$CASE_ROOT/before.json"
  write_fixture_config "$CASE_ROOT" "$CASE_CONFIG"
  set +e
  (cd "$CASE_ROOT" && cargo run -q --manifest-path "$ROOT_DIR/Cargo.toml" -p ugoite-cli --locked \
    -- --config "$CASE_CONFIG" space verify --deep --format json) >"$CASE_ROOT/verify.json" 2>"$CASE_ROOT/verify.stderr"
  AUTH_VERIFY_EXIT=$?
  set -e
  if [ "$AUTH_VERIFY_EXIT" -eq 0 ]; then
    echo "$AUTH_CASE Space authorization unexpectedly verified successfully" >&2
    exit 1
  fi
  AUTH_CASE="$AUTH_CASE" EXPECTED_DETAIL="$EXPECTED_DETAIL" "$DENO_BIN" eval '
    const report = JSON.parse(await Deno.readTextFile(Deno.args[0]));
    const section = report.sections.authorization;
    if (section.status !== "invalid") throw new Error(`${Deno.env.get("AUTH_CASE")} auth status was ${section.status}`);
    if (!section.detail?.includes(Deno.env.get("EXPECTED_DETAIL")!)) throw new Error(`${Deno.env.get("AUTH_CASE")} auth detail did not distinguish the failure: ${section.detail}`);
  ' "$CASE_ROOT/verify.json"
  cmp -s "$CASE_ROOT/before.json" "$CASE_ROOT/spaces/$SPACE_UID/security/principals.json"
done

SOURCE_FILE_PROOF="$(SOURCE_ROOT="$SOURCE_ROOT" SPACE_UID="$SPACE_UID" "$DENO_BIN" eval '
  const root = `${Deno.env.get("SOURCE_ROOT")}/spaces/${Deno.env.get("SPACE_UID")}`;
  const files: Record<string, string> = {};
  const appendOnlyPrefixes: Record<string, number[]> = {};
  for await (const entry of Deno.readDir(root)) {
    const pending = [entry.name];
    while (pending.length) {
      const relative = pending.pop()!;
      const path = `${root}/${relative}`;
      const info = await Deno.stat(path);
      if (info.isDirectory) {
        for await (const child of Deno.readDir(path)) pending.push(`${relative}/${child.name}`);
      } else if (info.isFile) {
        const bytes = await Deno.readFile(path);
        const digest = new Uint8Array(await crypto.subtle.digest("SHA-256", bytes));
        files[relative] = [...digest].map((byte) => byte.toString(16).padStart(2, "0")).join("");
        if (relative === "audit/events.jsonl") {
          appendOnlyPrefixes[relative] = Array.from(bytes);
        }
      }
    }
  }
  console.log(JSON.stringify({ files, append_only_prefixes: appendOnlyPrefixes }));
')"

PORTABLE_SPACE_JSON="$(cat "$SOURCE_ROOT/space.json")" \
PORTABLE_ENTRY_JSON="$(cat "$SOURCE_ROOT/entry.json")" \
PORTABLE_SQL_JSON="$(cat "$SOURCE_ROOT/sql.json")" \
PORTABLE_ASSET_JSON="$(cat "$SOURCE_ROOT/asset.json")" \
PORTABLE_ASSET_ENTRY_JSON="$(cat "$SOURCE_ROOT/asset-entry-receipt.json")" \
PORTABLE_CHANGES_JSON="$(cat "$SOURCE_ROOT/changes.json")" \
PORTABLE_SOURCE_FILES="$SOURCE_FILE_PROOF" \
PORTABLE_SOURCE_SHA="$CHECKOUT_SOURCE_SHA" \
PORTABLE_WORKING_TREE_DIRTY="$WORKING_TREE_DIRTY" \
PORTABLE_SOURCE_REPORT="$SOURCE_ROOT/verify-source.json" \
PORTABLE_COPY_REPORT="$DEST_ROOT/verify-copy.json" \
PORTABLE_PARTIAL_REPORT="$PARTIAL_ROOT/verify.json" \
PORTABLE_CORRUPT_REPORT="$AUTH_ROOT/corrupt/verify.json" \
PORTABLE_FOREIGN_REPORT="$AUTH_ROOT/foreign/verify.json" \
PORTABLE_SPACE_SLUG="$PORTABLE_SPACE_SLUG" \
PORTABLE_FORM_NAME="$PORTABLE_FORM_NAME" \
PORTABLE_ASSET_FORM_NAME="$ASSET_FORM_NAME" \
DEST_ROOT="$DEST_ROOT" \
"$DENO_BIN" eval '
  const space = JSON.parse(Deno.env.get("PORTABLE_SPACE_JSON")!);
  const entry = JSON.parse(Deno.env.get("PORTABLE_ENTRY_JSON")!);
  const sql = JSON.parse(Deno.env.get("PORTABLE_SQL_JSON")!);
  const asset = JSON.parse(Deno.env.get("PORTABLE_ASSET_JSON")!);
  const assetEntry = JSON.parse(Deno.env.get("PORTABLE_ASSET_ENTRY_JSON")!);
  const changes = JSON.parse(Deno.env.get("PORTABLE_CHANGES_JSON")!);
  const sourceFileProof = JSON.parse(Deno.env.get("PORTABLE_SOURCE_FILES")!);
  const readReport = async (name: string) => JSON.parse(await Deno.readTextFile(Deno.env.get(name)!));
  const sourceReport = await readReport("PORTABLE_SOURCE_REPORT");
  const copyReport = await readReport("PORTABLE_COPY_REPORT");
  const partialReport = await readReport("PORTABLE_PARTIAL_REPORT");
  const corruptReport = await readReport("PORTABLE_CORRUPT_REPORT");
  const foreignReport = await readReport("PORTABLE_FOREIGN_REPORT");
  await Deno.writeTextFile(`${Deno.env.get("DEST_ROOT")}/portable-space-proof.json`, JSON.stringify({
    schema_version: 1,
    source_sha: Deno.env.get("PORTABLE_SOURCE_SHA"),
    working_tree_dirty: Deno.env.get("PORTABLE_WORKING_TREE_DIRTY") === "true",
    fixture_seed: Deno.env.get("PORTABLE_SPACE_SLUG"),
    runner_command: Deno.env.get("E2E_PORTABLE_RUNNER_COMMAND") ?? "bash e2e/scripts/run-e2e.sh portable-space",
    environment: { os: Deno.build.os, arch: Deno.build.arch, storage_backend: "local-filesystem" },
    verification: {
      source: { status: sourceReport.status, authorization: sourceReport.sections.authorization.status },
      copied: { status: copyReport.status, authorization: copyReport.sections.authorization.status },
      partial: { status: partialReport.status },
      corrupt_authorization: { status: corruptReport.sections.authorization.status },
      foreign_authorization: { status: foreignReport.sections.authorization.status },
    },
    space_uid: space.space.space_uid,
    slug: Deno.env.get("PORTABLE_SPACE_SLUG"),
    form_name: Deno.env.get("PORTABLE_FORM_NAME"),
    entry_id: entry.id,
    revision_id: entry.revision_id,
    asset_form_name: Deno.env.get("PORTABLE_ASSET_FORM_NAME"),
    asset_entry_id: assetEntry.id,
    asset_id: asset.asset_reference.asset_id,
    asset_sha256: asset.asset_reference.sha256,
    saved_sql_id: sql.id,
    saved_sql_revision_id: sql.revision_id,
    changes,
    source_files: sourceFileProof.files,
    append_only_prefixes: sourceFileProof.append_only_prefixes,
  }, null, 2));
'

printf '%s\n' "$PORTABLE_CLI_CONFIG"
