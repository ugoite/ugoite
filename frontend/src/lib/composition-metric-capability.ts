import { compositionAggregateCapability } from "~/lib/ugoite-client";
import type { DraftAggregateOperation } from "~/lib/composition-draft";

/** Resolve supported Form field types from the Rust aggregate contract. */
export const supportedCompositionAggregateFieldTypes = async (
  operation: Exclude<DraftAggregateOperation, "count_rows">,
  fieldTypes: readonly string[],
): Promise<ReadonlySet<string>> => {
  const supported = new Set<string>();
  // The WASM protocol adapter uses a shared result buffer, so serialize calls.
  for (const fieldType of new Set(fieldTypes)) {
    try {
      const capability = await compositionAggregateCapability(
        operation,
        fieldType,
      );
      if (capability.supported) supported.add(fieldType);
    } catch {
      // An unavailable capability lookup fails closed for this field type.
    }
  }
  return supported;
};
