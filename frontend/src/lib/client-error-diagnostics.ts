const knownErrorTypes = new Set([
  "Error",
  "EvalError",
  "RangeError",
  "ReferenceError",
  "SyntaxError",
  "TypeError",
  "URIError",
]);

const routeFamilies = new Set([
  "assets",
  "compositions",
  "dashboard",
  "entries",
  "forms",
  "search",
  "settings",
  "sql",
]);

export interface ClientErrorDiagnostic {
  category: "app-error-boundary";
  errorType: string;
  fingerprint: string;
  routeFamily: string;
  stackLocations: string[];
}

function safeErrorType(error: unknown): string {
  try {
    if (!(error instanceof Error)) return "Error";
    const name = error.name;
    return knownErrorTypes.has(name) ? name : "Error";
  } catch {
    return "Error";
  }
}

function safeRouteFamily(pathname: string): string {
  const [path] = pathname.split(/[?#]/, 1);
  const segments = path.split("/").filter(Boolean);
  if (segments[0] !== "spaces") {
    return segments[0] === "login" ? "/login" : "/other";
  }
  if (segments.length === 1) return "/spaces";
  const family = segments[2];
  return `/spaces/:space/${family && routeFamilies.has(family) ? family : "other"}`;
}

function normalizedStackLocation(frame: string): string | undefined {
  let location = frame.trim();
  const parenthesized = location.match(/\((https?:\/\/[^()]+|file:\/\/[^()]+|\/[^()]+)\)/);
  if (parenthesized) {
    location = parenthesized[1];
  } else {
    if (location.startsWith("at ")) location = location.slice(3);
    const functionSeparator = location.lastIndexOf("@");
    if (functionSeparator >= 0) location = location.slice(functionSeparator + 1);
  }

  const match = location.match(/^(https?:\/\/\S+|file:\/\/\S+|\/\S+):(\d+):(\d+)$/);
  if (!match) return undefined;

  let path: string;
  try {
    path = match[1].startsWith("/")
      ? match[1]
      : new URL(match[1]).pathname;
  } catch {
    return undefined;
  }

  const fileName = path.split("/").at(-1) ?? "";
  const extension = fileName.match(/\.(js|mjs|cjs|ts|tsx|jsx|wasm)$/i)?.[1]
    ?.toLowerCase();
  if (!extension) return undefined;

  // Keep only a generic source kind and the frame position. Even a plausible
  // filename can contain a Space, Entry, or deployment identifier, so no path
  // segments or file names leave the browser through this diagnostic.
  const sourceKind = extension === "wasm"
    ? "wasm"
    : extension === "ts" || extension === "tsx"
    ? "typescript"
    : "javascript";
  return `${sourceKind}:${match[2]}:${match[3]}`;
}

function stackLocations(error: unknown): string[] {
  try {
    if (!(error instanceof Error) || typeof error.stack !== "string") return [];
    return error.stack
      .split("\n")
      .slice(1, 7)
      .map(normalizedStackLocation)
      .filter((location): location is string => location !== undefined)
      .slice(0, 4);
  } catch {
    return [];
  }
}

function fingerprint(errorType: string, locations: readonly string[]): string {
  const source = `${errorType}\0${locations.join("\n")}`;
  let hash = 2_166_136_261;
  for (let index = 0; index < source.length; index += 1) {
    hash = Math.imul(hash ^ source.charCodeAt(index), 16_777_619);
  }
  return (hash >>> 0).toString(16).padStart(8, "0");
}

export function createClientErrorDiagnostic(
  error: unknown,
  pathname: string,
): ClientErrorDiagnostic {
  const errorType = safeErrorType(error);
  const locations = stackLocations(error);
  return {
    category: "app-error-boundary",
    errorType,
    fingerprint: fingerprint(errorType, locations),
    routeFamily: safeRouteFamily(pathname),
    stackLocations: locations,
  };
}

export function reportClientErrorBoundary(
  error: unknown,
  pathname: string,
): void {
  try {
    console.error(
      "[ugoite:app-error]",
      createClientErrorDiagnostic(error, pathname),
    );
  } catch {
    // A diagnostics failure must not change the error fallback behavior.
  }
}
