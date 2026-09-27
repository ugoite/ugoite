import { expect, test } from "@playwright/test";
import { getBackendUrl, waitForServers } from "./lib/client.ts";

type PortableProof = {
  schema_version: number;
  source_sha: string;
  working_tree_dirty: boolean;
  fixture_seed: string;
  runner_command: string;
  environment: { os: string; arch: string; storage_backend: string };
  verification: {
    source: { status: string; authorization: string };
    copied: { status: string; authorization: string };
    partial: { status: string };
    corrupt_authorization: { status: string };
    foreign_authorization: { status: string };
  };
  space_uid: string;
  slug: string;
  form_name: string;
  entry_id: string;
  revision_id: string;
  asset_form_name: string;
  asset_entry_id: string;
  asset_id: string;
  asset_sha256: string;
  saved_sql_id: string;
  saved_sql_revision_id: string;
  changes: Array<Record<string, unknown>>;
  source_files: Record<string, string>;
};

test("fresh Node claims a copied Space, reads its Knowledge, and appends without rewriting history", async ({ request }) => {
  const proofFile = Deno.env.get("E2E_PORTABLE_PROOF_FILE");
  expect(proofFile).toBeTruthy();
  const proof = JSON.parse(
    await Deno.readTextFile(proofFile!),
  ) as PortableProof;
  expect(proof.space_uid).toBeTruthy();
  expect(proof.schema_version).toBe(1);
  expect(proof.source_sha).toMatch(/^[0-9a-f]{40}$/);
  expect(proof.fixture_seed).toBeTruthy();
  expect(proof.environment.os).toBeTruthy();
  expect(proof.environment.arch).toBeTruthy();
  expect(proof.verification.source.authorization).toBe("incomplete");
  expect(proof.verification.copied.authorization).toBe("incomplete");
  expect(["valid", "valid_with_rebuildable_derived_state"]).toContain(
    proof.verification.source.status,
  );
  expect(["valid", "valid_with_rebuildable_derived_state"]).toContain(
    proof.verification.copied.status,
  );
  expect(["invalid", "incomplete"]).toContain(
    proof.verification.partial.status,
  );
  expect(proof.verification.corrupt_authorization.status).toBe("invalid");
  expect(proof.verification.foreign_authorization.status).toBe("invalid");
  expect(proof.changes.length).toBeGreaterThan(0);
  console.log(
    "Portable recovery evidence:",
    JSON.stringify({
      source_sha: proof.source_sha,
      working_tree_dirty: proof.working_tree_dirty,
      fixture_seed: proof.fixture_seed,
      runner_command: proof.runner_command,
      environment: proof.environment,
      verification: proof.verification,
    }),
  );
  await waitForServers(request);

  const spacesResponse = await request.get(getBackendUrl("/spaces"));
  expect(spacesResponse.ok()).toBe(true);
  const spaces = await spacesResponse.json() as Array<{
    space_uid?: string;
    slug?: string;
  }>;
  expect(spaces).toEqual(expect.arrayContaining([
    expect.objectContaining({
      space_uid: proof.space_uid,
      slug: proof.slug,
    }),
  ]));

  const formResponse = await request.get(
    getBackendUrl(
      `/spaces/${proof.space_uid}/forms/${encodeURIComponent(proof.form_name)}`,
    ),
  );
  expect(formResponse.ok()).toBe(true);
  expect(await formResponse.json()).toMatchObject({ name: proof.form_name });

  const entryHistoryResponse = await request.get(
    getBackendUrl(
      `/spaces/${proof.space_uid}/entries/${proof.entry_id}/history`,
    ),
  );
  expect(entryHistoryResponse.ok()).toBe(true);
  const entryHistory = await entryHistoryResponse.json() as {
    revisions?: Array<{ revision_id?: string }>;
  };
  expect(entryHistory.revisions?.map((revision) => revision.revision_id))
    .toEqual(
      [proof.revision_id],
    );

  const changesResponse = await request.get(
    getBackendUrl(`/spaces/${proof.space_uid}/changes`),
  );
  expect(changesResponse.ok()).toBe(true);
  const changes = await changesResponse.json() as Array<
    Record<string, unknown>
  >;
  expect(changes).toEqual(proof.changes);

  const assetEntryResponse = await request.get(
    getBackendUrl(`/spaces/${proof.space_uid}/entries/${proof.asset_entry_id}`),
  );
  expect(assetEntryResponse.ok()).toBe(true);
  expect(await assetEntryResponse.json()).toMatchObject({
    id: proof.asset_entry_id,
    form: proof.asset_form_name,
  });

  const assetResponse = await request.get(
    getBackendUrl(
      `/spaces/${proof.space_uid}/assets/${proof.asset_id}?form=${
        encodeURIComponent(proof.asset_form_name)
      }&entry_id=${encodeURIComponent(proof.asset_entry_id)}`,
    ),
  );
  expect(assetResponse.ok()).toBe(true);
  const assetBytes = await assetResponse.body();
  expect(
    new Uint8Array(
      await crypto.subtle.digest("SHA-256", new Uint8Array(assetBytes).buffer),
    ),
  )
    .toEqual(
      Uint8Array.from(
        proof.asset_sha256.match(/.{2}/g)!,
        (byte) => Number.parseInt(byte, 16),
      ),
    );

  const savedSqlResponse = await request.get(
    getBackendUrl(`/spaces/${proof.space_uid}/sql/${proof.saved_sql_id}`),
  );
  expect(savedSqlResponse.ok()).toBe(true);
  expect(await savedSqlResponse.json()).toMatchObject({
    id: proof.saved_sql_id,
    revision_id: proof.saved_sql_revision_id,
    name: "portable-recovery",
    sql: "SELECT 1 AS recovery_check",
  });

  const appendResponse = await request.post(
    getBackendUrl(`/spaces/${proof.space_uid}/entries`),
    {
      data: {
        form: proof.form_name,
        fields: {
          Subject: "Appended by the fresh Node",
          Body: "The imported revision remains intact.",
        },
      },
    },
  );
  expect(appendResponse.status()).toBe(201);
  const appended = await appendResponse.json() as {
    id: string;
    revision_id: string;
    change_id: string;
  };
  expect(appended.id).toBeTruthy();
  expect(appended.revision_id).toBeTruthy();
  expect(appended.change_id).toBeTruthy();

  const afterAppendChangesResponse = await request.get(
    getBackendUrl(`/spaces/${proof.space_uid}/changes`),
  );
  expect(afterAppendChangesResponse.ok()).toBe(true);
  const afterAppendChanges = await afterAppendChangesResponse.json() as Array<
    Record<string, unknown>
  >;
  expect(afterAppendChanges.length).toBeGreaterThan(proof.changes.length);
  expect(afterAppendChanges.at(-1)).toMatchObject({
    change_id: appended.change_id,
  });
  expect(afterAppendChanges.slice(0, proof.changes.length)).toEqual(
    proof.changes,
  );

  const originalHistoryAfterAppend = await request.get(
    getBackendUrl(
      `/spaces/${proof.space_uid}/entries/${proof.entry_id}/history`,
    ),
  );
  expect(originalHistoryAfterAppend.ok()).toBe(true);
  const preservedHistory = await originalHistoryAfterAppend.json() as {
    revisions?: Array<{ revision_id?: string }>;
  };
  expect(preservedHistory.revisions?.map((revision) => revision.revision_id))
    .toEqual([proof.revision_id]);
});
