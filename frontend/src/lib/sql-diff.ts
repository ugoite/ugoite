export type SqlLineDiffEntry = {
  kind:
    | "context"
    | "removed"
    | "added"
    | "omitted"
    | "omitted-changes"
    | "omitted-range"
    | "preview-limit"
    | "unchanged";
  line: string;
  omittedCount?: number;
  removedCount?: number;
  addedCount?: number;
};

const MAX_SQL_DIFF_CHARS = 256 * 1024;
const MAX_SQL_DIFF_LINES = 10_000;

function fitsSqlDiffBounds(sql: string): boolean {
  if (sql.length > MAX_SQL_DIFF_CHARS) return false;
  let lineCount = 1;
  for (let index = 0; index < sql.length; index += 1) {
    if (sql.charCodeAt(index) === 10) {
      lineCount += 1;
      if (lineCount > MAX_SQL_DIFF_LINES) return false;
    }
  }
  return true;
}

function compactSqlLineDiff(diff: SqlLineDiffEntry[]): SqlLineDiffEntry[] {
  const hasChanges = diff.some((line) =>
    line.kind === "removed" || line.kind === "added" ||
    line.kind === "omitted-changes" || line.kind === "omitted-range"
  );
  if (!hasChanges) return [{ kind: "unchanged", line: "" }];

  const boundedDiff: SqlLineDiffEntry[] = [];
  let removedCount = 0;
  let addedCount = 0;
  const flushChangedRun = () => {
    const count = removedCount + addedCount;
    if (count > 40) {
      boundedDiff.push({
        kind: "omitted-changes",
        line: "",
        removedCount,
        addedCount,
      });
    } else if (count > 0) {
      for (const entry of pendingChangedRun) boundedDiff.push(entry);
    }
    pendingChangedRun.length = 0;
    removedCount = 0;
    addedCount = 0;
  };
  const pendingChangedRun: SqlLineDiffEntry[] = [];
  for (const entry of diff) {
    if (entry.kind === "removed" || entry.kind === "added") {
      pendingChangedRun.push(entry);
      if (entry.kind === "removed") removedCount += 1;
      else addedCount += 1;
    } else {
      flushChangedRun();
      boundedDiff.push(entry);
    }
  }
  flushChangedRun();

  const changedLines: number[] = [];
  let totalRemoved = 0;
  let totalAdded = 0;
  let hasUncountedChanges = false;
  boundedDiff.forEach((line, index) => {
    if (line.kind === "removed") totalRemoved += 1;
    else if (line.kind === "added") totalAdded += 1;
    else if (line.kind === "omitted-changes") {
      totalRemoved += line.removedCount ?? 0;
      totalAdded += line.addedCount ?? 0;
    } else if (line.kind === "omitted-range") hasUncountedChanges = true;
    if (
      line.kind === "removed" || line.kind === "added" ||
      line.kind === "omitted-changes" || line.kind === "omitted-range"
    ) changedLines.push(index);
  });

  const visible = new Uint8Array(boundedDiff.length);
  for (const changedLine of changedLines) {
    for (
      let index = Math.max(0, changedLine - 2);
      index <= Math.min(boundedDiff.length - 1, changedLine + 2);
      index += 1
    ) visible[index] = 1;
  }

  const compact: SqlLineDiffEntry[] = [];
  let omittedCount = 0;
  const flushOmitted = () => {
    if (omittedCount === 0) return;
    compact.push({ kind: "omitted", line: "", omittedCount });
    omittedCount = 0;
  };
  for (let index = 0; index < boundedDiff.length; index += 1) {
    if (visible[index]) {
      flushOmitted();
      compact.push(boundedDiff[index]);
    } else {
      omittedCount += 1;
    }
  }
  flushOmitted();
  if (compact.length > 600) {
    if (hasUncountedChanges) {
      return [{ kind: "omitted-range", line: "" }];
    }
    return [{
      kind: "omitted-changes",
      line: "",
      removedCount: totalRemoved,
      addedCount: totalAdded,
    }];
  }
  return compact;
}

