#!/usr/bin/env bash
set -euo pipefail

mode="${1:-check}"
apply_fixes=false
if [ "$mode" = "--fix" ] || [ "$mode" = "fix" ]; then
  apply_fixes=true
elif [ "$mode" != "check" ]; then
  echo "Usage: scripts/check-shell-quality.sh [check|--fix]" >&2
  exit 1
fi

if ! command -v shfmt >/dev/null 2>&1; then
  echo "shfmt is required. Install with: sudo apt-get install shfmt" >&2
  exit 1
fi
if ! command -v shellcheck >/dev/null 2>&1; then
  echo "shellcheck is required. Install with: sudo apt-get install shellcheck" >&2
  exit 1
fi

mapfile -t script_paths < <(
  find scripts e2e/scripts -maxdepth 1 -type f -name "*.sh" | sort
)

if [ "${#script_paths[@]}" -eq 0 ]; then
  echo "No shell scripts found under scripts/ or e2e/scripts/."
  exit 0
fi

if [ "$apply_fixes" = true ]; then
  shfmt -w -i 2 -ci "${script_paths[@]}"
else
  shfmt -d -i 2 -ci "${script_paths[@]}"
fi

shellcheck "${script_paths[@]}"

for script_path in "${script_paths[@]}"; do
  bash -n "$script_path"
  shebang="$(head -n 1 "$script_path")"
  case "$shebang" in
    "#!/usr/bin/env bash" | "#!/bin/bash") ;;
    *)
      echo "Invalid shebang in ${script_path}: ${shebang}" >&2
      exit 1
      ;;
  esac
done

echo "Shell quality checks passed for ${#script_paths[@]} scripts."
