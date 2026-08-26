import type { CDPSession } from "@playwright/test";

type VirtualAuthenticator = {
  authenticatorId: string;
};

export async function addVirtualAuthenticator(
  cdp: CDPSession,
): Promise<string> {
  const result = await cdp.send("WebAuthn.addVirtualAuthenticator", {
    options: {
      protocol: "ctap2",
      transport: "internal",
      hasResidentKey: true,
      hasUserVerification: true,
      isUserVerified: true,
      automaticPresenceSimulation: true,
    },
  }) as VirtualAuthenticator;
  return result.authenticatorId;
}

export async function removeVirtualAuthenticator(
  cdp: CDPSession,
  authenticatorId: string,
): Promise<void> {
  await cdp.send("WebAuthn.removeVirtualAuthenticator", { authenticatorId });
}
