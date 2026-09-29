#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUTPUT_DIR="${UGOITE_SQL_EXPORT_MEASURE_OUTPUT:-$ROOT_DIR/target/sql-export-measurement}"
PROFILE_DIR="${UGOITE_CP1_PROFILE_DIR:-$ROOT_DIR/target/cp1-profiling}"
FIXTURE_BUNDLE_DIR="${UGOITE_CP1_FIXTURE_BUNDLE_DIR:-}"
if [[ -n "$FIXTURE_BUNDLE_DIR" && "$FIXTURE_BUNDLE_DIR" != /* ]]; then
  FIXTURE_BUNDLE_DIR="$ROOT_DIR/$FIXTURE_BUNDLE_DIR"
fi
EXPORT_FIXTURE_ROW="$(deno run --quiet "$ROOT_DIR/tools/cp1_fixture_spec.ts" export)"
if [[ "$EXPORT_FIXTURE_ROW" == *$'\n'* ]]; then
  echo "CP1 export fixture specification must contain exactly one fixture" >&2
  exit 1
fi
IFS=$'\t' read -r FIXTURE_SLUG FIXTURE_SCENARIO FIXTURE_SEED FIXTURE_ENTRY_COUNT FIXTURE_OWNER_DISPLAY_NAME <<<"$EXPORT_FIXTURE_ROW"
if [[ -z "$FIXTURE_SLUG" || -z "$FIXTURE_SCENARIO" ]] \
  || ! [[ "$FIXTURE_SEED" =~ ^[0-9]+$ && "$FIXTURE_ENTRY_COUNT" =~ ^[0-9]+$ ]]; then
  echo "Invalid CP1 export fixture specification" >&2
  exit 1
fi
PROFILE_RUN_ID="$(date -u +%Y%m%dT%H%M%SZ)-$$"
PROFILE_STARTED_MS="$(deno eval --quiet 'console.log(Date.now())')"
mkdir -p "$PROFILE_DIR"
PROFILE_REPORT="$PROFILE_DIR/export-${PROFILE_RUN_ID}.json"
CLI_BUILD_RESOURCE="$PROFILE_DIR/export-cli-build-${PROFILE_RUN_ID}.time.txt"
CLI_PROFILE_TIMED_STEP_ARGS=()
mkdir -p "$OUTPUT_DIR"
if [[ -n "$(find "$OUTPUT_DIR" -mindepth 1 -maxdepth 1 -print -quit)" ]]; then
  echo "Refusing to overwrite a non-empty export measurement output directory: $OUTPUT_DIR" >&2
  echo "Choose another path with UGOITE_SQL_EXPORT_MEASURE_OUTPUT." >&2
  exit 1
fi

KEEP_ROOT=false
if [[ -n "${UGOITE_SQL_EXPORT_MEASURE_ROOT:-}" ]]; then
  MEASURE_ROOT="$UGOITE_SQL_EXPORT_MEASURE_ROOT"
  KEEP_ROOT=true
  mkdir -p "$MEASURE_ROOT"
  if [[ -n "$(find "$MEASURE_ROOT" -mindepth 1 -maxdepth 1 -print -quit)" ]]; then
    echo "Refusing to seed a non-empty export measurement root: $MEASURE_ROOT" >&2
    exit 1
  fi
else
  MEASURE_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/ugoite-sql-export.XXXXXX")"
fi

CONFIG="$MEASURE_ROOT/ugoite.toml"
DATA_ROOT="$MEASURE_ROOT/data"
SQL_FILE="$OUTPUT_DIR/query.sql"
SPACE_PATH="$DATA_ROOT/spaces"
SOURCE_SHA="$(git -C "$ROOT_DIR" rev-parse HEAD)"
if [[ -n "$FIXTURE_BUNDLE_DIR" ]]; then
  SEED_PROFILE="$DATA_ROOT/.cp1-profiles/$FIXTURE_SLUG.json"
  mkdir -m 700 -p "$DATA_ROOT"
else
  SEED_PROFILE="$PROFILE_DIR/${FIXTURE_SLUG}-${PROFILE_RUN_ID}.json"
fi

cleanup() {
  local exit_code=$?
  trap - EXIT INT TERM
  set +e
  deno run -A "$ROOT_DIR/tools/cp1_profile.ts" export \
    --output "$PROFILE_REPORT" \
    --root "$DATA_ROOT" \
    --started-ms "$PROFILE_STARTED_MS" \
    --exit-code "$exit_code" \
    --evidence-report "$OUTPUT_DIR/measurement.json" \
    --seed "$FIXTURE_SLUG" "$SEED_PROFILE" "${SEED_PROFILE%.json}.time.txt" \
    --export-run 100 "$OUTPUT_DIR/page-100.ndjson" \
      "$OUTPUT_DIR/page-100.summary.json" "$OUTPUT_DIR/page-100.time.txt" \
    --export-run 1000 "$OUTPUT_DIR/page-1000.ndjson" \
      "$OUTPUT_DIR/page-1000.summary.json" "$OUTPUT_DIR/page-1000.time.txt" \
    "${CLI_PROFILE_TIMED_STEP_ARGS[@]}"
  local profile_exit_code=$?
  if [[ "$KEEP_ROOT" == false ]]; then
    rm -rf "$MEASURE_ROOT"
    local cleanup_exit_code=$?
    if [[ "$exit_code" -eq 0 && "$cleanup_exit_code" -ne 0 ]]; then
      exit_code=$cleanup_exit_code
    fi
  fi
  if [[ "$exit_code" -eq 0 && "$profile_exit_code" -ne 0 ]]; then exit_code=1; fi
  exit "$exit_code"
}
trap 'exit 130' INT
trap 'exit 143' TERM
trap cleanup EXIT

CLI_MODE="local-build"
if [[ -n "${UGOITE_SQL_EXPORT_CLI_BINARY:-}" ]]; then
  CLI_MODE="verified-artifact"
  BIN="$UGOITE_SQL_EXPORT_CLI_BINARY"
  if [[ "$BIN" != /* ]]; then BIN="$ROOT_DIR/$BIN"; fi
  if [[ ! -f "$BIN" || -L "$BIN" || ! -x "$BIN" ]]; then
    echo "UGOITE_SQL_EXPORT_CLI_BINARY must name an executable regular file: $BIN" >&2
    exit 1
  fi

  expected_cli_sha="${UGOITE_SOURCE_SHA:-$SOURCE_SHA}"
  if [[ ! "$expected_cli_sha" =~ ^[0-9a-fA-F]{40}$ || "$expected_cli_sha" != "$SOURCE_SHA" ]]; then
    echo "UGOITE_SOURCE_SHA must match the checked out source before using a verified CLI artifact" >&2
    exit 1
  fi
  if [[ ! -f "$BIN.source-sha" || -L "$BIN.source-sha" ]]; then
    echo "Verified SQL export CLI is missing its source SHA sidecar: $BIN.source-sha" >&2
    exit 1
  fi
  cli_source_sha="$(tr -d '[:space:]' <"$BIN.source-sha")"
  if [[ "$cli_source_sha" != "$expected_cli_sha" ]]; then
    echo "Verified SQL export CLI source SHA does not match this checkout" >&2
    exit 1
  fi
  export UGOITE_SQL_EXPORT_CLI_SOURCE_SHA="$cli_source_sha"

  if [[ -n "${UGOITE_SQL_EXPORT_CLI_LOAD_RESOURCE:-}" ]]; then
    if [[ ! -f "$UGOITE_SQL_EXPORT_CLI_LOAD_RESOURCE" || -L "$UGOITE_SQL_EXPORT_CLI_LOAD_RESOURCE" ]]; then
      echo "SQL export CLI artifact load resource file is missing: $UGOITE_SQL_EXPORT_CLI_LOAD_RESOURCE" >&2
      exit 1
    fi
    CLI_PROFILE_TIMED_STEP_ARGS=(
      --timed-step cli-artifact-load "$UGOITE_SQL_EXPORT_CLI_LOAD_RESOURCE"
    )
  fi
  if [[ -n "${UGOITE_SQL_EXPORT_CLI_TRANSFER_REPORT:-}" ]] \
    && [[ ! -f "$UGOITE_SQL_EXPORT_CLI_TRANSFER_REPORT" || -L "$UGOITE_SQL_EXPORT_CLI_TRANSFER_REPORT" ]]; then
    echo "SQL export CLI artifact transfer report is missing: $UGOITE_SQL_EXPORT_CLI_TRANSFER_REPORT" >&2
    exit 1
  fi
else
  BIN="$ROOT_DIR/target/rust/debug/ugoite"
  CLI_PROFILE_TIMED_STEP_ARGS=(--timed-step cli-build "$CLI_BUILD_RESOURCE")
fi

if [[ "$CLI_MODE" == "local-build" ]]; then
  echo "Building CLI and preparing the fixed ${FIXTURE_ENTRY_COUNT}-entry dataset..." >&2
  bash "$ROOT_DIR/scripts/measure-process-resources.sh" \
    "$CLI_BUILD_RESOURCE" cargo build --locked -p ugoite-cli
else
  echo "Using verified CLI artifact and preparing the fixed ${FIXTURE_ENTRY_COUNT}-entry dataset..." >&2
fi
if [[ -n "$FIXTURE_BUNDLE_DIR" ]]; then
  deno run -A "$ROOT_DIR/tools/cp1_fixture_bundle.ts" load export \
    --bundle-dir "$FIXTURE_BUNDLE_DIR" \
    --destination "$DATA_ROOT" \
    --xtask "${UGOITE_SEED_XTASK_BINARY:-}" \
    --source-sha "$SOURCE_SHA"
else
  seed_args=(
    --root "$DATA_ROOT"
    --space-id "$FIXTURE_SLUG"
    --scenario "$FIXTURE_SCENARIO"
    --entry-count "$FIXTURE_ENTRY_COUNT"
    --seed "$FIXTURE_SEED"
  )
  if [[ -n "$FIXTURE_OWNER_DISPLAY_NAME" ]]; then
    seed_args+=(--owner "$FIXTURE_OWNER_DISPLAY_NAME")
  fi
  bash "$ROOT_DIR/scripts/dev-seed.sh" "${seed_args[@]}" \
    --profile-output "$SEED_PROFILE"
fi

SPACE_UID="$(deno eval --quiet '
  const [spacesRoot, expectedSlug] = Deno.args;
  for await (const entry of Deno.readDir(spacesRoot)) {
    if (!entry.isDirectory) continue;
    try {
      const path = `${spacesRoot}/${entry.name}`;
      const meta = JSON.parse(await Deno.readTextFile(`${path}/meta.json`));
      if (meta.slug === expectedSlug && meta.space_uid === entry.name) {
        console.log(meta.space_uid);
        Deno.exit(0);
      }
    } catch { /* skip incomplete seed artifacts */ }
  }
  Deno.exit(1);
