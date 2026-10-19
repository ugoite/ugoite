export type CompositionVariableName = { id: string };

/** Composition variables use their stable IDs as their visible names. */
export const compositionVariableDisplayName = (
  _variables: readonly CompositionVariableName[],
  variableId: string,
): string => variableId;
