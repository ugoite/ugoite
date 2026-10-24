import "@testing-library/jest-dom/vitest";
import { createSignal } from "solid-js";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import * as client from "~/lib/ugoite-client";
import { SqlFormPicker } from "./SqlFormPicker";

const { applyMock } = vi.hoisted(() => ({ applyMock: vi.fn() }));
vi.mock("~/lib/ugoite-client", async (original) => {
  const actual = await original<typeof client>();
  applyMock.mockImplementation(actual.applySqlDocumentSemanticEditTransaction);
  return { ...actual, applySqlDocumentSemanticEditTransaction: applyMock };
});

const forms = [
  { relation: "form_123", label: "Expenses" },
  { relation: "form_456", label: "Invoices" },
];
function setup(source: string) {
  const [sql, setSql] = createSignal(source);
  const [scope, setScope] = createSignal("query-a");
  const [choices, setChoices] = createSignal(forms);
  const advanced = vi.fn();
  const view = render(() => (
    <SqlFormPicker
      sql={sql}
      scope={scope}
      forms={choices}
      onSqlChange={setSql}
      onAdvanced={advanced}
    />
  ));
  return { sql, setSql, scope, setScope, setChoices, advanced, ...view };
}

describe("Form selection through the actual Rust SQL Document", () => {
  beforeEach(async () => {
    const actual = await vi.importActual<typeof client>("~/lib/ugoite-client");
    applyMock.mockReset().mockImplementation(
      actual.applySqlDocumentSemanticEditTransaction,
    );
  });

  it("starts an empty query by choosing a human-labelled Form without a syntax tree", async () => {
    const { sql } = setup("-- 日本語");
    const button = await screen.findByRole("button", { name: "Expenses" });
    await waitFor(() => expect(button).toBeEnabled());
    expect(screen.queryByRole("tree")).not.toBeInTheDocument();
    expect(screen.queryByRole("combobox")).not.toBeInTheDocument();
    fireEvent.click(button);
    await waitFor(() =>
      expect(sql()).toBe('-- 日本語\nSELECT * FROM "form_123"')
    );
    expect((await client.parseSqlDocument(sql())).status).toBe("parsed");
  });

  it("replaces a source while preserving aliases, comments, columns, predicates and ordering", async () => {
    const { sql } = setup(
      'SELECT e.amount /* keep */ FROM "form_123" AS e WHERE e.amount > 5 ORDER BY e.amount',
    );
    const button = await screen.findByRole("button", { name: "Invoices" });
    await waitFor(() => expect(button).toBeEnabled());
    fireEvent.click(button);
    await waitFor(() =>
      expect(sql()).toBe(
        'SELECT e.amount /* keep */ FROM "form_456" AS e WHERE e.amount > 5 ORDER BY e.amount',
      )
    );
  });

  it("adds a source to a relation-free SELECT without replacing its expression", async () => {
    const { sql } = setup("SELECT 42 AS answer");
    const button = await screen.findByRole("button", { name: "Expenses" });
    await waitFor(() => expect(button).toBeEnabled());
    fireEvent.click(button);
    await waitFor(() =>
      expect(sql()).toBe('SELECT 42 AS answer FROM "form_123"')
    );
  });

  it("requires an explicit source choice only when there are multiple sources", async () => {
    const { sql } = setup(
      'SELECT e.amount FROM "form_123" AS e JOIN "form_456" AS i ON e.id = i.id',
    );
    const chooser = await screen.findByRole("combobox", {
      name: "Source to change",
    });
    expect(screen.getByRole("option", { name: "1. e · Expenses" }))
      .toBeInTheDocument();
    expect(screen.getByRole("option", { name: "2. i · Invoices" }))
      .toBeInTheDocument();
    fireEvent.change(chooser, {
      target: { value: chooser.querySelectorAll("option")[1].value },
    });
    fireEvent.click(screen.getByRole("button", { name: "Expenses" }));
    await waitFor(() =>
      expect(sql()).toBe(
        'SELECT e.amount FROM "form_123" AS e JOIN "form_123" AS i ON e.id = i.id',
      )
    );
  });

  it("keeps an invalid draft and opens explicit advanced editing", async () => {
    const { sql, advanced } = setup("SELECT (");
    const button = await screen.findByRole("button", { name: "Expenses" });
    await screen.findByText("Complete the SQL before choosing a Form.");
    expect(button).toBeDisabled();
    expect(sql()).toBe("SELECT (");
    fireEvent.click(
      screen.getByRole("button", { name: "Advanced structure…" }),
    );
    expect(advanced).toHaveBeenCalledOnce();
    expect(applyMock).not.toHaveBeenCalled();
  });

  it("keeps the focused Form row through refreshed metadata and a failed operation", async () => {
    let fail!: (error: Error) => void;
    applyMock.mockImplementationOnce(() =>
      new Promise((_, reject) => {
        fail = reject;
      })
    );
    const { setChoices, sql } = setup("SELECT 1");
    const form = await screen.findByRole("button", { name: "Expenses" });
    await waitFor(() => expect(form).toBeEnabled());
    form.focus();
    fireEvent.click(form);
    await waitFor(() => expect(applyMock).toHaveBeenCalledOnce());
    setChoices(forms.map((item) => ({ ...item })));
    expect(screen.getByRole("button", { name: "Expenses" })).toBe(form);
    expect(form).toHaveFocus();
    fail(new Error("unavailable"));
    await screen.findByRole("alert");
    expect(form).toHaveFocus();
    expect(sql()).toBe("SELECT 1");
  });

  it.each(["source", "scope", "unmount"])(
    "discards an in-flight operation after %s changes",
    async (change) => {
      let resolve: (
        result: Awaited<
          ReturnType<typeof client.applySqlDocumentSemanticEditTransaction>
        >,
      ) => void;
      applyMock.mockImplementationOnce(() =>
        new Promise((done) => {
          resolve = done;
        })
      );
      const initial = "SELECT 1";
      const view = setup(initial);
      const button = await screen.findByRole("button", { name: "Expenses" });
      await waitFor(() => expect(button).toBeEnabled());
      fireEvent.click(button);
      await waitFor(() => expect(applyMock).toHaveBeenCalledOnce());
      if (change === "source") view.setSql("SELECT 2");
      if (change === "scope") view.setScope("query-b");
      if (change === "unmount") view.unmount();
      resolve!({
        document: await client.parseSqlDocument('SELECT 1 FROM "form_123"'),
        undo: { expected_source: "", edits: [] },
      });
      await Promise.resolve();
      expect(view.sql()).toBe(change === "source" ? "SELECT 2" : initial);
    },
  );
});
