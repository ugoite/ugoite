#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -ne 3 ]; then
  printf 'usage: %s <version> <target> <output-path>\n' "$0" >&2
  exit 1
fi

version="$1"
target="$2"
output_path="$3"
repo="${UGOITE_GITHUB_REPO:-ugoite/ugoite}"

cat >"${output_path}" <<EOF
#!/usr/bin/env bash
set -euo pipefail

version="\${UGOITE_VERSION:-${version}}"
target="\${UGOITE_TARGET_OVERRIDE:-${target}}"
repo="\${UGOITE_GITHUB_REPO:-${repo}}"
script_url="\${UGOITE_INSTALL_SCRIPT_URL:-https://raw.githubusercontent.com/\${repo}/v\${version}/scripts/install-ugoite-cli.sh}"

tmpdir="\$(mktemp -d)"
cleanup() {
  rm -rf "\${tmpdir}"
}
trap cleanup EXIT HUP INT TERM

curl -fsSL "\${script_url}" -o "\${tmpdir}/install-ugoite-cli.sh"
chmod +x "\${tmpdir}/install-ugoite-cli.sh"
export UGOITE_VERSION="\${version}"
export UGOITE_TARGET_OVERRIDE="\${target}"

bash "\${tmpdir}/install-ugoite-cli.sh" "\$@"
EOF

chmod +x "${output_path}"