' -- "$SPACE_PATH" "$FIXTURE_SLUG")"
if [[ -z "$SPACE_UID" ]]; then
  echo "Seed command did not create a UUID-matched Space below $SPACE_PATH" >&2
  exit 1
fi
echo "Using seeded Space UID: $SPACE_UID" >&2

"$BIN" --config "$CONFIG" config init >/dev/null
"$BIN" --config "$CONFIG" config connection set local --type core --root "$DATA_ROOT" >/dev/null
"$BIN" --config "$CONFIG" context add measure --connection local --space "$SPACE_UID" >/dev/null
"$BIN" --config "$CONFIG" context use measure >/dev/null

FORMS_JSON="$("$BIN" --config "$CONFIG" form list)"
deno eval --quiet '
  const forms = JSON.parse(Deno.args[0]);
  if (!Array.isArray(forms) || forms.length === 0) throw new Error("seed has no Forms");
  const statements = forms.map((form) => {
    const id = String(form.id).replaceAll("-", "");
    if (!/^[0-9a-f]{32}$/i.test(id)) throw new Error(`invalid form id: ${form.id}`);
    return `SELECT _ugoite_id FROM "form_${id}"`;
  });
  console.log(`${statements.join(" UNION ALL ")} ORDER BY _ugoite_id`);
