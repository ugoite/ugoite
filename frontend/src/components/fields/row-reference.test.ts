import { describe, expect, it } from "vitest";
import {
  buildRowReferencePreview,
  displayableRowReferenceValue,
  humanRowReferenceFormName,
  rowReferencePreviewCharLimit,
  rowReferenceTargetMatches,
} from "~/components/fields/row-reference";

const projectForm = {
  id: "form-project",
  name: "Project",
  fields: {
    Title: { type: "string" },
    Budget: { type: "double" },
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

  it("prefers the field label over the field name like the backend preview", () => {
    const labeledForm = {
      id: "form-project",
      name: "Project",
      fields: {
        Title: { type: "string", label: "Project title" },
        Budget: { type: "double" },
      },
    };
    expect(
      buildRowReferencePreview(
        { Title: "Alpha", Budget: 42 },
        labeledForm,
      ),
    ).toBe("Project title: Alpha · Budget: 42");
  });

  it("prefers the field label over the field name like the backend preview", () => {
    const labeledForm = {
      id: "form-project",
      name: "Project",
      fields: {
        Title: { type: "string", label: "Project title" },
        Budget: { type: "double" },
      },
    };
    expect(
      buildRowReferencePreview(
        { Title: "Alpha", Budget: 42 },
        labeledForm,
      ),
    ).toBe("Project title: Alpha · Budget: 42");
  });

  it("falls back to the field name for blank labels", () => {
    const blankLabelForm = {
      id: "form-project",
      name: "Project",
      fields: {
        Title: { type: "string", label: "  " },
      },
    };
    expect(
      buildRowReferencePreview({ Title: "Alpha" }, blankLabelForm),
    ).toBe("Title: Alpha");
  });

  it("truncates to the backend preview char budget", () => {
    const preview = buildRowReferencePreview(
      { Title: "x".repeat(rowReferencePreviewCharLimit + 100) },
      projectForm,
    );
    expect(Array.from(preview).length).toBe(rowReferencePreviewCharLimit);
  });

  it("returns an empty preview for non-object fields", () => {
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

describe("humanRowReferenceFormName", () => {
  const forms = [
    { id: "form-project", name: "Project" },
    { id: "form-task", name: "Task" },
  ];

  it("resolves stable ids to human Form names", () => {
    expect(humanRowReferenceFormName("form-task", forms)).toBe("Task");
    expect(humanRowReferenceFormName("Task", forms)).toBe("Task");
  });

  it("leaves unknown references verbatim only when the catalog cannot resolve them", () => {
    expect(humanRowReferenceFormName("form-missing", forms)).toBe(
      "form-missing",
    );
    expect(humanRowReferenceFormName("form-missing")).toBe("form-missing");
  });
});
