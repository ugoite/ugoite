#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUTPUT_FILE="${UGOITE_STARTUP_MEASURE_OUTPUT:-$ROOT_DIR/target/startup-measurement.json}"
RUNS="${UGOITE_STARTUP_MEASURE_RUNS:-3}"
STARTUP_TIMEOUT_SECONDS="${UGOITE_STARTUP_MEASURE_TIMEOUT_SECONDS:-900}"
TARGET_DIR="${UGOITE_STARTUP_MEASURE_TARGET_DIR:-${CARGO_TARGET_DIR:-target}}"
KEEP_ROOT=false
REUSE_ROOT="${UGOITE_STARTUP_MEASURE_REUSE_ROOT:-false}"
FIXTURE_MARKER_CONTENT='{"seed":{"startup-space-a":3146001,"startup-space-b":3144001},"entries":{"startup-space-a":6000,"startup-space-b":4000},"scenario":"renewable-ops","version":1}'

if [[ -n "${UGOITE_STARTUP_MEASURE_ROOT:-}" ]]; then
  MEASURE_ROOT="$UGOITE_STARTUP_MEASURE_ROOT"
  KEEP_ROOT=true
  mkdir -p "$MEASURE_ROOT"
  if [[ -n "$(find "$MEASURE_ROOT" -mindepth 1 -maxdepth 1 -print -quit)" ]]; then
    if [[ "$REUSE_ROOT" != true || ! -f "$MEASURE_ROOT/.ugoite-startup-measurement-fixture.json" ]] || \
      [[ ! -s "$MEASURE_ROOT/.ugoite-startup-measurement-fixture.json" ]]; then
      echo "Refusing to reuse an unrecognized startup measurement root: $MEASURE_ROOT" >&2
      exit 1
    fi
    echo "Reusing the previously verified startup measurement fixture." >&2
  else
    REUSE_ROOT=false
  fi
else
  MEASURE_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/ugoite-startup.XXXXXX")"
fi