' -- "$FORMS_JSON" >"$SQL_FILE"

if [[ "$(uname -s)" == "Darwin" ]]; then
  TIME_ARGS=(-l)
else
  TIME_ARGS=(-v)
fi

for PAGE_SIZE in 100 1000; do
  RESULT="$OUTPUT_DIR/page-${PAGE_SIZE}.ndjson"
  SUMMARY="$OUTPUT_DIR/page-${PAGE_SIZE}.summary.json"
  TIME_LOG="$OUTPUT_DIR/page-${PAGE_SIZE}.time.txt"
  echo "Exporting with page size $PAGE_SIZE..." >&2
  /usr/bin/time "${TIME_ARGS[@]}" -o "$TIME_LOG" \
    "$BIN" --config "$CONFIG" sql export "$SQL_FILE" \
      --max-rows "$FIXTURE_ENTRY_COUNT" --page-size "$PAGE_SIZE" --output "$RESULT" >"$SUMMARY"
  ROWS="$(wc -l <"$RESULT" | tr -d '[:space:]')"
  EXPECTED_PAGES=$(( (FIXTURE_ENTRY_COUNT + PAGE_SIZE - 1) / PAGE_SIZE ))
  deno eval --quiet '
    const [summaryPath, rows, expectedPages, expectedRowsText] = Deno.args;
    const expectedRows = Number(expectedRowsText);
    const summary = JSON.parse(await Deno.readTextFile(summaryPath));
    if (Number(rows) !== expectedRows) throw new Error("expected " + expectedRows + " rows, got " + rows);
    if (summary.rows_exported !== expectedRows) throw new Error("summary row count: " + summary.rows_exported);
    if (summary.pages_fetched !== Number(expectedPages)) {
      throw new Error(`expected ${expectedPages} pages, got ${summary.pages_fetched}`);
    }
  ' -- "$SUMMARY" "$ROWS" "$EXPECTED_PAGES" "$FIXTURE_ENTRY_COUNT"
