// This module is also loaded as plain JavaScript by the trusted GitHub Script adapter.
// @ts-nocheck: The trusted GitHub Script adapter evaluates this as plain JavaScript.

const reviewSectionPattern =
  /##\s*Knowledge Compatibility Review\s*\n+([\s\S]*?)(?:\n##\s|$)/i;

const classifications = [
  {
    kind: "no-effect",
    pattern:
      /^\s*-\s+\[[xX]\]\s+No effect on the v0\.1 Knowledge semantic contract\.\s*$/im,
  },
  {
    kind: "preserving",
    pattern:
      /^\s*-\s+\[[xX]\]\s+Preserving implementation change; the canonical fixture and focused tests remain passing\.\s*$/im,
  },
  {
    kind: "breaking",
    pattern:
      /^\s*-\s+\[[xX]\]\s+Breaking semantic change; an explicit versioned contract or migration\/re-encoding decision is documented\.\s*$/im,
  },
];

/** @type {Record<"Evidence" | "Decision", string>} */
const notePlaceholders = {
  Evidence: "<required for preserving changes>",
  Decision: "<required for breaking changes>",
};

/**
 * @param {string} reviewText
 * @param {"Evidence" | "Decision"} label
 */
function hasReviewNote(reviewText, label) {
  const noteMatch = reviewText.match(
    new RegExp(`^\\s*${label}:\\s*(.*)$`, "im"),
  );
  const note = noteMatch?.[1]?.trim() ?? "";
  return note !== "" && note !== notePlaceholders[label];
}

/**
 * @param {string} body
 * @returns {string[]}
 */
export function validateKnowledgeCompatibilityReview(body) {
  const compatibilityMatch = body.match(reviewSectionPattern);
  if (!compatibilityMatch) {
    return ["Knowledge Compatibility Review section is missing."];
  }

  const reviewText = compatibilityMatch[1].trim();
  const checkedCheckboxes = reviewText.match(/^\s*-\s+\[[xX]\]\s+.+$/gm) ??
    [];
  const checkedClassifications = classifications.filter(({ pattern }) =>
    pattern.test(reviewText)
  );
  const errors = [];

  if (checkedCheckboxes.length !== 1 || checkedClassifications.length !== 1) {
    errors.push(
      "Knowledge Compatibility Review must select exactly one valid classification.",
    );
  }

  const classification = checkedClassifications[0]?.kind;
  if (
    classification === "preserving" && !hasReviewNote(reviewText, "Evidence")
  ) {
    errors.push("Preserving changes must include non-empty Evidence: notes.");
  }
  if (classification === "breaking" && !hasReviewNote(reviewText, "Decision")) {
    errors.push("Breaking changes must include non-empty Decision: notes.");
  }

  return errors;
}
