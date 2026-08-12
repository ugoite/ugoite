#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: bash scripts/dev-seed.sh [--root PATH] [--space-id ID] [--scenario NAME] [--entry-count N] [--seed VALUE]

Create local sample data with the existing ugoite-cli sample-data command and
visible terminal progress.

Defaults:
  --root        ./data
  --space-id    dev-seed
  --scenario    renewable-ops
  --entry-count 50

Environment variable overrides:
  UGOITE_SEED_ROOT
  UGOITE_ROOT (fallback root shared with local dev backend)
  UGOITE_SEED_SPACE_ID
  UGOITE_SEED_SCENARIO
  UGOITE_SEED_ENTRY_COUNT
  UGOITE_SEED_VALUE
EOF
}

ROOT_PATH="${UGOITE_SEED_ROOT:-${UGOITE_ROOT:-./data}}"
SPACE_ID="${UGOITE_SEED_SPACE_ID:-dev-seed}"
SCENARIO="${UGOITE_SEED_SCENARIO:-renewable-ops}"
ENTRY_COUNT="${UGOITE_SEED_ENTRY_COUNT:-50}"
SEED_VALUE="${UGOITE_SEED_VALUE:-}"
CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-target/rust}"

while (($# > 0)); do
  case "$1" in
    --root)
      ROOT_PATH="${2:?missing value for --root}"
      shift 2
      ;;
    --space-id)
      SPACE_ID="${2:?missing value for --space-id}"
      shift 2
      ;;
    --scenario)
      SCENARIO="${2:?missing value for --scenario}"
      shift 2
      ;;
    --entry-count)
      ENTRY_COUNT="${2:?missing value for --entry-count}"
      shift 2
      ;;
    --seed)
      SEED_VALUE="${2:?missing value for --seed}"
      shift 2
      ;;
    -h | --help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

if ! [[ "$ENTRY_COUNT" =~ ^[0-9]+$ ]]; then
  echo "UGOITE_SEED_ENTRY_COUNT/--entry-count must be an integer: $ENTRY_COUNT" >&2
  exit 1
fi

if [[ -n "$SEED_VALUE" ]] && ! [[ "$SEED_VALUE" =~ ^[0-9]+$ ]]; then
  echo "UGOITE_SEED_VALUE/--seed must be an integer: $SEED_VALUE" >&2
  exit 1
fi

space_path_for_slug() {
  deno eval --quiet '
    const [spacesRoot, expectedSlug, requireCurrentIdentity] = Deno.args;
    const uuidV7 =
      /^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;
    let matchingSpace;

    try {
      for await (const entry of Deno.readDir(spacesRoot)) {
        if (!entry.isDirectory) continue;
        const spacePath = `${spacesRoot}/${entry.name}`;
        try {
          const meta = JSON.parse(
            await Deno.readTextFile(`${spacePath}/meta.json`),
          );
          if (requireCurrentIdentity === "true") {
            if (
              meta?.slug !== expectedSlug ||
              !uuidV7.test(entry.name) ||
              meta?.space_uid !== entry.name
            ) {
              continue;
            }
          } else if (
            entry.name !== expectedSlug &&
            meta?.slug !== expectedSlug
          ) {
            continue;
          }
          matchingSpace = spacePath;
          break;
        } catch {
          // Ignore incomplete or unrelated top-level directories.
        }
      }
    } catch {
      // The spaces directory does not exist yet.
    }

    if (matchingSpace) console.log(matchingSpace);
    else Deno.exit(1);
  ' -- "$ROOT_PATH/spaces" "$SPACE_ID" "${1:-false}"
}

if existing_space="$(space_path_for_slug)"; then
  echo "Refusing to overwrite existing local sample space: $existing_space" >&2
  echo "Choose a different space with UGOITE_SEED_SPACE_ID or --space-id." >&2
  exit 1
fi

echo "Seeding local sample data..." >&2
echo "  root: $ROOT_PATH" >&2
echo "  space: $SPACE_ID" >&2
echo "  scenario: $SCENARIO" >&2
echo "  entry_count: $ENTRY_COUNT" >&2
echo "  cargo_target_dir: $CARGO_TARGET_DIR" >&2
if [[ -n "$SEED_VALUE" ]]; then
  echo "  seed: $SEED_VALUE" >&2
fi

command=(
  env
  "CARGO_TARGET_DIR=$CARGO_TARGET_DIR"
  cargo
  run
  -q
  -p
  ugoite-cli
  --
  space
  sample-data
  --scenario
  "$SCENARIO"
  --entry-count
  "$ENTRY_COUNT"
)

if [[ -n "$SEED_VALUE" ]]; then
  command+=(--seed "$SEED_VALUE")
fi

command+=(-- "$ROOT_PATH" "$SPACE_ID")

"${command[@]}"

if ! created_space="$(space_path_for_slug true)"; then
  echo "Seed command finished but no Space with slug '$SPACE_ID' was found below: $ROOT_PATH/spaces" >&2
  exit 1
fi

echo "Verified seeded local sample space at $created_space" >&2
