# Saved SQL created before Form binding

This Space uses the on-disk output format produced by the Ugoite CLI at source
commit `eaa2b7d7f08e3c1598d1a82d07ae5861c4a86527`, the parent of the change that
introduced immutable Form bindings (`f8fd520da`). It is a frozen regression
fixture, not a revision assembled by current test helpers.

The historical CLI source was built with a deterministic, fixture-only Space
integrity key substituted for runtime-generated key material. That change
only makes the fixture reproducible and avoids storing generated credentials;
it does not alter the Knowledge records being tested. The key is test-only and
not used outside this fixture. The fixture contains an `Expense` Form with
integer JPY amounts, a purpose, and a date; three Entries; and one saved query
over the stable `form_<FormId>` relation. Its SQL revision has no Form binding
metadata. It contains no local CLI configuration.

`expected.json` records the Space, Form, Entry, SQL, and revision identities
used by the reader regression test. The SHA-256 of the sorted `SHA256SUMS`
manifest is `d6fc2541e14d5aca45d43128b79cc4748ba59cc6949d2de172055566e16d8703`; the test checks every listed Space file before
using the fixture.
