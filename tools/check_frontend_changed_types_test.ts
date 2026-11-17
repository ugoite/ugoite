import { assertEquals } from "@std/assert";
import { isFrontendApplicationTypeScript } from "./check_frontend_changed_types.ts";

Deno.test("changed frontend type check includes application TypeScript", () => {
  assertEquals(
    isFrontendApplicationTypeScript("frontend/src/components/Composition.tsx"),
    true,
  );
  assertEquals(
    isFrontendApplicationTypeScript("frontend/src/lib/composition.ts"),
    true,
  );
});

Deno.test("changed frontend type check excludes tests, declarations, and other paths", () => {
  assertEquals(
    isFrontendApplicationTypeScript(
      "frontend/src/components/Composition.test.tsx",
    ),
    false,
  );
  assertEquals(
    isFrontendApplicationTypeScript("frontend/src/types.d.ts"),
    false,
  );
  assertEquals(isFrontendApplicationTypeScript("tools/composition.ts"), false);
});