function myersSqlLineDiff(
  currentLines: string[],
  generatedLines: string[],
): SqlLineDiffEntry[] | null {
  const maxEditDistance = Math.min(
    256,
    currentLines.length + generatedLines.length,
  );
  const offset = maxEditDistance + 1;
  const frontier = new Int32Array(2 * maxEditDistance + 3);
  frontier.fill(-1);
  frontier[offset + 1] = 0;
  const trace: Int32Array[] = [];
  let workBudget = 1_000_000;

  for (let distance = 0; distance <= maxEditDistance; distance += 1) {
    trace.push(frontier.slice());
    for (let diagonal = -distance; diagonal <= distance; diagonal += 2) {
      workBudget -= 1;
      if (workBudget < 0) return null;
      let current = diagonal === -distance || (
          diagonal !== distance &&
          frontier[offset + diagonal - 1] < frontier[offset + diagonal + 1]
        )
        ? frontier[offset + diagonal + 1]
        : frontier[offset + diagonal - 1] + 1;
      let generated = current - diagonal;

      while (
        current < currentLines.length && generated < generatedLines.length &&
        currentLines[current] === generatedLines[generated]
      ) {
        current += 1;
        generated += 1;
        workBudget -= 1;
        if (workBudget < 0) return null;
      }

      frontier[offset + diagonal] = current;
      if (
        current >= currentLines.length && generated >= generatedLines.length
      ) {
        let backtrackCurrent = currentLines.length;
        let backtrackGenerated = generatedLines.length;
        const reversed: SqlLineDiffEntry[] = [];

        for (let step = distance; step >= 0; step -= 1) {
          const previousFrontier = trace[step];
          const previousDiagonal = backtrackCurrent - backtrackGenerated;
          const previousPathDiagonal = previousDiagonal === -step || (
              previousDiagonal !== step &&
              previousFrontier[offset + previousDiagonal - 1] <
                previousFrontier[offset + previousDiagonal + 1]
            )
            ? previousDiagonal + 1
            : previousDiagonal - 1;
          const previousCurrent = previousFrontier[
            offset + previousPathDiagonal
          ];
          const previousGenerated = previousCurrent - previousPathDiagonal;

          while (
            backtrackCurrent > previousCurrent &&
            backtrackGenerated > previousGenerated
          ) {
            reversed.push({
              kind: "context",
              line: currentLines[backtrackCurrent - 1],
            });
            backtrackCurrent -= 1;
            backtrackGenerated -= 1;
          }

          if (step === 0) break;
          if (backtrackCurrent === previousCurrent) {
            reversed.push({
              kind: "added",
              line: generatedLines[backtrackGenerated - 1],
            });
            backtrackGenerated -= 1;
          } else {
            reversed.push({
              kind: "removed",
              line: currentLines[backtrackCurrent - 1],
            });
            backtrackCurrent -= 1;
          }
        }
        return reversed.reverse();
      }
    }
  }
  return null;
}

function prefixSuffixSqlLineDiff(
  currentLines: string[],
  generatedLines: string[],
): SqlLineDiffEntry[] {
  let prefix = 0;
  while (
    prefix < currentLines.length && prefix < generatedLines.length &&
    currentLines[prefix] === generatedLines[prefix]
  ) prefix += 1;

  let suffix = 0;
  while (
    suffix < currentLines.length - prefix &&
    suffix < generatedLines.length - prefix &&
    currentLines[currentLines.length - 1 - suffix] ===
      generatedLines[generatedLines.length - 1 - suffix]
  ) suffix += 1;

  const removedCount = currentLines.length - prefix - suffix;
  const addedCount = generatedLines.length - prefix - suffix;
  return [
    ...currentLines.slice(0, prefix).map((line) => ({
      kind: "context" as const,
      line,
    })),
    ...(removedCount + addedCount > 40
      ? [{
        kind: "omitted-range" as const,
        line: "",
      }]
      : [
        ...currentLines.slice(prefix, currentLines.length - suffix).map((
          line,
        ) => ({
          kind: "removed" as const,
          line,
        })),
        ...generatedLines.slice(prefix, generatedLines.length - suffix).map((
          line,
        ) => ({
          kind: "added" as const,
          line,
        })),
      ]),
    ...currentLines.slice(currentLines.length - suffix).map((line) => ({
      kind: "context" as const,
      line,
    })),
  ];
}

export function buildSqlLineDiff(
  currentSql: string,
  generatedSql: string,
): SqlLineDiffEntry[] {
  if (
    !fitsSqlDiffBounds(currentSql) || !fitsSqlDiffBounds(generatedSql)
  ) return [{ kind: "preview-limit", line: "" }];

  const currentLines = currentSql.split(/\r?\n/);
  const generatedLines = generatedSql.split(/\r?\n/);
  const cellCount = (currentLines.length + 1) * (generatedLines.length + 1);

  if (cellCount > 250_000) {
    const myersDiff = myersSqlLineDiff(currentLines, generatedLines);
    return compactSqlLineDiff(
      myersDiff ?? prefixSuffixSqlLineDiff(currentLines, generatedLines),
    );
  }

  const lengths = Array.from(
    { length: currentLines.length + 1 },
    () => new Uint32Array(generatedLines.length + 1),
  );
  for (let current = currentLines.length - 1; current >= 0; current -= 1) {
    for (
      let generated = generatedLines.length - 1;
      generated >= 0;
      generated -= 1
    ) {
      lengths[current][generated] = currentLines[current] ===
          generatedLines[generated]
        ? lengths[current + 1][generated + 1] + 1
        : Math.max(
          lengths[current + 1][generated],
          lengths[current][generated + 1],
        );
    }
  }

  const diff: SqlLineDiffEntry[] = [];
  let current = 0;
  let generated = 0;
  while (current < currentLines.length || generated < generatedLines.length) {
    if (
      current < currentLines.length && generated < generatedLines.length &&
      currentLines[current] === generatedLines[generated]
    ) {
      diff.push({ kind: "context", line: currentLines[current] });
      current += 1;
      generated += 1;
    } else if (
      current < currentLines.length &&
      (generated === generatedLines.length ||
        lengths[current + 1][generated] >= lengths[current][generated + 1])
    ) {
      diff.push({ kind: "removed", line: currentLines[current] });
      current += 1;
    } else {
      diff.push({ kind: "added", line: generatedLines[generated] });
      generated += 1;
    }
  }
  return compactSqlLineDiff(diff);
}