done

# Compare content independent of the order in which the engine returns equal keys.
deno eval --quiet '
  const [smallPath, largePath, expectedRowsText] = Deno.args;
  const expectedRows = Number(expectedRowsText);
  const canonical = async (path: string) => {
    const rows = (await Deno.readTextFile(path)).trimEnd().split("\n").filter(Boolean);
    const normalized = rows.map((row) => JSON.stringify(JSON.parse(row))).sort();
    const bytes = new TextEncoder().encode(normalized.join("\n"));
    const digest = await crypto.subtle.digest("SHA-256", bytes);
    return { rows: rows.length, sha256: [...new Uint8Array(digest)].map((b) => b.toString(16).padStart(2, "0")).join("") };
  };
  const small = await canonical(smallPath);
  const large = await canonical(largePath);
  if (small.rows !== expectedRows || large.rows !== expectedRows || small.sha256 !== large.sha256) {
    throw new Error("page-size exports differ: " + JSON.stringify({ small, large }));
  }
  console.log(JSON.stringify({ rows: small.rows, canonical_sha256: small.sha256 }));
' -- "$OUTPUT_DIR/page-100.ndjson" "$OUTPUT_DIR/page-1000.ndjson" "$FIXTURE_ENTRY_COUNT" >"$OUTPUT_DIR/equivalence.json"

