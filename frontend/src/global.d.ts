/// <reference types="@solidjs/start/env" />
/// <reference types="vitest/globals" />
/// <reference types="@testing-library/jest-dom" />

interface ImportMetaEnv {
  readonly DEV?: boolean;
  readonly MANIFEST?: Record<string, unknown>;
  readonly START_APP?: string;
  readonly START_ISLANDS?: boolean;
  readonly [key: string]: unknown;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
