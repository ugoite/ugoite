const I64_MIN = -9223372036854775808n;
const I64_MAX = 9223372036854775807n;

const rawJson = (JSON as typeof JSON & {
  rawJSON?: (text: string) => unknown;
}).rawJSON;

const integerVariableKeys = Symbol("ugoite.composition.integer-variable-keys");

type IntegerVariableRecord = Record<string, unknown> & {
  [integerVariableKeys]?: ReadonlySet<string>;
};

/** Mark variable values whose type is known to be the Composition i64 type. */
export const markExactIntegerVariableValues = <
  T extends Record<string, unknown>,
>(
  values: T,
  variableIds: readonly string[],
): T => {
  if (variableIds.length === 0) return values;
  const copy = { ...values } as T & IntegerVariableRecord;
  Object.defineProperty(copy, integerVariableKeys, {
    value: new Set(variableIds),
    enumerable: false,
  });
  return copy;
};

export const encodeMarkedIntegerVariableValues = (
  values: Record<string, unknown>,
): Record<string, unknown> => {
  const keys = (values as IntegerVariableRecord)[integerVariableKeys];
  if (!keys?.size) return values;
  return Object.fromEntries(
    Object.entries(values).map(([key, value]) =>
      keys.has(key)
        ? [key, exactIntegerJsonValue(value as string | number)]
        : [key, value]
    ),
  );
};

const record = (value: unknown): value is Record<string, unknown> =>
  Boolean(value) && typeof value === "object" && !Array.isArray(value);

const encodeLiteral = (value: unknown): unknown => {
  if (!record(value) || value.kind !== "integer") return value;
  return {
    ...value,
    value: exactIntegerJsonValue(value.value as string | number),
  };
};

/** Encode only values at strict Composition/Variable Graph integer fields. */
export const encodeCompositionDocumentIntegers = (
  document: unknown,
): unknown => {
  if (!record(document) || !record(document.spec)) return document;
  const spec = document.spec;
  if (!Array.isArray(spec.variables)) return document;
  return {
    ...document,
    spec: {
      ...spec,
      variables: spec.variables.map((node) => {
        if (!record(node)) return node;
        if (node.kind === "constant") {
          return { ...node, value: encodeLiteral(node.value) };
        }
        if (node.kind === "input" && node.default !== undefined) {
          return { ...node, default: encodeLiteral(node.default) };
        }
        return node;
      }),
    },
  };
};

export const stringifyCompositionDocument = (document: unknown): string =>
  JSON.stringify(encodeCompositionDocumentIntegers(document));

export const encodeVariableGraphIntegers = (graph: unknown): unknown => {
  if (!record(graph) || !Array.isArray(graph.variables)) return graph;
  return {
    ...graph,
    variables: graph.variables.map((node) => {
      if (!record(node)) return node;
      if (node.kind === "constant") {
        return { ...node, value: encodeLiteral(node.value) };
      }
      if (node.kind === "input" && node.default !== undefined) {
        return { ...node, default: encodeLiteral(node.default) };
      }
      return node;
    }),
  };
};

/** Return an exact JSON integer token for a typed Rust i64 field. */
export const exactIntegerJsonValue = (value: string | number): unknown => {
  let integer: bigint;
  if (typeof value === "number") {
    if (!Number.isSafeInteger(value)) {
      throw new RangeError("Unsafe integer number cannot be encoded exactly");
    }
    integer = BigInt(value);
  } else {
    if (value.length > 20 || !/^-?(?:0|[1-9]\d*)$/.test(value)) {
      throw new RangeError("Expected a canonical integer value");
    }
    integer = BigInt(value);
  }
  if (integer < I64_MIN || integer > I64_MAX) {
    throw new RangeError("Integer is outside the supported i64 range");
  }
  if (Number.isSafeInteger(Number(integer))) return Number(integer);
  if (!rawJson) {
    throw new Error("This browser cannot preserve large integer values");
  }
  return rawJson(integer.toString());
};

