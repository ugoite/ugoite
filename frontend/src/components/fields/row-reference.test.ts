import { describe, expect, it } from "vitest";
import {
  buildRowReferencePreview,
  displayableRowReferenceValue,
  rowReferencePreviewCharLimit,
  rowReferenceTargetMatches,
} from "~/components/fields/row-reference";

const projectForm = {
  id: "form-project",
  name: "Project",
  fields: {
    Title: { type: "string" },
    Budget: { type: "number" },
    Active: { type: "boolean" },
    Tags: { type: "list" },
    Meta: { type: "object_list" },
  },
};

describe("buildRowReferencePreview", () => {
  it("renders target-Form fields in order joined with middots", () => {
    expect(
      buildRowReferencePreview(
        { Title: "Alpha", Budget: 42, Active: true },
        projectForm,
      ),
    ).toBe("Title: Alpha · Budget: 42 · Active: true");
  });

  it("skips null, missing, and empty values", () => {
    expect(
      buildRowReferencePreview(
        { Title: "", Budget: null, Unknown: "x" },
        projectForm,
      ),
    ).toBe("");
  });

  it("renders collections as JSON without leaking structure errors", () => {
    expect(
      buildRowReferencePreview(
        { Tags: ["a", "b"], Meta: { owner: "ada" } },
        projectForm,
      ),
    ).toBe('Tags: ["a","b"] · Meta: {"owner":"ada"}');
  });

  it("truncates to the backend preview char budget", () => {
    const preview = buildRowReferencePreview(
      { Title: "x".repeat(rowReferencePreviewCharLimit + 100) },
      projectForm,
    );
    expect(Array.from(preview).length).toBe(rowReferencePreviewCharLimit);
  });

  it("returns an empty preview for non-object frontmatter", () => {
    expect(buildRowReferencePreview(null, projectForm)).toBe("");
    expect(buildRowReferencePreview("Title: Alpha", projectForm)).toBe("");
    expect(buildRowReferencePreview(undefined, projectForm)).toBe("");
  });
});

describe("displayableRowReferenceValue", () => {
  it("keeps strings verbatim and stringifies scalars", () => {
    expect(displayableRowReferenceValue("Alpha")).toBe("Alpha");
    expect(displayableRowReferenceValue(42)).toBe("42");
    expect(displayableRowReferenceValue(false)).toBe("false");
  });

  it("renders nothing for nullish values", () => {
    expect(displayableRowReferenceValue(null)).toBe("");
    expect(displayableRowReferenceValue(undefined)).toBe("");
  });
});

describe("rowReferenceTargetMatches", () => {
  it("accepts the target name or the stable target id", () => {
    expect(rowReferenceTargetMatches("Project", projectForm)).toBe(true);
    expect(rowReferenceTargetMatches("form-project", projectForm)).toBe(true);
  });

  it("rejects other forms and blank values", () => {
    expect(rowReferenceTargetMatches("Other", projectForm)).toBe(false);
    expect(rowReferenceTargetMatches("", projectForm)).toBe(false);
    expect(rowReferenceTargetMatches(undefined, projectForm)).toBe(false);
    expect(rowReferenceTargetMatches(null, projectForm)).toBe(false);
  });
});
