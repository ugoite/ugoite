import { expect, test } from "@playwright/test";
import { join } from "node:path";
import { getBackendUrl, waitForServers } from "./lib/client.ts";
import { openIsolatedPasskeyPage } from "./lib/security-context.ts";

type RemoteBarrier = {
  proxyUrl: string;
  waitForSecondPage: () => Promise<void>;
  waitForFirstPageResponse: () => Promise<void>;
  waitForCredentialExpiry: () => Promise<void>;
  releasePage: () => void;
  queryRequests: () => number;
  refreshRequests: () => number;
  successfulRefreshResponses: () => number;
  sqlPageIdentity: () => Array<{ sql: string; hasContinuation: boolean }>;
  leaksOpaqueContinuation: (text: string) => boolean;
  safeEvents: () => string[];
  close: () => Promise<void>;
};

function deferred() {
  let resolve!: () => void;
  const promise = new Promise<void>((done) => resolve = done);
  return { promise, resolve };
}

/**
 * A forward-only HTTP proxy that pauses the second SQL page request until the
 * test has revoked membership. It forwards the original URL, headers, body,
 * status, and response body to the real server; it never supplies an API
 * response or participates in authorization.
 */
async function startRemoteBarrier(
  backendUrl: string,
  spaceId: string,
  options: { holdFirstPageResponse?: boolean } = {},
): Promise<RemoteBarrier> {
  const secondPageSeen = deferred();
  const firstPageResponseReady = deferred();
  const release = deferred();
  let sqlQueries = 0;
  let refreshGrants = 0;
  let successfulRefreshes = 0;
  let credentialExpiresAt: number | undefined;
  let released = false;
  let opaqueContinuation: string | undefined;
  const events: string[] = [];
  const sqlPageIdentity: Array<{ sql: string; hasContinuation: boolean }> = [];
  const server = Deno.serve(
    { hostname: "127.0.0.1", port: 0, onListen: () => {} },
    async (request: Request) => {
      const target = request.url.startsWith("http://") ||
          request.url.startsWith("https://")
        ? request.url
        : new URL(request.url, backendUrl).toString();
      const targetUrl = new URL(target);
      const body = request.method === "GET" || request.method === "HEAD"
        ? undefined
        : await request.arrayBuffer();
      events.push(`incoming ${request.method} ${targetUrl.pathname}`);

      let grantType = "";
      if (
        request.method === "POST" && targetUrl.pathname.endsWith("/oauth/token")
      ) {
        try {
          grantType = String(
            (JSON.parse(new TextDecoder().decode(body)) as {
              grant_type?: unknown;
            })
              .grant_type ?? "",
          );
        } catch {
          // Do not retain or log token request bodies.
        }
        if (grantType === "refresh_token") refreshGrants += 1;
      }

      if (
        request.method === "POST" &&
        isSpaceSqlQueryPath(targetUrl.pathname, spaceId)
      ) {
        sqlQueries += 1;
        const payload = JSON.parse(new TextDecoder().decode(body)) as {
          sql?: unknown;
          continuation?: unknown;
        };
        sqlPageIdentity.push({
          sql: String(payload.sql ?? ""),
          hasContinuation: typeof payload.continuation === "string",
        });
        if (sqlQueries === 2) {
          secondPageSeen.resolve();
          if (!released) await release.promise;
        }
      }

      const headers = new Headers(request.headers);
      for (
        const name of [
          "connection",
          "proxy-connection",
          "keep-alive",
          "transfer-encoding",
        ]
      ) {
        headers.delete(name);
      }
      const upstream = await fetch(targetUrl, {
        method: request.method,
        headers,
        body,
        redirect: "manual",
      });
      if (grantType === "refresh_token" && upstream.ok) {
        successfulRefreshes += 1;
      }
      if (
        grantType === "urn:ietf:params:oauth:grant-type:device_code" &&
        upstream.ok
      ) {
        const token = await upstream.clone().json() as {
          access_token?: unknown;
          expires_in?: unknown;
        };
        if (
          typeof token.access_token === "string" &&
          typeof token.expires_in === "number"
        ) {
          credentialExpiresAt = Date.now() + token.expires_in * 1000;
        }
      }
      if (
        request.method === "POST" &&
        isSpaceSqlQueryPath(targetUrl.pathname, spaceId) &&
        upstream.ok
      ) {
        const page = await upstream.clone().json() as { next?: unknown };
        if (typeof page.next === "string") opaqueContinuation = page.next;
      }
      let safeError = "";
      if (
        request.method === "POST" &&
        isSpaceSqlQueryPath(targetUrl.pathname, spaceId) &&
        !upstream.ok
      ) {
        const payload = await upstream.clone().json().catch(() => null) as {
          code?: unknown;
          kind?: unknown;
          message?: unknown;
        } | null;
        if (payload) {
          safeError = [payload.code, payload.kind, payload.message]
            .filter((value): value is string => typeof value === "string")
            .join("/");
        }
      }
      const responseHeaders = new Headers(upstream.headers);
      for (const name of ["connection", "transfer-encoding", "keep-alive"]) {
        responseHeaders.delete(name);
      }
      const responseBody = await upstream.arrayBuffer();
      if (
        options.holdFirstPageResponse &&
        request.method === "POST" &&
        isSpaceSqlQueryPath(targetUrl.pathname, spaceId) &&
        sqlQueries === 1
      ) {
        firstPageResponseReady.resolve();
        if (!released) await release.promise;
      }
      events.push(
        `${request.method} ${targetUrl.pathname} -> ${upstream.status} (${responseBody.byteLength} bytes)${
          safeError ? ` ${safeError}` : ""
        }`,
      );
      return new Response(responseBody, {
        status: upstream.status,
        statusText: upstream.statusText,
        headers: responseHeaders,
      });
    },
  );
  const address = server.addr as Deno.NetAddr;

  return {
    proxyUrl: `http://127.0.0.1:${address.port}`,
    waitForSecondPage: async () => {
      await Promise.race([
        secondPageSeen.promise,
        new Promise<never>((_, reject) =>
          setTimeout(
            () =>
              reject(
                new Error(
                  `page-two request was not observed after ${sqlQueries} SQL request(s); proxy: ${
                    events.join("; ") || "no requests"
                  }`,
                ),
              ),
            30_000,
          )
        ),
      ]);
    },
    waitForFirstPageResponse: async () => {
      await Promise.race([
        firstPageResponseReady.promise,
        new Promise<never>((_, reject) =>
          setTimeout(
            () => reject(new Error("page-one response was not observed")),
            30_000,
          )
        ),
      ]);
    },
    waitForCredentialExpiry: async () => {
      await Promise.race([
        new Promise<void>((resolve) => {
          const poll = () => {
            if (credentialExpiresAt !== undefined) resolve();
            else setTimeout(poll, 50);
          };
          poll();
        }),
        new Promise<never>((_, reject) =>
          setTimeout(
            () => reject(new Error("device token expiry was not observed")),
            30_000,
          )
        ),
      ]);
      const expiresAt = credentialExpiresAt;
      if (expiresAt === undefined) {
        throw new Error("device token expiry was not observed");
      }
      const delayMs = Math.max(0, expiresAt + 1_500 - Date.now());
      if (delayMs > 0) {
        await new Promise((resolve) => setTimeout(resolve, delayMs));
      }
    },
    releasePage: () => {
      released = true;
      release.resolve();
    },
    queryRequests: () => sqlQueries,
    refreshRequests: () => refreshGrants,
    successfulRefreshResponses: () => successfulRefreshes,
    sqlPageIdentity: () => [...sqlPageIdentity],
    leaksOpaqueContinuation: (text: string) =>
      Boolean(opaqueContinuation && text.includes(opaqueContinuation)),
    safeEvents: () => [...events],
    close: async () => {
      release.resolve();
      await server.shutdown();
    },
  };
}

