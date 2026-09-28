# Frozen pre-binding Saved SQL history

This Space was written by the unmodified Ugoite `v0.2.0` CLI at source commit
`c40905a505f619aeb344ec96dc0f81b07a083bee`, before the Form-binding change
`f8fd520da7b27c3168a8e9978e2d3858ec9ee8bf`. That source has no
`bindingVersion` or `formBindings` fields and uses the ordinary `form save`,
`entry create`, `sql saved create`, and `sql saved update` paths. The tag's
domain constant defines Space version `0.1`.

The frozen Space contains an `Expense` Form, three synthetic Entries, and one
Saved SQL Entry with two authentic revisions. Revision 1 filters the physical
`form_<UUID>` relation using the saved integer variable `minimum`; revision 2
changes its variable and SQL to `maximum`. `expected.json` records the exact
source text, variables, revision IDs, parent, checksums, and signatures.
Generated lock files and CLI/Node control stores are excluded. The only
post-generation metadata edits are replacing the generated HMAC key/id before
saving records and removing any host-specific `storage_config`; all other
distributed Space files are copied byte-for-byte from the old CLI output.

The CLI-generated HMAC key was replaced immediately after `space create`,
before saving the Form, Entries, or Saved SQL, with a public fixture-only key.
Its Base64 value is
`cHVibGljLXByZWJpbmRpbmctZml4dHVyZS1rZXktMjAyNg==` (decoded value
`public-prebinding-fixture-key-2026`). This key is intentionally public and must
never be used for real data. The fixture contains no user data or credentials.
It is immutable test data: CI reads it and never regenerates it. `SHA256SUMS`
covers every distributed Space file; `expected.json` records the manifest and
payload digests.

## Reproduction

With the exact source tag checked out and its lockfile, build the old executable
with Rust 1.94 and SHA-256:

```sh
cargo build --locked -p ugoite-cli --bin ugoite
shasum -a 256 target/debug/ugoite
```

Create a fresh isolated CLI config and Space with the v0.2.0 executable. Run
from the repository root and set the config path explicitly so these commands do
not alter the operator's normal CLI config:

```sh
export GENERATION_ROOT="$(mktemp -d)"
export GENERATION_CONFIG="$GENERATION_ROOT/ugoite.toml"
export INPUT="$GENERATION_ROOT/inputs"
cp -R fixtures/spaces/0.1/pre-binding-sql-history/inputs "$INPUT"
ugoite --config "$GENERATION_CONFIG" config init
ugoite --config "$GENERATION_CONFIG" config connection set local --type core --root "$GENERATION_ROOT"
SPACE_UID="$(ugoite --config "$GENERATION_CONFIG" space create prebinding-sql-history --name 'Pre-binding SQL history' | python3 -c 'import json,sys; print(json.load(sys.stdin)["space"]["space_uid"])')"
python3 - "$GENERATION_ROOT/spaces/$SPACE_UID/meta.json" <<'PY'
import json,sys
from pathlib import Path
path=Path(sys.argv[1]); meta=json.loads(path.read_text())
meta['hmac_key']='cHVibGljLXByZWJpbmRpbmctZml4dHVyZS1rZXktMjAyNg=='
meta['hmac_key_id']='public-test-fixture-2026'
path.write_text(json.dumps(meta,indent=2)+'\n')
PY
ugoite --config "$GENERATION_CONFIG" form save "$INPUT/expense-form.json"
FORM_RELATION="$(ugoite --config "$GENERATION_CONFIG" form list | python3 -c 'import json,sys; print(next(x["sql_relation"] for x in json.load(sys.stdin) if x["name"]=="Expense"))')"
python3 - "$INPUT" "$FORM_RELATION" <<'PY'
import re,sys
from pathlib import Path
root=Path(sys.argv[1]); relation=sys.argv[2]
for path in root.glob('expense-v*.sql'):
    path.write_text(re.sub(r'form_[0-9a-f]{32}', relation, path.read_text()))
PY
ugoite --config "$GENERATION_CONFIG" entry create --id expense-01 --form Expense --fields-file "$INPUT/expense-01.json"
ugoite --config "$GENERATION_CONFIG" entry create --id expense-02 --form Expense --fields-file "$INPUT/expense-02.json"
ugoite --config "$GENERATION_CONFIG" entry create --id expense-03 --form Expense --fields-file "$INPUT/expense-03.json"
SQL_ID="$(ugoite --config "$GENERATION_CONFIG" sql saved create --name 'Expenses over threshold' --sql "$INPUT/expense-v1.sql" \
  --variables '[{"name":"minimum","type":"integer","description":"Minimum JPY amount"}]' | \
  python3 -c 'import json,sys; print(json.load(sys.stdin)["id"])')"
ugoite --config "$GENERATION_CONFIG" sql saved update "$SQL_ID" --sql "$INPUT/expense-v2.sql" \
  --variables '[{"name":"maximum","type":"integer","description":"Minimum JPY amount"}]'
```

`expected.json` and `SHA256SUMS` freeze the source tag/commit, generating
executable digest, Space and revision IDs, integrity values, and per-file
payload hashes. The Space payload digest hashes each sorted relative path, a
NUL separator, and the file bytes.
