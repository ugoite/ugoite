import { assertEquals } from "@std/assert/equals";

const repository = new URL("../", import.meta.url);
const [registrySource, toolbarSource, sqlSource, protocolSource] = await Promise
  .all([
    Deno.readTextFile(
      new URL("crates/ugoite-domain/src/sql_functions.rs", repository),
    ),
    Deno.readTextFile(
      new URL("frontend/src/components/SqlDocumentTreeEditor.tsx", repository),
    ),
    Deno.readTextFile(new URL("frontend/src/lib/sql.ts", repository)),
    Deno.readTextFile(
      new URL("frontend/src/lib/ugoite-client/protocol.ts", repository),
    ),
  ]);

function registryFunctions(): Array<{
  name: string;
  aliases: string[];
  input: "AnyScalar" | "Numeric" | "Orderable";
  arity: { min: number; max: number | null };
  distinct: "Unsupported" | "SingleArgument";
  wildcard: "Unsupported" | "WithOrWithoutDistinct";
}> {
  const registry = registrySource.match(
    /pub const USER_SQL_AGGREGATES: &\[SqlAggregateFunction\] = &\[([\s\S]*?)\];/,
  );
  if (!registry) throw new Error("USER_SQL_AGGREGATES registry was not found");

  return [...registry[1].matchAll(/\b([A-Z][A-Z0-9_]*)\b/g)].map((match) => {
    const identifier = match[1];
    const definition = registrySource.match(
      new RegExp(
        `const ${identifier}: SqlAggregateFunction = SqlAggregateFunction \\{([\\s\\S]*?)\\n\\};`,
      ),
    );
    if (!definition) {
      throw new Error(`Missing aggregate descriptor ${identifier}`);
    }
    const name = definition[1].match(/\bname:\s*"([^"]+)"/);
    const aliases = definition[1].match(/\baliases:\s*&\[([^\]]*)\]/);
    const input = definition[1].match(
      /\binput:\s*SqlAggregateInput::(AnyScalar|Numeric|Orderable)/,
    );
    const arity = definition[1].match(
      /\barity:\s*SqlFunctionArity::(exact|at_least)\((\d+)\)/,
    );
    const distinct = definition[1].match(
      /\bdistinct:\s*SqlAggregateDistinct::(Unsupported|SingleArgument)/,
    );
    const wildcard = definition[1].match(
      /\bwildcard:\s*SqlAggregateWildcard::(Unsupported|WithOrWithoutDistinct)/,
    );
    if (!name || !aliases || !input || !arity || !distinct || !wildcard) {
      throw new Error(`Incomplete aggregate descriptor ${identifier}`);
    }
    const minimum = Number(arity[2]);
    return {
      name: name[1],
      aliases: [...aliases[1].matchAll(/"([^"]+)"/g)].map((alias) => alias[1]),
      input: input[1] as "AnyScalar" | "Numeric" | "Orderable",
      arity: {
        min: minimum,
        max: arity[1] === "exact" ? minimum : null,
      },
      distinct: distinct[1] as "Unsupported" | "SingleArgument",
      wildcard: wildcard[1] as "Unsupported" | "WithOrWithoutDistinct",
    };
  });
}

function returnedAggregateLists(source: string): Map<string, string[]> {
  const body = source.match(
    /const aggregateFunctionsForType = \([\s\S]*?\n\};/,
  );
  if (!body) throw new Error("aggregateFunctionsForType was not found");
  const result = new Map<string, string[]>();
  let pendingTypes: string[] = [];

  for (const line of body[0].split("\n")) {
    const type = line.match(/^\s*case "([^"]+)":/);
    if (type) pendingTypes.push(type[1]);

    const returned = line.match(/return \[([^\]]*)\]/);
    if (!returned) continue;
    const functions = [...returned[1].matchAll(/"([^"]+)"/g)].map((entry) =>
      entry[1]
    );
    for (const pendingType of pendingTypes) result.set(pendingType, functions);
    pendingTypes = [];
  }
  return result;
}

Deno.test("browser aggregate picker follows the Rust aggregate registry", () => {
  const functions = registryFunctions();
  const registeredSpellings = functions.flatMap((
    { name, aliases },
  ) => [name, ...aliases]);
  const count = functions.find(({ name }) => name === "count");
  if (!count) throw new Error("COUNT aggregate descriptor was not found");
  assertEquals(count.arity, { min: 0, max: null });
  assertEquals(count.distinct, "SingleArgument");
  assertEquals(count.wildcard, "WithOrWithoutDistinct");
  for (const descriptor of functions.filter(({ name }) => name !== "count")) {
    assertEquals(descriptor.arity, { min: 1, max: 1 }, descriptor.name);
    assertEquals(descriptor.distinct, "SingleArgument", descriptor.name);
    assertEquals(descriptor.wildcard, "Unsupported", descriptor.name);
  }
  const toolbarList = toolbarSource.match(
    /const aggregateFunctions:[\s\S]*?= \[([\s\S]*?)\];/,
  );
  const protocolList = protocolSource.match(
    /export type SqlDocumentAggregateFunction =([\s\S]*?);/,
  );
  if (!toolbarList || !protocolList) {
    throw new Error("Typed aggregate editor or protocol list was not found");
  }
  const toolbarNames = [...toolbarList[1].matchAll(/kind: "([^"]+)"/g)].map((
    [, name],
  ) => name);
  const protocolNames = [...protocolList[1].matchAll(/"([^"]+)"/g)].map((
    [, name],
  ) => name);
  const canonical = functions.map(({ name }) => name);
  assertEquals(toolbarNames.slice().sort(), canonical.slice().sort());
  assertEquals(protocolNames.slice().sort(), canonical.slice().sort());
  assertEquals(
    registeredSpellings.includes("mean"),
    true,
    "MEAN remains an accepted AVG alias rather than a separate picker entry",
  );
  assertEquals(toolbarSource.includes('kind: "set_function_argument"'), true);
  assertEquals(toolbarSource.includes('kind: "set_aggregate_distinct"'), true);

  const byType = returnedAggregateLists(sqlSource);
  const numericTypes = new Set(["integer", "float", "decimal"]);
  const orderableTypes = new Set([
    "string",
    "boolean",
    "integer",
    "float",
    "decimal",
    "date",
    "timestamp",
  ]);
  for (const [type, actual] of byType) {
    const expected = functions.flatMap(({ name, aliases, input }) => {
      const allowed = input === "AnyScalar" ||
        (input === "Numeric" && numericTypes.has(type)) ||
        (input === "Orderable" && orderableTypes.has(type));
      return allowed ? [name, ...aliases] : [];
    });
    assertEquals(actual, expected, `aggregate picker capabilities for ${type}`);
  }

  assertEquals(
    byType.size,
    9,
    "all SQL structure types have an aggregate capability row",
  );
});
