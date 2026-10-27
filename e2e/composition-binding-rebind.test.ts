import { expect, test } from "@playwright/test";
import { parse as parseYaml } from "yaml";
import { getBackendUrl, getFrontendUrl, waitForServers } from "./lib/client.ts";

type FormField = { sql_column?: string };
type FormSummary = {
  id?: string;
  name?: string;
  sql_relation?: string;
  fields?: Record<string, FormField>;
};
type SavedSql = { id?: string; revision_id?: string; name?: string };
type SavedRevision = {
  revision: { entry_id: string; revision_id: string };
  fields: Record<string, unknown>;
};

const sqlIdentifier = (value: string) => `"${value.replaceAll('"', '""')}"`;
const slug = (value: string) =>
  value.normalize("NFKD").toLowerCase()
    .replace(/\p{M}/gu, "").replace(/[^a-z0-9]+/g, "_")
    .replace(/^_+|_+$/g, "");

test("Composition rebinds one shared Form table to exact Saved SQL and reopens both", async ({ page, request }) => {
  test.setTimeout(120_000);
  await waitForServers(request);
  const suffix = crypto.randomUUID().slice(0, 8);

  // A new server-backed Space keeps this journey's Knowledge isolated from
  // other acceptance fixtures and from the user's default Space.
  const spaceResponse = await request.post(getBackendUrl("/spaces"), {
    data: {
      slug: `composition-rebind-${suffix}`,
      name: `Composition Rebind ${suffix}`,
    },
  });
  expect([200, 201]).toContain(spaceResponse.status());
  const space = await spaceResponse.json() as { space_uid?: string };
  expect(space.space_uid).toBeTruthy();
  const spaceId = space.space_uid!;

  const formName = `RebindForm${suffix}`;
  const formResponse = await request.post(
    getBackendUrl(`/spaces/${spaceId}/forms`),
    {
      data: {
        name: formName,
        version: 1,
        template: `# ${formName}\n\n## Item\n\n## Amount\n`,
        fields: {
          Item: { type: "string", required: true },
          Amount: { type: "integer", required: true },
        },
      },
    },
  );
  expect([200, 201]).toContain(formResponse.status());
  for (
    const [item, amount] of [["First shared row", 11], [
      "Second shared row",
      22,
    ]] as const
  ) {
    const entry = await request.post(
      getBackendUrl(`/spaces/${spaceId}/entries`),
      { data: { form: formName, fields: { Item: item, Amount: amount } } },
    );
    expect(entry.status()).toBe(201);
  }

  const formsResponse = await request.get(
    getBackendUrl(`/spaces/${spaceId}/forms`),
  );
  expect(formsResponse.ok()).toBe(true);
  const forms = await formsResponse.json() as FormSummary[];
  const form = forms.find((candidate) => candidate.name === formName);
  expect(form?.id).toBeTruthy();
  expect(form?.sql_relation).toBeTruthy();
  const itemColumn = form?.fields?.Item?.sql_column;
  const amountColumn = form?.fields?.Amount?.sql_column;
  expect(itemColumn).toBeTruthy();
  expect(amountColumn).toBeTruthy();

  const savedSqlName = `Rebound table ${suffix}`;
  const sqlResponse = await request.post(
    getBackendUrl(`/spaces/${spaceId}/sql`),
    {
      data: {
        name: savedSqlName,
        kind: "user-query",
        sql: `SELECT ${sqlIdentifier(itemColumn!)} AS item, ${
          sqlIdentifier(amountColumn!)
        } AS amount FROM ${sqlIdentifier(form!.sql_relation!)} ORDER BY ${
          sqlIdentifier(amountColumn!)
        } ASC`,
        variables: [],
      },
    },
  );
  expect([200, 201]).toContain(sqlResponse.status());
  const savedSql = await sqlResponse.json() as SavedSql;
  expect(savedSql.id).toBeTruthy();
  expect(savedSql.revision_id).toBeTruthy();

  const formVariableId = `${slug(formName)}_rows`;
  // Creating Saved SQL materializes the reserved SQL metadata Form. Snapshot
  // after that user-owned source exists, so the later delta isolates only the
  // first-use Composition registry provisioning.
  const formsBeforeResponse = await request.get(
    getBackendUrl(`/spaces/${spaceId}/forms`),
  );
  expect(formsBeforeResponse.ok()).toBe(true);
  const formsBefore = await formsBeforeResponse.json() as FormSummary[];
  expect(formsBefore.filter(({ name }) => name === "SQL")).toHaveLength(1);
  expect(
    formsBefore.filter(({ name }) => name === "_ugoite_compositions"),
  ).toHaveLength(0);
  const formIdsBefore = formsBefore.map(({ id }) => id).sort();
  const userFormIdsBefore = formsBefore.filter(({ name }) =>
    name !== "SQL" && name !== "User" && name !== "UserGroup" &&
    name !== "_ugoite_compositions"
  ).map(({ id }) => id).sort();
  const sqlListBefore = await request.get(
    getBackendUrl(`/spaces/${spaceId}/sql`),
  );
  expect(sqlListBefore.ok()).toBe(true);
  const savedSqlSourcesBefore = (await sqlListBefore.json() as SavedSql[])
    .map(({ id, revision_id }) => ({ id, revision_id }))
    .sort((left, right) => String(left.id).localeCompare(String(right.id)));
  expect(savedSqlSourcesBefore).toContainEqual({
    id: savedSql.id,
    revision_id: savedSql.revision_id,
  });

  await page.goto(
    getFrontendUrl(`/spaces/${spaceId}/compositions/new`),
    { waitUntil: "domcontentloaded" },
  );
  await expect(page.getByPlaceholder("Composition name")).toBeVisible();
  const compositionName = `Shared source rebind ${suffix}`;
  await page.getByLabel("Name", { exact: true }).fill(compositionName);
  const palette = page.getByRole("group", { name: "Add to design" });

  // First view is bound to the Form. The second view explicitly reuses its
  // query variable so the later source change must affect only one reference.
  await palette.getByRole("button", { name: "Table", exact: true }).click();
  const formPicker = page.getByRole("dialog", { name: "Table", exact: true });
  await formPicker.getByRole("tab", { name: "Forms", exact: true }).click();
  await formPicker.getByRole("button", { name: formName, exact: true }).click();
  await expect(formPicker).toHaveCount(0);

  await palette.getByRole("button", { name: "Table", exact: true }).click();
  const reusePicker = page.getByRole("dialog", {
    name: "Table",
    exact: true,
  });
  await reusePicker.getByRole("tab", { name: "Variables", exact: true })
    .click();
  await reusePicker.getByRole("button", {
    name: formVariableId,
    exact: true,
  }).click();
  await expect(reusePicker).toHaveCount(0);

  const componentOne = page.locator(
    '.compositionGraphValue[data-component-id="component_1"]',
  );
  const componentTwo = page.locator(
    '.compositionGraphValue[data-component-id="component_2"]',
  );
  await expect(componentOne.getByRole("table")).toContainText(
    "First shared row",
    {
      timeout: 30_000,
    },
  );
  await expect(componentTwo.getByRole("table")).toContainText(
    "Second shared row",
    {
      timeout: 30_000,
    },
  );

  await componentOne.getByRole("button", {
    name: `table, ${formVariableId}`,
    exact: true,
  }).click();
  const inspector = page.getByRole("complementary", {
    name: "Selected component",
    exact: true,
  });
  await inspector.getByRole("button", {
    name: `Change variable, ${formVariableId}`,
    exact: true,
  }).click();
  const rebindPicker = page.getByRole("dialog", {
    name: "Table",
    exact: true,
  });
  await rebindPicker.getByRole("tab", { name: "Saved SQL", exact: true })
    .click();
  await rebindPicker.getByRole("button", {
    name: savedSqlName,
    exact: true,
  }).click();
  await expect(rebindPicker).toHaveCount(0, { timeout: 30_000 });

  await expect(componentOne.getByRole("table")).toContainText(
    "First shared row",
    {
      timeout: 30_000,
    },
  );
  await expect(componentTwo.getByRole("table")).toContainText(
    "Second shared row",
    {
      timeout: 30_000,
    },
  );
  await expect(componentTwo.getByRole("table")).toHaveAccessibleName(
    `${formVariableId} results`,
  );

  const saveResponsePromise = page.waitForResponse((response) =>
    response.request().method() === "POST" &&
    new URL(response.url()).pathname.endsWith(
      `/spaces/${spaceId}/compositions`,
    )
  );
  await page.getByRole("button", { name: "Save", exact: true }).click();
  const saveResponse = await saveResponsePromise;
  expect([200, 201]).toContain(saveResponse.status());
  const saved = await saveResponse.json() as {
    composition_id: string;
    revision_id: string;
  };
  expect(saved.composition_id).toBeTruthy();
  expect(saved.revision_id).toBeTruthy();
  await expect(page).toHaveURL(
    new RegExp(
      `/spaces/${spaceId}/compositions/${saved.composition_id}/${saved.revision_id}$`,
    ),
    { timeout: 30_000 },
  );

  const historyResponse = await request.get(
    getBackendUrl(
      `/spaces/${spaceId}/compositions/${saved.composition_id}/history?limit=100&offset=0`,
    ),
  );
  expect(historyResponse.ok()).toBe(true);
  const history = await historyResponse.json() as {
    total: number;
    revisions: Array<{ revision: { revision_id: string } }>;
  };
  expect(history.total).toBe(1);
  expect(history.revisions.map(({ revision }) => revision.revision_id)).toEqual(
    [
      saved.revision_id,
    ],
  );

  const revisionResponse = await request.get(
    getBackendUrl(
      `/spaces/${spaceId}/compositions/${saved.composition_id}/history/${saved.revision_id}`,
    ),
  );
  expect(revisionResponse.ok()).toBe(true);
  const revision = await revisionResponse.json() as SavedRevision;
  expect(revision.revision).toMatchObject({
    entry_id: saved.composition_id,
    revision_id: saved.revision_id,
  });
  const document = parseYaml(String(revision.fields.spec)) as {
    spec?: {
      variables?: Array<{
        id?: string;
        query?: {
          kind?: string;
          entry_id?: string;
          revision_id?: string;
          form_bindings?: Array<{ relation: string; form_id: string }>;
        };
      }>;
      components?: Array<{ id?: string; kind?: string; variable?: string }>;
    };
  };
  const variables = document.spec?.variables ?? [];
  const components = document.spec?.components ?? [];
  const formQuery = variables.find((variable) =>
    variable.id === formVariableId
  );
  const savedSqlVariable = variables.find((variable) =>
    variable.query?.kind === "saved_sql"
  );
  expect(formQuery?.query?.form_bindings).toEqual([{
    relation: form!.name,
    form_id: form!.id,
  }]);
  expect(savedSqlVariable?.query).toMatchObject({
    kind: "saved_sql",
    entry_id: savedSql.id,
    revision_id: savedSql.revision_id,
  });
  expect(components.find(({ id }) => id === "component_1")?.variable).toBe(
    savedSqlVariable?.id,
  );
  expect(components.find(({ id }) => id === "component_2")?.variable).toBe(
    formVariableId,
  );

  // Reopening the exact persisted revision must retain both independent
  // references, including the Form identity and pinned Saved SQL revision.
  await page.reload({ waitUntil: "domcontentloaded" });
  await expect(page).toHaveURL(
    new RegExp(
      `/spaces/${spaceId}/compositions/${saved.composition_id}/${saved.revision_id}$`,
    ),
  );
  await expect(
    page.locator(
      '.compositionGraphValue[data-component-id="component_1"]',
    ).getByRole("table"),
  ).toContainText("First shared row", {
    timeout: 30_000,
  });
  await expect(
    page.locator(
      '.compositionGraphValue[data-component-id="component_2"]',
    ).getByRole("table"),
  ).toContainText("Second shared row", {
    timeout: 30_000,
  });

  const formsAfter = await (await request.get(
    getBackendUrl(`/spaces/${spaceId}/forms`),
  )).json() as FormSummary[];
  const idsBefore = new Set(formIdsBefore);
  const addedForms = formsAfter.filter(({ id }) => !idsBefore.has(id));
  // `_ugoite_compositions` is the reserved, append-only registry Form
  // provisioned by the Composition service (crates/ugoite-iceberg/src/composition.rs);
  // it is not another user-authored Form or transient Work record.
  expect(
    addedForms.map(({ name }) => name),
    `Only first-use _ugoite_compositions registry provisioning is expected: before=${
      JSON.stringify(formsBefore.map(({ id, name }) => ({ id, name })))
    } after=${
      JSON.stringify(formsAfter.map(({ id, name }) => ({ id, name })))
    }`,
  ).toEqual(["_ugoite_compositions"]);
  expect(formsAfter.filter(({ name }) => name === "_ugoite_compositions"))
    .toHaveLength(1);
  expect(formsAfter.filter(({ name }) => name === "SQL")).toHaveLength(1);
  expect(new Set(formsAfter.map(({ id }) => id)).size).toBe(formsAfter.length);
  expect(
    formsAfter.map(({ id }) => id).filter((id) => idsBefore.has(id)).sort(),
  )
    .toEqual(formIdsBefore);
  const userFormIdsAfter = formsAfter.filter(({ name }) =>
    name !== "SQL" && name !== "User" && name !== "UserGroup" &&
    name !== "_ugoite_compositions"
  ).map(({ id }) => id).sort();
  expect(userFormIdsAfter).toEqual(userFormIdsBefore);
  const sqlListAfter = await request.get(
    getBackendUrl(`/spaces/${spaceId}/sql`),
  );
  expect(sqlListAfter.ok()).toBe(true);
  const savedSqlSourcesAfter = (await sqlListAfter.json() as SavedSql[])
    .map(({ id, revision_id }) => ({ id, revision_id }))
    .sort((left, right) => String(left.id).localeCompare(String(right.id)));
  expect(savedSqlSourcesAfter).toEqual(savedSqlSourcesBefore);
});
