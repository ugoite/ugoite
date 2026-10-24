import { expect, test } from "@playwright/test";
import {
  getBackendUrl,
  getDefaultSpaceId,
  getFrontendUrl,
  waitForServers,
} from "./lib/client.ts";

import {
  completeCompositionGraphYaml,
  saveCompositionGraph,
} from "./lib/composition-graph.ts";

test(
  "SQL Columns edits human-labelled fields and preserves focus at narrow widths",
  async ({ page, request }, testInfo) => {
    test.setTimeout(120_000);
    await waitForServers(request);
    const spaceId = await getDefaultSpaceId(request);
    const formName = `Columns${crypto.randomUUID().slice(0, 8)}`;
    const created = await request.post(
      getBackendUrl(`/spaces/${spaceId}/forms`),
      {
        data: {
          name: formName,
          version: 1,
          template: `# ${formName}\n\n## amount\n\n## day\n`,
          fields: {
            amount: { type: "integer", required: true, label: "Amount" },
            day: { type: "date", required: true, label: "Day" },
          },
        },
      },
    );
    expect([200, 201]).toContain(created.status());
    const entry = await request.post(
      getBackendUrl(`/spaces/${spaceId}/entries`),
      { data: { form: formName, fields: { amount: 11, day: "2026-10-24" } } },
    );
    expect(entry.status()).toBe(201);
    await page.goto(getFrontendUrl(`/spaces/${spaceId}/sql/new`), {
      waitUntil: "domcontentloaded",
    });
    const sqlEditor = page.getByRole("textbox", {
      name: "SQL",
      exact: true,
    });
    await expect(sqlEditor).toHaveAttribute("contenteditable", "true");
    await expect(sqlEditor).toHaveText("SELECT 1");
    await sqlEditor.click();
    await sqlEditor.press("ControlOrMeta+A");
    await sqlEditor.pressSequentially("SELECT *");
    await expect(sqlEditor).toHaveText("SELECT *");
    await page.getByRole("button", { name: "Form", exact: true }).click();
    await page.getByRole("dialog", { name: "Form", exact: true }).getByRole(
      "button",
      { name: formName, exact: true },
    ).click();
    const sourceSql = await page.getByRole("textbox", {
      name: "SQL",
      exact: true,
    }).innerText();
    const originalPlan = await request.post(
      getBackendUrl(`/spaces/${spaceId}/sql/query/plan`),
      {
        data: { sql: sourceSql, parameters: {}, parameter_types: {} },
      },
    );
    expect(originalPlan.ok()).toBe(true);
    const expectedSchema =
      (await originalPlan.json() as { result_schema: Array<{ name: string }> })
        .result_schema;
    const opener = page.getByRole("button", { name: "Columns", exact: true });
    for (const width of [320, 375, 390]) {
      await page.setViewportSize({ width, height: 844 });
      await opener.click();
      const dialog = page.getByRole("dialog", { name: "Columns", exact: true });
      await expect(dialog.getByRole("tree")).toHaveCount(0);
      await expect(
        dialog.getByRole("button", { name: "Choose fields", exact: true }),
      ).toBeVisible();
      const targets = await dialog.getByRole("button").evaluateAll((buttons) =>
        buttons.filter((button) => button.getClientRects().length).map(
          (button) => {
            const box = button.getBoundingClientRect();
            return { width: box.width, height: box.height };
          },
        )
      );
      expect(targets.every((box) => box.width >= 44 && box.height >= 44)).toBe(
        true,
      );
      expect(
        await page.evaluate(() =>
          document.documentElement.scrollWidth <= innerWidth
        ),
      ).toBe(true);
      if (width !== 390) {
        await page.keyboard.press("Escape");
        await expect(opener).toBeFocused();
        continue;
      }
      await dialog.getByRole("button", { name: "Choose fields", exact: true })
        .click();
      const fields = dialog.getByRole("list", { name: "Result fields" });
      await expect(fields.getByRole("listitem")).toHaveCount(
        expectedSchema.length,
      );
      const expandedPlan = await request.post(
        getBackendUrl(`/spaces/${spaceId}/sql/query/plan`),
        {
          data: {
            sql: await page.getByRole("textbox", { name: "SQL", exact: true })
              .innerText(),
            parameters: {},
            parameter_types: {},
          },
        },
      );
      expect(expandedPlan.ok()).toBe(true);
      expect((await expandedPlan.json()).result_schema).toEqual(expectedSchema);
      const labels = await fields.locator(".sql-columns-editor__name")
        .allTextContents();
      for (
        const name of labels.filter((name) =>
          name !== "Amount" && name !== "Day"
        )
      ) {
        const remove = dialog.getByRole("button", {
          name: `Remove ${name}`,
          exact: true,
        });
        await expect(remove).not.toHaveAttribute("aria-disabled", "true");
        await remove.click();
        await expect(remove).toHaveCount(0);
      }
      await expect(fields.getByRole("listitem")).toHaveText([/Amount/, /Day/]);
      const move = dialog.getByRole("button", {
        name: "Move Amount down",
        exact: true,
      });
      await expect(move).not.toHaveAttribute("aria-disabled", "true");
      await move.click();
      await expect(fields.getByRole("listitem")).toHaveText([/Day/, /Amount/]);
      await expect(move).toBeFocused();
      await page.screenshot({
        path: testInfo.outputPath("sql-columns-390.png"),
        fullPage: true,
      });
      await page.keyboard.press("Escape");
      await expect(opener).toBeFocused();
    }
    await page.getByRole("button", { name: "Filter", exact: true }).click();
    const filterDialog = page.getByRole("dialog", {
      name: "Filter",
      exact: true,
    });
    await filterDialog.getByRole("combobox", { name: "Column" }).selectOption({
      label: "Amount",
    });
    await filterDialog.getByRole("combobox", { name: "Operator" }).selectOption(
      "gte",
    );
    await filterDialog.getByRole("combobox", { name: "Value type" })
      .selectOption(
        "number",
      );
    await filterDialog.getByRole("textbox", { name: "Value" }).fill("10");
    await filterDialog.getByRole("button", { name: "Add filter" }).click();
    await expect(filterDialog).toBeHidden();
    await page.getByRole("button", { name: "Sort", exact: true }).click();
    const sortDialog = page.getByRole("dialog", { name: "Sort", exact: true });
    await sortDialog.getByRole("combobox", { name: "Column" }).selectOption({
      label: "Amount",
    });
    await sortDialog.getByRole("combobox", { name: "Direction" }).selectOption(
      "desc",
    );
    await sortDialog.getByRole("button", { name: "Add sort" }).click();
    await expect(sortDialog).toBeHidden();
    const taskEditedSql = await page.getByRole("textbox", {
      name: "SQL",
      exact: true,
    }).innerText();
    expect(taskEditedSql).toContain("amount >= 10");
    expect(taskEditedSql).toContain("ORDER BY amount DESC");
    await page.getByRole("button", { name: "Run Query", exact: true }).click();
    await expect(page.getByRole("table")).toContainText("11", {
      timeout: 30_000,
    });
    await expect(page.getByRole("table")).toContainText("2026-10-24");
    await page.getByLabel("Query name", { exact: true }).fill(
      `Result fields ${formName}`,
    );
    const savedResponse = page.waitForResponse((response) =>
      response.request().method() === "POST" &&
      new URL(response.url()).pathname === `/api/spaces/${spaceId}/sql`
    );
    await page.getByRole("button", { name: "Save", exact: true }).click();
    const response = await savedResponse;
    expect([200, 201]).toContain(response.status());
    const saved = await response.json() as { id: string };
    await page.waitForURL((url) => url.pathname === `/spaces/${spaceId}/sql`);
    await page.goto(getFrontendUrl(`/spaces/${spaceId}/sql/${saved.id}`));
    await page.getByRole("button", { name: "Columns", exact: true }).click();
    await expect(
      page.getByRole("dialog", { name: "Columns", exact: true }).getByRole(
        "listitem",
      ),
    ).toHaveText([/Day/, /Amount/]);
    await page.keyboard.press("Escape");
    const definition = await (await request.get(
      getBackendUrl(`/spaces/${spaceId}/sql/${saved.id}`),
    )).json() as { sql: string };
    expect(definition.sql).toContain("amount >= 10");
    expect(definition.sql).toContain("ORDER BY amount DESC");
    const yaml = await completeCompositionGraphYaml(request, spaceId, {
      name: `Columns Composition ${formName}`,
      variables: [{
        id: "results",
        kind: "query",
        returns: { kind: "table", columns: [] },
        query: {
          kind: "inline_sql",
          sql: definition.sql,
          arguments: [],
          form_bindings: [],
          schema_fingerprint: "",
        },
      }],
      components: [{ id: "table", kind: "table", variable: "results" }],
      rows: [{ id: "row", items: [{ kind: "component", component: "table" }] }],
    });
    const composition = await saveCompositionGraph(request, spaceId, yaml);
    await page.goto(
      getFrontendUrl(
        `/spaces/${spaceId}/compositions/${composition.composition_id}/${composition.revision_id}/edit`,
      ),
    );
    await page.getByRole("tab", { name: "Variables", exact: true }).click();
    await page.getByRole("button", { name: "Show variables", exact: true })
      .click();
    const resultsVariable = page.getByRole("button", {
      name: "results",
      exact: true,
    });
    await expect(resultsVariable).toBeVisible();
    await resultsVariable.click();
    await page.getByRole("button", { name: "Columns", exact: true }).click();
    const dialog = page.getByRole("dialog", { name: "Columns", exact: true });
    await expect(dialog.getByRole("listitem")).toHaveText([/Day/, /Amount/]);
    await dialog.getByRole("button", { name: "Move Day down", exact: true })
      .click();
    await expect(dialog.getByRole("listitem")).toHaveText([/Amount/, /Day/]);
    await page.keyboard.press("Escape");
    await page.getByRole("button", { name: "Run Query", exact: true }).click();
    await expect(page.getByRole("table")).toContainText("11", {
      timeout: 30_000,
    });
  },
);
