# Space 0.1 compatibility fixture

This directory is frozen compatibility evidence for the first stable Space
generation. The bootstrap metadata and settings are the historical input; the
test opens that input through the current Space reader, exercises the Entry
Form and append-only history paths, closes and reopens the Space, and verifies
the same Knowledge meaning.

`space_version: "0.1"` is the only Space compatibility identity. Product
versions, subsystem-local schema versions, and physical Iceberg encoding are
independent concepts. A missing, malformed, or unsupported Space Version must
fail closed with `UNSUPPORTED_SPACE_VERSION`; the old `schema_version` field is
never inferred as an alias and open never performs migration.

## Archived v0.1.1 Asset object

`assets/01a1f765-628e-7cf0-8ac6-16e581b0d678` is the 43-byte object written
by the archived v0.1.1 Linux x86_64 CLI `asset upload` command. The release
tarball SHA-256 is
`7d1a102b0e12df268334f9a1e1530142300b40af754177f0ba168ec34d4d05f5`; its
sidecar checksum was verified before use. The executable reports
`ugoite 0.1.1`, and the corresponding source tag is commit
`3c26c8e8ad36fc02dec63fca543218b9bf0f05d8`. The release-generated object
was uploaded by that binary in an isolated core workspace using its
`space create` and `asset upload --filename historical-asset.bin` commands,
then copied byte-for-byte from
`spaces/01a1f765-6242-79c1-b6b0-c16523eaace1/assets/01a1f765-628e-7cf0-8ac6-16e581b0d678`;
the compatibility test stages it under the fixed Space ID above so the fixture
remains deterministic. Its object SHA-256 is recorded in `expected.json`. This
test covers the old
`assets/<asset_id>` location after current open and reopen; it does not claim
that the containing Space metadata was produced by the v0.1.1 CLI.
