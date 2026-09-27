#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUTPUT_FILE="${UGOITE_STARTUP_MEASURE_OUTPUT:-$ROOT_DIR/target/startup-measurement.json}"
RUNS="${UGOITE_STARTUP_MEASURE_RUNS:-3}"
STARTUP_TIMEOUT_SECONDS="${UGOITE_STARTUP_MEASURE_TIMEOUT_SECONDS:-900}"
KEEP_ROOT=false
REUSE_ROOT="${UGOITE_STARTUP_MEASURE_REUSE_ROOT:-false}"
FIXTURE_MARKER_CONTENT='{"seed":{"startup-space-a":3146001,"startup-space-b":3144001},"entries":{"startup-space-a":6000,"startup-space-b":4000},"scenario":"renewable-ops","version":1}'

if [[ -n "${UGOITE_STARTUP_MEASURE_ROOT:-}" ]]; then
  MEASURE_ROOT="$UGOITE_STARTUP_MEASURE_ROOT"
  KEEP_ROOT=true
  mkdir -p "$MEASURE_ROOT"
  if [[ -n "$(find "$MEASURE_ROOT" -mindepth 1 -maxdepth 1 -print -quit)" ]]; then
    if [[ "$REUSE_ROOT" != true || ! -f "$MEASURE_ROOT/.ugoite-startup-measurement-fixture.json" ]] || \
      [[ "$(cat "$MEASURE_ROOT/.ugoite-startup-measurement-fixture.json")" != "$FIXTURE_MARKER_CONTENT" ]]; then
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
cleanup() {
  if [[ "$KEEP_ROOT" == false ]]; then rm -rf "$MEASURE_ROOT"; fi
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
  printf '%s' "$FIXTURE_MARKER_CONTENT" >"$MEASURE_ROOT/.ugoite-startup-measurement-fixture.json"
fi

echo "Building server from source..." >&2
cargo build --locked -p ugoite-server
SOURCE_SHA="${UGOITE_SOURCE_SHA:-$(git -C "$ROOT_DIR" rev-parse HEAD)}"
TARGET_DIR="${CARGO_TARGET_DIR:-target}"
if [[ "$TARGET_DIR" != /* ]]; then TARGET_DIR="$ROOT_DIR/$TARGET_DIR"; fi
SERVER="$TARGET_DIR/debug/ugoite-server"
NODE_SECRET_KEY="$(head -c 32 /dev/urandom | base64)"

for ((run = 1; run <= RUNS; run++)); do
  port=$((18400 + run))
  log_file="$LOG_DIR/run-$run.log"
  echo "Startup measurement run $run/$RUNS..." >&2
  UGOITE_ROOT="$MEASURE_ROOT" \
  UGOITE_SERVER_ADDRESS="127.0.0.1:$port" \
  UGOITE_STARTUP_METRICS=true \
  UGOITE_SOURCE_SHA="$SOURCE_SHA" \
  UGOITE_NODE_SECRET_KEY="$NODE_SECRET_KEY" \
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

python3 - "$OUTPUT_FILE" "$SOURCE_SHA" "$RUNS" "$MEASURE_ROOT" "$LOG_DIR" <<'PY'
import json
import platform
import sys
from datetime import datetime, timezone
from pathlib import Path

output, source_sha, runs, root, log_dir = sys.argv[1:]
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
    measurements.append({
        "run": run_number,
        "cache_state": "first_process" if run_number == 1 else "subsequent_process",
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
