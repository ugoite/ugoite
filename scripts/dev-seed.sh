#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

usage() {
  cat <<'EOF'
Usage: bash scripts/dev-seed.sh [--root PATH] [--space-id ID] [--scenario NAME] [--entry-count N] [--seed VALUE] [--owner NAME] [--profile-output PATH]

Create local sample data with the xtask dev seed command and
visible terminal progress.

Defaults:
  --root        ./data
  --space-id    dev-seed
  --scenario    renewable-ops
  --entry-count 50
  --owner       omit to create a Space without authorization owner metadata

Environment variable overrides:
  UGOITE_SEED_ROOT
  UGOITE_ROOT (fallback root shared with local dev backend)
  UGOITE_SEED_SPACE_ID
  UGOITE_SEED_SCENARIO
  UGOITE_SEED_ENTRY_COUNT
  UGOITE_SEED_VALUE
  UGOITE_SEED_PROFILE_OUTPUT
  UGOITE_SEED_XTASK_BINARY
EOF
}

SEED_ROOT="${UGOITE_SEED_ROOT:-${UGOITE_ROOT:-./data}}"
SPACE_ID="${UGOITE_SEED_SPACE_ID:-dev-seed}"
SCENARIO="${UGOITE_SEED_SCENARIO:-renewable-ops}"
ENTRY_COUNT="${UGOITE_SEED_ENTRY_COUNT:-50}"
SEED_VALUE="${UGOITE_SEED_VALUE:-}"
OWNER_DISPLAY_NAME="${UGOITE_SEED_OWNER:-}"
PROFILE_OUTPUT="${UGOITE_SEED_PROFILE_OUTPUT:-}"
CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-target/rust}"
XTASK_BINARY="${UGOITE_SEED_XTASK_BINARY:-}"

while (($# > 0)); do
  case "$1" in
    --root)
      SEED_ROOT="${2:?missing value for --root}"
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
    --owner)
      OWNER_DISPLAY_NAME="${2:?missing value for --owner}"
      shift 2
      ;;
    --profile-output)
      PROFILE_OUTPUT="${2:?missing value for --profile-output}"
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
  ' -- "$SEED_ROOT/spaces" "$SPACE_ID" "${1:-false}"
}

if existing_space="$(space_path_for_slug)"; then
  echo "Refusing to overwrite existing local sample space: $existing_space" >&2
  echo "Choose a different space with UGOITE_SEED_SPACE_ID or --space-id." >&2
  exit 1
fi

echo "Seeding local sample data..." >&2
echo "  root: $SEED_ROOT" >&2
echo "  space: $SPACE_ID" >&2
echo "  scenario: $SCENARIO" >&2
echo "  entry_count: $ENTRY_COUNT" >&2
echo "  cargo_target_dir: $CARGO_TARGET_DIR" >&2
if [[ -n "$XTASK_BINARY" ]]; then
  echo "  xtask_binary: $XTASK_BINARY" >&2
fi
if [[ -n "$SEED_VALUE" ]]; then
  echo "  seed: $SEED_VALUE" >&2
fi

if [[ -n "$XTASK_BINARY" ]]; then
  if [[ "$XTASK_BINARY" != /* ]]; then
    XTASK_BINARY="$ROOT_DIR/$XTASK_BINARY"
  fi
  if [[ ! -f "$XTASK_BINARY" || -L "$XTASK_BINARY" || ! -x "$XTASK_BINARY" ]]; then
    echo "UGOITE_SEED_XTASK_BINARY must name an executable regular file: $XTASK_BINARY" >&2
    exit 1
  fi
  expected_source_sha="$(git -C "$ROOT_DIR" rev-parse HEAD)"
  if [[ ! "$expected_source_sha" =~ ^[0-9a-f]{40}$ ]]; then
    echo "Could not resolve the checked out source SHA for the explicit xtask binary" >&2
    exit 1
  fi
  if [[ -n "${UGOITE_SOURCE_SHA:-}" && "$UGOITE_SOURCE_SHA" != "$expected_source_sha" ]]; then
    echo "UGOITE_SOURCE_SHA must match the checked out source before using an explicit xtask binary" >&2
    exit 1
  fi
  if [[ ! -f "$XTASK_BINARY.source-sha" || -L "$XTASK_BINARY.source-sha" ]]; then
    echo "Explicit xtask binary is missing its source SHA sidecar: $XTASK_BINARY.source-sha" >&2
    exit 1
  fi
  xtask_source_sha="$(tr -d '[:space:]' <"$XTASK_BINARY.source-sha")"
  if [[ "$xtask_source_sha" != "$expected_source_sha" ]]; then
    echo "Explicit xtask binary source SHA does not match this checkout" >&2
    exit 1
  fi
  command=(
    "$XTASK_BINARY"
    seed
    --root
    "$SEED_ROOT"
    --space-id
    "$SPACE_ID"
    --scenario
    "$SCENARIO"
    --entry-count
    "$ENTRY_COUNT"
  )
else
  command=(
    env
    "CARGO_TARGET_DIR=$CARGO_TARGET_DIR"
    cargo
    run
    -q
    -p
    xtask
    --
    seed
    --root
    "$SEED_ROOT"
    --space-id
    "$SPACE_ID"
    --scenario
    "$SCENARIO"
    --entry-count
    "$ENTRY_COUNT"
  )
fi

if [[ -n "$SEED_VALUE" ]]; then
  command+=(--seed "$SEED_VALUE")
fi
if [[ -n "$OWNER_DISPLAY_NAME" ]]; then
  command+=(--owner "$OWNER_DISPLAY_NAME")
fi
if [[ -n "$PROFILE_OUTPUT" ]]; then
  command+=(--profile-output "$PROFILE_OUTPUT")
fi

if [[ -n "$PROFILE_OUTPUT" ]]; then
  RESOURCE_OUTPUT="${UGOITE_SEED_RESOURCE_OUTPUT:-${PROFILE_OUTPUT%.json}.time.txt}"
  bash "$ROOT_DIR/scripts/measure-process-resources.sh" \
    "$RESOURCE_OUTPUT" "${command[@]}"
else
  "${command[@]}"
fi

if ! created_space="$(space_path_for_slug true)"; then
  echo "Seed command finished but no Space with slug '$SPACE_ID' was found below: $SEED_ROOT/spaces" >&2
  exit 1
fi

echo "Verified seeded local sample space at $created_space" >&2
