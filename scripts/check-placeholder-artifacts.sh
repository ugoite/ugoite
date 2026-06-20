#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

placeholder_files=()
while IFS= read -r placeholder_file; do
  placeholder_files+=("$placeholder_file")
done < <(
  deno eval '
const sentinel = "This file is intentionally left blank.";
const entries = [];
for await (const entry of Deno.readDir(".")) {
  if (!entry.isFile || entry.name.startsWith(".")) continue;
  entries.push(entry.name);
}
for (const name of entries.sort()) {
  let text;
  try {
    text = await Deno.readTextFile(name);
  } catch {
    continue;
  }
  if (text.trim() === sentinel) console.log(name);
}
'
)

if [ "${#placeholder_files[@]}" -gt 0 ]; then
  echo "Found placeholder root artifacts that must be removed:" >&2
  printf "  - %s\n" "${placeholder_files[@]}" >&2
  exit 1
fi

echo "Repository root placeholder artifact check passed."
