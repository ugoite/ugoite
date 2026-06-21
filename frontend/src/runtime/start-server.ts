// @refresh skip
import { eventHandler } from "vinxi/http";

type ManifestAsset = {
  attrs: Record<string, string>;
  tag: string;
  children?: string;
};

export type APIEvent = {
  request: Request;
  response?: Response;
  params?: Record<string, string>;
  nativeEvent?: unknown;
  context?: Record<string, unknown>;
};

export type HandlerOptions = {
  mode?: "sync" | "stream" | "async";
  nonce?: string;
  onCompleteAll?: (options: { write: (value: string) => void }) => void;
  onCompleteShell?: (options: { write: (value: string) => void }) => void;
};

export type DocumentComponentProps = {
  assets: unknown;
  scripts: unknown;
  children: unknown;
};

export type PageEvent = APIEvent & {
  assets: ManifestAsset[];
  manifest: unknown;
  router: {
    submission: unknown;
  };
  routes: unknown[];
  complete: boolean;
  $islands: Set<unknown>;
  nonce?: string;
  response: Response;
};

const buildClientBootHtml = async (): Promise<string> => {
  const env = (import.meta as unknown as {
    env: {
      MANIFEST?: Record<string, any>;
    };
  }).env;
  const clientManifest = env.MANIFEST?.client;
  const clientHandler = clientManifest?.handler;
  const clientInput = clientHandler
    ? clientManifest.inputs[clientHandler]
    : "/_build/client.js";
  const scriptPath = clientInput && typeof clientInput !== "string"
    ? clientInput.output.path
    : "/_build/client.js";
  const serializableManifest = clientManifest?.json
    ? await clientManifest.json()
    : Object.fromEntries(
      await Promise.all(
        Object.entries(clientManifest?.inputs ?? {}).map(async (
          [name, input],
        ) => [
          name,
          {
            output: { path: input.output.path },
            assets: await input.assets(),
          },
        ]),
      ),
    );
  const manifestScript = `window.manifest = ${
    JSON.stringify(serializableManifest)
  }`;

  return `<!DOCTYPE html>
<html lang="en">
  <head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <link rel="icon" href="/favicon.ico">
    <script>${manifestScript}</script>
  </head>
  <body>
    <div id="app"></div>
    <script type="module" src="${scriptPath}"></script>
  </body>
</html>`;
};

export function createHandler(
  _fn: (context: PageEvent) => unknown,
  _options?: HandlerOptions,
) {
  return eventHandler(async () => {
    return new Response(await buildClientBootHtml(), {
      headers: {
        "content-type": "text/html; charset=utf-8",
      },
    });
  });
}

export function StartServer(_props: {
  document: (props: DocumentComponentProps) => unknown;
}) {
  return null;
}