function cliBinary(): string {
  const path = Deno.env.get("UGOITE_CLI_BIN")?.trim();
  if (!path) {
    throw new Error(
      "UGOITE_CLI_BIN must point to the built ugoite CLI; use scripts/test-sql-export-remote-auth.sh",
    );
  }
  return path;
}

function backendBaseUrl(): string {
  const url = Deno.env.get("BACKEND_URL")?.trim() ||
    Deno.env.get("FRONTEND_URL")?.trim() || "http://localhost:3000";
  return new URL("/api", url).toString().replace(/\/$/, "");
}

function isSpaceSqlQueryPath(path: string, spaceId: string): boolean {
  return path.endsWith(`/spaces/${spaceId}/sql/query`);
}

async function waitForCliApprovalUrl(
  child: Deno.ChildProcess,
): Promise<string> {
  const reader = child.stderr.getReader();
  const decoder = new TextDecoder();
  let buffered = "";
  const diagnostics: string[] = [];
  const timer = setTimeout(() => {
    try {
      child.kill("SIGKILL");
    } catch {
      // The child may have exited while the timer was queued.
    }
  }, 30_000);
  try {
    while (true) {
      const result = await reader.read();
      if (result.done) {
        const status = await child.status;
        const safeDiagnostics = diagnostics.join(" ")
          .replace(
            /(https?:\/\/[^\s"?]+\?user_code=)[^\s"&]+/gi,
            "$1[redacted]",
          )
          .replace(
            /"(?:device_code|user_code|access_token|refresh_token)"\s*:\s*"[^"]*"/gi,
            '[secret]:"[redacted]"',
          )
          .replace(/\b[A-Z0-9]{4}-[A-Z0-9]{4}\b/g, "[redacted]")
          .slice(0, 500);
        throw new Error(
          `CLI device authorization prompt was not emitted (exit ${status.code}); ${
            safeDiagnostics || "no stderr output"
          }`,
        );
      }
      buffered += decoder.decode(result.value, { stream: true });
      const lines = buffered.split(/\r?\n/);
      diagnostics.push(...lines.slice(0, -1));
      for (const line of lines) {
        try {
          const state = JSON.parse(line) as {
            verification_uri_complete?: string;
          };
          if (state.verification_uri_complete) {
            await reader.cancel();
            return state.verification_uri_complete;
          }
        } catch {
          // Wait for the complete machine-readable ceremony line.
        }
      }
      buffered = lines.at(-1) ?? "";
    }
  } finally {
    clearTimeout(timer);
  }
}