if [[ "$OUTPUT_FILE" != /* ]]; then
  OUTPUT_FILE="$ROOT_DIR/$OUTPUT_FILE"
fi
mkdir -p "$(dirname "$OUTPUT_FILE")"
LOG_DIR="$(mktemp -d "${TMPDIR:-/tmp}/ugoite-startup-logs.XXXXXX")"
SECRET_FILE="${UGOITE_STARTUP_MEASURE_SECRET_FILE:-$MEASURE_ROOT.node-secret}"
cleanup() {
  if [[ "$KEEP_ROOT" == false ]]; then rm -rf "$MEASURE_ROOT"; fi
  if [[ "$KEEP_ROOT" == false ]]; then rm -f "$SECRET_FILE"; fi
  rm -rf "$LOG_DIR"
}
trap cleanup EXIT INT TERM

if [[ "$REUSE_ROOT" != true ]]; then
  echo "Seeding fixed 6,000 + 4,000 Entry startup fixture..." >&2
  bash "$ROOT_DIR/scripts/dev-seed.sh" \
    --root "$MEASURE_ROOT" --space-id startup-space-a \
    --owner "Startup Measurement Owner" --scenario renewable-ops \
    --entry-count 6000 --seed 3146001
  bash "$ROOT_DIR/scripts/dev-seed.sh" \
    --root "$MEASURE_ROOT" --space-id startup-space-b \
    --owner "Startup Measurement Owner" --scenario renewable-ops \
    --entry-count 4000 --seed 3144001
fi
python3 - "$MEASURE_ROOT" "$FIXTURE_MARKER_CONTENT" "$REUSE_ROOT" <<'PY'
import hashlib
import json
import sys
from pathlib import Path

root = Path(sys.argv[1])
expected = json.loads(sys.argv[2])
reuse = sys.argv[3] == "true"
marker_path = root / ".ugoite-startup-measurement-fixture.json"
digest = hashlib.sha256()
for path in sorted((root / "spaces").rglob("*")):
    if path.is_file():
        digest.update(path.relative_to(root).as_posix().encode())
        digest.update(b"\0")
        digest.update(path.read_bytes())
actual_digest = digest.hexdigest()
if reuse:
    marker = json.loads(marker_path.read_text())
    if any(marker.get(key) != value for key, value in expected.items()):
        raise SystemExit("Retained startup fixture does not match the expected seeds and counts")
    if marker.get("space_contents_sha256") != actual_digest:
        raise SystemExit("Retained startup fixture contents changed since it was seeded")
else:
    marker = {**expected, "space_contents_sha256": actual_digest}
    marker_path.write_text(json.dumps(marker, sort_keys=True) + "\n")
PY

if [[ "$REUSE_ROOT" == true && ! -f "$SECRET_FILE" ]]; then
  echo "A retained Node secret file is required to reuse the startup fixture: $SECRET_FILE" >&2
  exit 1
fi
if [[ ! -f "$SECRET_FILE" ]]; then
  (umask 077; head -c 32 /dev/urandom | base64 >"$SECRET_FILE")
fi
chmod 600 "$SECRET_FILE"
NODE_SECRET_KEY="$(cat "$SECRET_FILE")"

SOURCE_COMMIT_SHA="$(git -C "$ROOT_DIR" rev-parse HEAD)"
SOURCE_STATUS="$(git -C "$ROOT_DIR" status --porcelain --untracked-files=all)"
if [[ -n "$SOURCE_STATUS" ]]; then
  if [[ "${UGOITE_STARTUP_MEASURE_ALLOW_DIRTY:-false}" != true ]] || \
    ! git -C "$ROOT_DIR" diff --quiet -- || \
    [[ -n "$(git -C "$ROOT_DIR" ls-files --others --exclude-standard)" ]]; then
    echo "Startup measurements require a clean tree. To measure a fully staged source tree, set UGOITE_STARTUP_MEASURE_ALLOW_DIRTY=true." >&2
    exit 1
  fi
  SOURCE_SHA="$(git -C "$ROOT_DIR" write-tree)"
  SOURCE_ID_KIND="git_tree"
else
  SOURCE_SHA="$SOURCE_COMMIT_SHA"
  SOURCE_ID_KIND="git_commit"
fi
SOURCE_TREE_SHA="$(git -C "$ROOT_DIR" rev-parse HEAD^{tree})"
if [[ "$SOURCE_ID_KIND" == git_tree ]]; then SOURCE_TREE_SHA="$SOURCE_SHA"; fi

echo "Building server from source..." >&2
env -u CARGO_BUILD_BUILD_DIR \
  cargo build --locked --target-dir "$TARGET_DIR" -p ugoite-server
if [[ "$TARGET_DIR" != /* ]]; then TARGET_DIR="$ROOT_DIR/$TARGET_DIR"; fi
SERVER="$TARGET_DIR/debug/ugoite-server"

for ((run = 1; run <= RUNS; run++)); do
  port=$((18400 + run))
  log_file="$LOG_DIR/run-$run.log"
  echo "Startup measurement run $run/$RUNS..." >&2
  UGOITE_ROOT="$MEASURE_ROOT" \
  UGOITE_SERVER_ADDRESS="127.0.0.1:$port" \
  UGOITE_STARTUP_METRICS=true \
  UGOITE_SOURCE_SHA="$SOURCE_SHA" \
  UGOITE_NODE_SECRET_KEY="$NODE_SECRET_KEY" \
    env -u UGOITE_NODE_CONTROL_URI -u UGOITE_STORAGE_ENDPOINT \
      "$SERVER" >"$log_file" 2>&1 &
  server_pid=$!
  ready=false
  for _ in $(seq 1 "$((STARTUP_TIMEOUT_SECONDS * 10))"); do
    if curl --silent --fail "http://127.0.0.1:$port/health" >/dev/null; then
      ready=true
      break
    fi
    if ! kill -0 "$server_pid" 2>/dev/null; then break; fi
    sleep 0.1
  done
  if [[ "$ready" != true ]]; then
    sed -E 's/#secret=[^[:space:]]+/#secret=[REDACTED]/g' "$log_file" >&2
    kill "$server_pid" 2>/dev/null || true
    wait "$server_pid" 2>/dev/null || true
    echo "Server failed to reach /health during startup measurement" >&2
    exit 1
  fi
  kill -INT "$server_pid" 2>/dev/null || true
  wait "$server_pid" || true
done

python3 - "$OUTPUT_FILE" "$SOURCE_SHA" "$RUNS" "$MEASURE_ROOT" "$LOG_DIR" "$SOURCE_COMMIT_SHA" "$SOURCE_TREE_SHA" "$SOURCE_ID_KIND" <<'PY'
import json
import platform
import sys
from datetime import datetime, timezone
from pathlib import Path

args = sys.argv[1:]
output, source_sha, runs, root, log_dir = args[:5]
source_commit_sha, source_tree_sha, source_id_kind = args[5:]
measurements = []
audit_event_count = 0
audit_event_counts_by_space = {}
for path in sorted(Path(log_dir).glob("run-*.log")):
    run_number = int(path.stem.split("-")[-1])
    events = []
    for line in path.read_text(errors="replace").splitlines():
        try:
            value = json.loads(line)
        except json.JSONDecodeError:
            continue
        if value.get("event") == "ugoite.startup.phase":
            events.append(value)
    checkpoint_phase = next(
        (event for event in events if event.get("phase") == "audit_chain_verification"),
        None,
    )
    checkpoint_used = (
        checkpoint_phase.get("checkpoint_used") if checkpoint_phase else None
    )
    measurements.append({
        "run": run_number,
        "cache_state": "first_process" if run_number == 1 else "subsequent_process",
        "audit_checkpoint": (
            "verified" if checkpoint_used is True else
            "full_verification" if checkpoint_used is False else
            "not_reported"
        ),
        "phases": events,
    })
for audit_path in Path(root).glob("spaces/*/audit/events.jsonl"):
    event_count = sum(1 for line in audit_path.open() if line.strip())
    audit_event_count += event_count
    audit_event_counts_by_space[audit_path.parent.parent.name] = event_count
report = {
    "schema_version": 1,
    "measured_at": datetime.now(timezone.utc).isoformat(),
    "command": "mise run measure:startup",
    "source_sha": source_sha,
    "source_commit_sha": source_commit_sha,
    "source_tree_sha": source_tree_sha,
    "source_id_kind": source_id_kind,
    "fixture": {
        "seed": {"startup-space-a": 3146001, "startup-space-b": 3144001},
        "entries": 10000,
        "space_entry_counts": {"startup-space-a": 6000, "startup-space-b": 4000},
        "audit_event_count": audit_event_count,
        "audit_event_counts_by_space_uid": audit_event_counts_by_space,
    },
    "runner": {"os": platform.platform(), "architecture": platform.machine()},
    "process_runs": measurements,
    "scope": "Process-cold and subsequent-process timings; OS page cache is not controlled.",
}
Path(output).write_text(json.dumps(report, indent=2) + "\n")
print(f"Wrote startup measurement: {output}", file=sys.stderr)
PY

if [[ "$KEEP_ROOT" == true ]]; then
  echo "Seeded Space root kept at: $MEASURE_ROOT" >&2
fi
