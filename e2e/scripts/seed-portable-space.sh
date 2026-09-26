#!/bin/bash
# Seed a portable Space through the local CLI before a server/Node sees it.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
E2E_STORAGE_ROOT="${1:?usage: seed-portable-space.sh STORAGE_ROOT}"
PORTABLE_CLI_CONFIG="${E2E_STORAGE_ROOT}.cli-config.toml"
PORTABLE_SPACE_SLUG="portable-e2e-$(date +%s)-$$"
PORTABLE_FORM_NAME="PortableAudit"

run_portable_cli() {
  cargo run -q --manifest-path "$ROOT_DIR/Cargo.toml" -p ugoite-cli --locked \
    -- --config "$PORTABLE_CLI_CONFIG" "$@"
}

mkdir -p "$E2E_STORAGE_ROOT"
(
  cd "$E2E_STORAGE_ROOT"
  run_portable_cli config init >/dev/null
  PORTABLE_SPACE_JSON="$(run_portable_cli space -o json create "$PORTABLE_SPACE_SLUG")"
  cat > "$E2E_STORAGE_ROOT/portable-form.json" <<EOF
{"name":"$PORTABLE_FORM_NAME","version":1,"template":"# PortableAudit\\n\\n## Subject\\n\\n## Body\\n","fields":{"Subject":{"type":"string","required":true},"Body":{"type":"markdown","required":false}}}
EOF
  run_portable_cli form -o json save "$E2E_STORAGE_ROOT/portable-form.json" >/dev/null
  PORTABLE_ENTRY_JSON="$(run_portable_cli entry -o json create --form "$PORTABLE_FORM_NAME" --field 'Subject=Seeded before Node startup' --field 'Body=portable history survives setup')"
  PORTABLE_CHANGES_JSON="$(run_portable_cli change -o json list)"
  E2E_STORAGE_ROOT="$E2E_STORAGE_ROOT" \
    PORTABLE_ENTRY_JSON="$PORTABLE_ENTRY_JSON" \
    PORTABLE_CHANGES_JSON="$PORTABLE_CHANGES_JSON" \
    PORTABLE_SPACE_JSON="$PORTABLE_SPACE_JSON" \
    PORTABLE_SPACE_SLUG="$PORTABLE_SPACE_SLUG" \
    PORTABLE_FORM_NAME="$PORTABLE_FORM_NAME" \
    deno eval '
      const space = JSON.parse(Deno.env.get("PORTABLE_SPACE_JSON")!);
      const entry = JSON.parse(Deno.env.get("PORTABLE_ENTRY_JSON")!);
      const changes = JSON.parse(Deno.env.get("PORTABLE_CHANGES_JSON")!);
      await Deno.writeTextFile(`${Deno.env.get("E2E_STORAGE_ROOT")}/portable-space-proof.json`, JSON.stringify({
        space_uid: space.space.space_uid,
        slug: Deno.env.get("PORTABLE_SPACE_SLUG"),
        form_name: Deno.env.get("PORTABLE_FORM_NAME"),
        entry_id: entry.id,
        revision_id: entry.revision_id,
        changes,
      }));
    '
)

printf '%s\n' "$PORTABLE_CLI_CONFIG"