export type ExactJsonParseResult<T> = {
  value: T;
  integerSource(parent: object, key: string): string | undefined;
};

/** Parse JSON while retaining source tokens for later typed i64 decoding. */
export const parseJsonWithNumberSources = <T>(
  text: string,
): ExactJsonParseResult<T> => {
  const sources = new WeakMap<object, Map<string, string>>();
  const value = JSON.parse(text, function (
    this: Record<string, unknown>,
    key: string,
    parsed: unknown,
    context?: { source?: string },
  ) {
    if (
      typeof parsed === "number" && typeof context?.source === "string" &&
      this && typeof this === "object"
    ) {
      const parentSources = sources.get(this) ?? new Map<string, string>();
      parentSources.set(key, context.source);
      sources.set(this, parentSources);
    }
    return parsed;
  }) as T;
  return {
    value,
    integerSource: (parent, key) => sources.get(parent)?.get(key),
  };
};

export const exactIntegerSourceValue = (
  parent: Record<string, unknown>,
  key: string,
  integerSource: (parent: object, key: string) => string | undefined,
): string => {
  const source = integerSource(parent, key);
  if (source !== undefined) {
    // Reuse the exact-token validator without producing a JSON value.
    if (source.length > 20 || !/^-?(?:0|[1-9]\d*)$/.test(source)) {
      throw new RangeError("Protocol integer source is malformed");
    }
    const integer = BigInt(source);
    if (integer < I64_MIN || integer > I64_MAX) {
      throw new RangeError(
        "Protocol integer is outside the supported i64 range",
      );
    }
    return integer.toString();
  }
  const value = parent[key];
  if (typeof value !== "number" || !Number.isSafeInteger(value)) {
    throw new Error("This browser cannot read this large integer exactly");
  }
  return String(value);
};

type IntegerSourceLookup = (parent: object, key: string) => string | undefined;

const decodeLiteral = (value: unknown, source: IntegerSourceLookup): void => {
  if (
    !record(value) || value.kind !== "integer" ||
    typeof value.value !== "number"
  ) {
    return;
  }
  value.value = exactIntegerSourceValue(value, "value", source);
};

/** Decode typed integer literals from a strict Composition document only. */
export const decodeCompositionDocumentIntegers = (
  document: unknown,
  source: IntegerSourceLookup,
): void => {
  if (
    !record(document) || !record(document.spec) ||
    !Array.isArray(document.spec.variables)
  ) return;
  for (const node of document.spec.variables) {
    if (!record(node)) continue;
    if (node.kind === "constant") decodeLiteral(node.value, source);
    else if (node.kind === "input") decodeLiteral(node.default, source);
  }
};

/** Decode only typed Composition result literals; opaque JSON values stay opaque. */
export const decodeCompositionEvaluationIntegers = (
  evaluation: unknown,
  source: IntegerSourceLookup,
): void => {
  if (!record(evaluation)) return;
  if (record(evaluation.variable_values)) {
    for (const value of Object.values(evaluation.variable_values)) {
      decodeLiteral(value, source);
    }
  }
  if (record(evaluation.query_results)) {
    for (const result of Object.values(evaluation.query_results)) {
      if (!record(result) || !record(result.value)) continue;
      const evaluated = result.value;
      if (evaluated.kind === "scalar") decodeLiteral(evaluated.value, source);
      else if (evaluated.kind === "table" && Array.isArray(evaluated.rows)) {
        for (const row of evaluated.rows) {
          if (Array.isArray(row)) {
            for (const literal of row) decodeLiteral(literal, source);
          }
        }
      }
    }
  }
};

export const decodeCompositionPreviewIntegerDefaults = (
  preview: unknown,
  source: IntegerSourceLookup,
): void => {
  if (!record(preview) || !Array.isArray(preview.variable_definitions)) return;
  for (const variable of preview.variable_definitions) {
    if (
      record(variable) && record(variable.value_type) &&
      variable.value_type.scalar_type === "integer" &&
      record(variable.default)
    ) decodeLiteral(variable.default, source);
  }
};