async function finishChild(
  child: Deno.ChildProcess,
  label: string,
): Promise<Deno.CommandStatus> {
  const timeout = setTimeout(() => child.kill("SIGKILL"), 45_000);
  try {
    const status = await child.status;
    if (!status.success) {
      throw new Error(`${label} exited with code ${status.code}`);
    }
    return status;
  } finally {
    clearTimeout(timeout);
  }
}

test.describe("SQL export Remote authorization and credential lifecycle", () => {
  test.beforeAll(async ({ request }) => await waitForServers(request));

  test("a live membership revoke rejects page two and leaves no output", async ({ browser, request }) => {
    const id = Date.now();
    const created = await request.post(getBackendUrl("/spaces"), {
      data: { slug: `sql-revoke-${id}`, name: `SQL revoke ${id}` },
    });
    expect(created.status()).toBe(201);
    const { space_uid: spaceId } = await created.json() as {
      space_uid: string;
    };

    const form = await request.post(getBackendUrl(`/spaces/${spaceId}/forms`), {
      data: {
        name: `RemoteExport${id}`,
        version: 1,
        template: "# Remote export\n\n## Value\n",
        fields: { Value: { type: "string", required: true } },
      },
    });
    expect([200, 201]).toContain(form.status());
    for (const value of ["first", "second"]) {
      const entry = await request.post(
        getBackendUrl(`/spaces/${spaceId}/entries`),
        { data: { form: `RemoteExport${id}`, fields: { Value: value } } },
      );
      expect(entry.status()).toBe(201);
    }

    const invitation = await request.post(
      getBackendUrl(`/spaces/${spaceId}/members/invitations`),
      { data: { label: `Remote exporter ${id}`, role: "viewer" } },
    );
    expect(invitation.status()).toBe(201);
    const { invitation_url: invitationUrl } = await invitation.json() as {
      invitation_url: string;
    };
    const invitedName = `Remote exporter ${id}`;
    let viewerId: string | undefined;

    const configDir = await Deno.makeTempDir({ prefix: "ugoite-remote-auth-" });
    const configPath = join(configDir, "config.toml");
    const home = join(configDir, "home");
    const outputPath = join(configDir, "revoked.ndjson");
    const backend = backendBaseUrl();
    const barrier = await startRemoteBarrier(backend, spaceId);
    const loginConfig =
      `version = 1\ncurrent_context = "test"\n\n[connections.remote]\ntype = "backend"\nurl = ${
        JSON.stringify(backend)
      }\n\n[contexts.test]\nconnection = "remote"\nspace_uid = ${
        JSON.stringify(spaceId)
      }\ncredential = "remote-auth"\n`;
    await Deno.writeTextFile(configPath, loginConfig);
    await Deno.mkdir(home, { recursive: true });

    try {
      const isolated = await openIsolatedPasskeyPage(browser);
      try {
        await isolated.page.goto(invitationUrl);
        await isolated.page.getByRole("button", { name: "Accept invitation" })
          .click();
        await expect(isolated.page).toHaveURL(/\/spaces$/);

        const login = new Deno.Command(cliBinary(), {
          args: [
            "--config",
            configPath,
            "auth",
            "login",
            "--connection",
            "remote",
            "--credential",
            "remote-auth",
            "--device-name",
            `SQL export revocation ${id}`,
            "--space-uid",
            spaceId,
            "--actions",
            "read",
          ],
          env: {
            HOME: home,
            HTTP_PROXY: barrier.proxyUrl,
            http_proxy: barrier.proxyUrl,
            NO_PROXY: "",
            no_proxy: "",
          },
          stdin: "null",
          stdout: "null",
          stderr: "piped",
        }).spawn();
        let approvalUrl: string;
        try {
          approvalUrl = await waitForCliApprovalUrl(login);
        } catch (error) {
          throw new Error(
            `${
              error instanceof Error ? error.message : String(error)
            }; proxy: ${barrier.safeEvents().join("; ")}`,
          );
        }
        await isolated.page.goto(approvalUrl);
        await expect(isolated.page.getByRole("heading", {
          name: "Approve CLI access",
        })).toBeVisible();
        await isolated.page.getByRole("button", {
          name: "Review CLI access request",
        }).click();
        await isolated.page.getByRole("button", { name: "Approve CLI access" })
          .click();
        await expect(isolated.page.getByRole("heading", {
          name: "CLI access approved",
        })).toBeVisible();
        await finishChild(login, "device authorization");
      } finally {
        await isolated.close();
      }

      const memberList = await request.get(
        getBackendUrl(`/spaces/${spaceId}/members`),
      );
      const refreshedMembers = await memberList.json() as Array<{
        principal: { principal_id: string; display_name: string };
      }>;
      viewerId = refreshedMembers.find((member) =>
        member.principal.display_name === invitedName
      )?.principal.principal_id;
      expect(viewerId).toBeTruthy();

      const forms = await request.get(
        getBackendUrl(`/spaces/${spaceId}/forms`),
      );
      expect(forms.ok()).toBeTruthy();
      const formList = await forms.json() as Array<{
        name: string;
        sql_relation?: string;
      }>;
      const relation = formList.find((form) =>
        form.name === `RemoteExport${id}`
      )
        ?.sql_relation;
      expect(relation).toBeTruthy();
      const exportSql =
        `SELECT _ugoite_id FROM "${relation}" ORDER BY _ugoite_id`;
      const exportProcess = new Deno.Command(cliBinary(), {
        args: [
          "--config",
          configPath,
          "sql",
          "export",
          exportSql,
          "--max-rows",
          "10",
          "--page-size",
          "1",
          "--output",
          outputPath,
        ],
        env: {
          HOME: home,
          HTTP_PROXY: barrier.proxyUrl,
          http_proxy: barrier.proxyUrl,
          NO_PROXY: "",
          no_proxy: "",
        },
        stdin: "null",
        stdout: "piped",
        stderr: "piped",
      }).spawn();

      await barrier.waitForSecondPage();
      const revoked = await request.delete(
        getBackendUrl(`/spaces/${spaceId}/members/${viewerId}`),
      );
      expect(revoked.status()).toBe(200);
      barrier.releasePage();

      const result = await exportProcess.output();
      expect(result.success).toBe(false);
      expect(barrier.queryRequests()).toBe(2);
      const pages = barrier.sqlPageIdentity();
      expect(pages).toHaveLength(2);
      expect(pages[0].hasContinuation).toBe(false);
      expect(pages[1].hasContinuation).toBe(true);
      expect(pages[1].sql).toBe(pages[0].sql);
      const sqlResponses = barrier.safeEvents().filter((event) =>
        event.includes(`/spaces/${spaceId}/sql/query ->`)
      );
      expect(sqlResponses[0]).toContain("-> 200");
      expect(sqlResponses[1]).toContain("-> 403");
      expect(sqlResponses[1]).toMatch(/FORBIDDEN/i);
      expect(await Deno.stat(outputPath).catch(() => null)).toBeNull();
      const leftovers = [];
      for await (const entry of Deno.readDir(configDir)) {
        if (entry.name.startsWith(".ugoite-export-")) {
          leftovers.push(entry.name);
        }
      }
      expect(leftovers).toEqual([]);
      const stderr = new TextDecoder().decode(result.stderr);
      expect(stderr).toMatch(/FORBIDDEN|forbidden/i);
      expect(stderr).toMatch(/rows_exported/i);
      expect(barrier.leaksOpaqueContinuation(stderr)).toBe(false);
    } finally {
      barrier.releasePage();
      await barrier.close();
      await Deno.remove(configDir, { recursive: true }).catch(() => {});
    }
  });
  test("refreshes a credential that expires between SQL pages", async ({ browser, request }) => {
    test.setTimeout(420_000);
    const id = Date.now();
    const created = await request.post(getBackendUrl("/spaces"), {
      data: { slug: `sql-expiry-${id}`, name: `SQL expiry ${id}` },
    });
    expect(created.status()).toBe(201);
    const { space_uid: spaceId } = await created.json() as {
      space_uid: string;
    };

    const form = await request.post(getBackendUrl(`/spaces/${spaceId}/forms`), {
      data: {
        name: `RemoteExpiry${id}`,
        version: 1,
        template: "# Remote expiry\n\n## Value\n",
        fields: { Value: { type: "string", required: true } },
      },
    });
    expect([200, 201]).toContain(form.status());
    for (const value of ["first", "second"]) {
      const entry = await request.post(
        getBackendUrl(`/spaces/${spaceId}/entries`),
        { data: { form: `RemoteExpiry${id}`, fields: { Value: value } } },
      );
      expect(entry.status()).toBe(201);
    }

    const invitation = await request.post(
      getBackendUrl(`/spaces/${spaceId}/members/invitations`),
      { data: { label: `Remote expiry ${id}`, role: "viewer" } },
    );
    expect(invitation.status()).toBe(201);
    const { invitation_url: invitationUrl } = await invitation.json() as {
      invitation_url: string;
    };
    const configDir = await Deno.makeTempDir({
      prefix: "ugoite-remote-expiry-",
    });
    const configPath = join(configDir, "config.toml");
    const home = join(configDir, "home");
    const outputPath = join(configDir, "expired-between-pages.ndjson");
    const backend = backendBaseUrl();
    const barrier = await startRemoteBarrier(backend, spaceId, {
      holdFirstPageResponse: true,
    });
    const loginConfig =
      `version = 1\ncurrent_context = "test"\n\n[connections.remote]\ntype = "backend"\nurl = ${
        JSON.stringify(backend)
      }\n\n[contexts.test]\nconnection = "remote"\nspace_uid = ${
        JSON.stringify(spaceId)
      }\ncredential = "remote-auth"\n`;
    await Deno.writeTextFile(configPath, loginConfig);
    await Deno.mkdir(home, { recursive: true });

    try {
      const isolated = await openIsolatedPasskeyPage(browser);
      try {
        await isolated.page.goto(invitationUrl);
        await isolated.page.getByRole("button", { name: "Accept invitation" })
          .click();
        await expect(isolated.page).toHaveURL(/\/spaces$/);

        const login = new Deno.Command(cliBinary(), {
          args: [
            "--config",
            configPath,
            "auth",
            "login",
            "--connection",
            "remote",
            "--credential",
            "remote-auth",
            "--device-name",
            `SQL export expiry ${id}`,
            "--space-uid",
            spaceId,
            "--actions",
            "read",
          ],
          env: {
            HOME: home,
            HTTP_PROXY: barrier.proxyUrl,
            http_proxy: barrier.proxyUrl,
            NO_PROXY: "",
            no_proxy: "",
          },
          stdin: "null",
          stdout: "null",
          stderr: "piped",
        }).spawn();
        let approvalUrl: string;
        try {
          approvalUrl = await waitForCliApprovalUrl(login);
        } catch (error) {
          throw new Error(
            `${
              error instanceof Error ? error.message : String(error)
            }; proxy: ${barrier.safeEvents().join("; ")}`,
          );
        }
        await isolated.page.goto(approvalUrl);
        await expect(isolated.page.getByRole("heading", {
          name: "Approve CLI access",
        })).toBeVisible();
        await isolated.page.getByRole("button", {
          name: "Review CLI access request",
        }).click();
        await isolated.page.getByRole("button", { name: "Approve CLI access" })
          .click();
        await expect(isolated.page.getByRole("heading", {
          name: "CLI access approved",
        })).toBeVisible();
        await finishChild(login, "device authorization");
      } finally {
        await isolated.close();
      }

      const forms = await request.get(
        getBackendUrl(`/spaces/${spaceId}/forms`),
      );
      expect(forms.ok()).toBeTruthy();
      const formList = await forms.json() as Array<
        { name: string; sql_relation?: string }
      >;
      const relation = formList.find((item) =>
        item.name === `RemoteExpiry${id}`
      )
        ?.sql_relation;
      expect(relation).toBeTruthy();
      const exportSql =
        `SELECT _ugoite_id FROM "${relation}" ORDER BY _ugoite_id`;
      const exportProcess = new Deno.Command(cliBinary(), {
        args: [
          "--config",
          configPath,
          "sql",
          "export",
          exportSql,
          "--max-rows",
          "10",
          "--page-size",
          "1",
          "--output",
          outputPath,
        ],
        env: {
          HOME: home,
          HTTP_PROXY: barrier.proxyUrl,
          http_proxy: barrier.proxyUrl,
          NO_PROXY: "",
          no_proxy: "",
        },
        stdin: "null",
        stdout: "piped",
        stderr: "piped",
      }).spawn();

      await barrier.waitForFirstPageResponse();
      await barrier.waitForCredentialExpiry();
      barrier.releasePage();
      const result = await exportProcess.output();
      expect(result.success).toBe(true);
      expect(barrier.queryRequests()).toBe(2);
      expect(barrier.refreshRequests()).toBe(1);
      expect(barrier.successfulRefreshResponses()).toBe(1);
      const pages = barrier.sqlPageIdentity();
      expect(pages).toHaveLength(2);
      expect(pages[0].hasContinuation).toBe(false);
      expect(pages[1].hasContinuation).toBe(true);
      expect(pages[1].sql).toBe(pages[0].sql);
      expect(await Deno.stat(outputPath).then((info) => info.isFile)).toBe(
        true,
      );
      const outputRows = (await Deno.readTextFile(outputPath)).trim().split(
        "\n",
      );
      expect(outputRows).toHaveLength(2);
      expect(barrier.leaksOpaqueContinuation(outputRows.join("\n"))).toBe(
        false,
      );
      expect(
        barrier.safeEvents().some((event) => event.includes("/oauth/token")),
      ).toBe(true);
    } finally {
      barrier.releasePage();
      await barrier.close();
      await Deno.remove(configDir, { recursive: true }).catch(() => {});
    }
  });
});
