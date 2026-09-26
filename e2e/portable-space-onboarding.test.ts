import { expect, test } from "@playwright/test";
import { getBackendUrl, waitForServers } from "./lib/client.ts";

type PortableProof = {
  space_uid: string;
  slug: string;
  form_name: string;
  entry_id: string;
  revision_id: string;
  changes: Array<Record<string, unknown>>;
};

function changeId(row: Record<string, unknown>): string | undefined {
  if (typeof row.change_id === "string") return row.change_id;
  const change = row.change;
  return change && typeof change === "object" &&
      typeof (change as Record<string, unknown>).change_id === "string"
    ? (change as Record<string, string>).change_id
    : undefined;
}

test("first Passkey setup claims the CLI Space without changing its history", async ({ request }) => {
  const proofFile = Deno.env.get("E2E_PORTABLE_PROOF_FILE");
  expect(proofFile).toBeTruthy();
  const proof = JSON.parse(
    await Deno.readTextFile(proofFile!),
  ) as PortableProof;
  expect(proof.space_uid).toBeTruthy();
  expect(proof.changes.length).toBeGreaterThan(0);
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
});
