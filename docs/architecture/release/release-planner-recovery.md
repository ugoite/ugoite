---
title: "Release planner ref recovery"
sidebar:
  order: 4
---

This runbook is for recovering an orphaned release-planner branch ref. It is
not part of normal candidate creation or release promotion, and it does not
introduce an automated release planner.

## Safety boundary

Recovery accepts only an exact `refs/heads/...` ref and an operator-supplied
40-character commit SHA. Tags, published releases, release assets, and
container or package aliases are outside this procedure and have no deletion
path here.

The command is a dry run unless `--delete` is provided. With `--delete`, it
reads the branch ref once to identify the observed commit, then reads it again
immediately before the delete request. If either observation differs from the
expected SHA, it aborts without sending a delete request.

The final comparison narrows the race window but is not an atomic conditional
delete: the GitHub ref API does not attach an expected SHA to deletion. If the
branch may change concurrently, stop the planner or coordinate with its owner
before using `--delete`, and rerun the dry run after the branch is stable.

## Procedure

1. Record the exact branch ref and commit SHA from the planner incident. Do
   not infer a ref from a tag or a published release.
2. Run the dry run and inspect the reported `observed_sha`:

   ```bash
   bash scripts/recover-release-planner-ref.sh \
     --repo OWNER/REPO \
     --ref refs/heads/RELEASE_PLANNER_BRANCH \
     --expected-sha 0123456789abcdef0123456789abcdef01234567
   ```

3. Only when the observed commit is the recorded orphaned commit, rerun the
   same command with `--delete`. The command performs the final comparison and
   deletes only that branch ref.
4. Confirm that the ref is gone with a read-only `gh api` lookup. Do not delete
   tags, GitHub Releases, release assets, or mutable aliases as part of this
   recovery.

Normal release promotion remains available through the candidate verification
and promotion workflow; this recovery procedure does not alter that path.
