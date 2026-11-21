import { assertEquals } from "@std/assert/equals";

const repository = new URL("../", import.meta.url);
const [registrySource, completionSource] = await Promise
  .all([
    Deno.readTextFile(
      new URL("crates/ugoite-domain/src/sql_functions.rs", repository),
    ),
    Deno.readTextFile(
      new URL("frontend/src/lib/sql-completion.ts", repository),
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

Deno.test("SQL completion aggregate names follow the Rust registry", () => {
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
  const completionList = completionSource.match(
    /const SQL_AGGREGATE_FUNCTIONS = \[([\s\S]*?)\];/,
  );
  if (!completionList) {
    throw new Error("SQL completion source was not found");
  }
  const completionNames = [...completionList[1].matchAll(/"([A-Z]+)"/g)].map((
    [, name],
  ) => name.toLowerCase());
  const canonical = functions.map(({ name }) => name);
  assertEquals(completionNames.slice().sort(), canonical.slice().sort());
  assertEquals(
    registeredSpellings.includes("mean"),
    true,
    "MEAN remains an accepted AVG alias rather than a separate picker entry",
  );
});
