import { dirname, join } from "node:path";

const [manifestPath, outputPath] = Deno.args;

if (!manifestPath || !outputPath) {
  throw new Error(
    "Usage: generate-static-index.ts <manifest.json> <index.html>",
  );
}

type ManifestEntry = {
  file: string;
  css?: string[];
  imports?: string[];
};

const manifest = JSON.parse(await Deno.readTextFile(manifestPath)) as Record<
  string,
  ManifestEntry
>;
const clientEntry = manifest["virtual:$vinxi/handler/client"];

if (!clientEntry?.file) {
  throw new Error("Missing virtual:$vinxi/handler/client in client manifest");
}

const toBuildPath = (path: string): string => `/_build/${path}`;
const inputs = Object.fromEntries(
  Object.entries(manifest)
    .filter(([, entry]) => entry.file)
    .map(([key, entry]) => [
      key,
      {
        output: toBuildPath(entry.file),
        assets: (entry.css ?? []).map(toBuildPath),
      },
    ]),
);

const preloadLinks = [...new Set(clientEntry.imports ?? [])]
  .map((key) => manifest[key]?.file)
  .filter((file): file is string => Boolean(file))
  .map((file) => `\t\t<link rel="modulepreload" href="${toBuildPath(file)}">`)
  .join("\n");
const stylesheetLinks = (clientEntry.css ?? [])
  .map((file) => `\t\t<link rel="stylesheet" href="${toBuildPath(file)}">`)
  .join("\n");
const manifestScript = JSON.stringify(inputs);
const manifestScriptPath = join(
  dirname(outputPath),
  "_build",
  "ugoite-manifest.js",
);

const html = `<!doctype html>
<html lang="en">
	<head>
		<meta charset="utf-8">
		<meta name="viewport" content="width=device-width, initial-scale=1">
		<title>Ugoite</title>
		<link rel="icon" href="/favicon.ico">
${preloadLinks}
${stylesheetLinks}
		<script src="/_build/ugoite-manifest.js"></script>
	</head>
	<body>
		<div id="app"></div>
		<script type="module" src="${toBuildPath(clientEntry.file)}"></script>
	</body>
</html>
`;

await Deno.mkdir(dirname(manifestScriptPath), { recursive: true });
await Deno.writeTextFile(
  manifestScriptPath,
  `window.manifest = ${manifestScript};\n`,
);
await Deno.writeTextFile(outputPath, html);
