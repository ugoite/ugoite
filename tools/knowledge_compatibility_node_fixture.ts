type AdapterFixtureResult = { loaded: boolean };

export function runAdapterFixture(value: string): AdapterFixtureResult {
  return { loaded: value === "loaded" };
}