# A byte-bound failure must be explicit and must not publish a partial file.
BYTE_LIMIT_OUTPUT="$OUTPUT_DIR/byte-limited.ndjson"
if "$BIN" --config "$CONFIG" sql export "$SQL_FILE" \
  --max-rows "$FIXTURE_ENTRY_COUNT" --max-bytes 1024 --page-size 1000 \
  --output "$BYTE_LIMIT_OUTPUT" \
  2>"$OUTPUT_DIR/byte-limit.stderr"; then
  echo "Expected the fixed fixture to exceed the 1,024-byte export bound" >&2
  exit 1
fi
if [[ -e "$BYTE_LIMIT_OUTPUT" ]]; then
  echo "A byte-limited export must not publish a partial destination" >&2
  exit 1
fi
if grep -q "max-bytes would be exceeded" "$OUTPUT_DIR/byte-limit.stderr"; then
  :
else
  echo "Byte-limited export failed for an unexpected reason" >&2
  cat "$OUTPUT_DIR/byte-limit.stderr" >&2
  exit 1
fi
if find "$OUTPUT_DIR" -maxdepth 1 -name '.ugoite-export-*' -print -quit | grep -q .; then
  echo "A failed byte-limited export left a temporary output file" >&2
  exit 1
fi

deno eval --quiet '
  const [output, sourceSha, spaceUid, dataRoot, fixtureSlug, scenario, seedText, entryCountText] = Deno.args;
  const seed = Number(seedText);
  const entryCount = Number(entryCountText);
  const readSummary = async (size: number) => JSON.parse(await Deno.readTextFile(`${output}/page-${size}.summary.json`));
  const equivalence = JSON.parse(await Deno.readTextFile(`${output}/equivalence.json`));
  const report = {
    source_sha: sourceSha,
    generated_at: new Date().toISOString(),
    fixture: {
      slug: fixtureSlug,
      seed,
      scenario,
      entry_count: entryCount,
      space_uid: spaceUid,
      space_root: dataRoot,
      forms: "all forms listed by `ugoite form list`",
    },
    query_file: "query.sql",
    query_semantics: "SELECT _ugoite_id from every seeded Form, UNION ALL, ordered by _ugoite_id",
    max_rows: entryCount,
    byte_limit_failure: {
      max_bytes: 1024,
      expected_error: "serialized NDJSON byte bound exceeded",
      published_partial_file: false,
      temporary_file_left: false,
    },
    runs: [
      { page_size: 100, summary: await readSummary(100), output: "page-100.ndjson", peak_rss_raw_log: "page-100.time.txt" },
      { page_size: 1000, summary: await readSummary(1000), output: "page-1000.ndjson", peak_rss_raw_log: "page-1000.time.txt" },
    ],
    output_equivalence: equivalence,
    measurement_limits: [
      "Local filesystem-backed core; this does not measure a Remote server or network.",
      "RSS is the CLI process maximum reported by the platform time utility; it excludes the server and seed process.",
      "This does not measure ACL revocation, credential expiry, or interruption/fault scenarios.",
    ],
  };
  await Deno.writeTextFile(`${output}/measurement.json`, JSON.stringify(report, null, 2) + "\n");
' -- "$OUTPUT_DIR" "$SOURCE_SHA" "$SPACE_UID" "$DATA_ROOT" "$FIXTURE_SLUG" "$FIXTURE_SCENARIO" "$FIXTURE_SEED" "$FIXTURE_ENTRY_COUNT"

echo "Measurement completed: $OUTPUT_DIR/measurement.json" >&2
echo "Raw outputs and platform time logs are retained in: $OUTPUT_DIR" >&2
if [[ "$KEEP_ROOT" == true ]]; then echo "Seeded Space root kept at: $MEASURE_ROOT" >&2; fi
