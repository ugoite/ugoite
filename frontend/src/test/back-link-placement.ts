import { expect } from "vitest";

export function expectBackLinkAtHeaderStart(backLink: HTMLElement): void {
  const headerStart = backLink.closest<HTMLElement>(".screenHeadStart");
  expect(headerStart).not.toBeNull();
  expect(headerStart?.firstElementChild).toBe(backLink);
}
