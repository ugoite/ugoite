/**
 * Keep links in `docs/` GitHub-readable (`guide/automate/cli.md`) while emitting
 * the directory URLs used by Starlight (`docs/guide/automate/cli/`).
 *
 * This is a native Sätteri MDAST plugin. Using the default Astro 7 Markdown
 * processor lets Starlight add its own asides, heading links, and code-block
 * transforms to the same pipeline.
 */
const satteriDocLinks = {
  name: "ugoite-doc-links",
  link(node, context) {
    const sourcePath = context.fileURL?.pathname ?? "";
    const url = rewriteDocLink(node.url, sourcePath);
    if (url !== node.url) {
      context.setProperty(node, "url", url);
    }
  },
};

export default satteriDocLinks;

export function rewriteDocLink(url, sourcePath) {
  if (typeof url !== "string") {
    return url;
  }

  const [linkPath, fragment] = splitFragment(url);
  if (
    !linkPath ||
    /^[a-z][a-z\d+.-]*:/i.test(linkPath) ||
    !/(?:^|\/)\.?[^/]*\.mdx?$/i.test(linkPath)
  ) {
    return url;
  }

  const sourceIsIndex = /(?:^|[/\\])index\.mdx?$/i.test(sourcePath);
  const sourceIsDocsRoot = /(?:^|[/\\])docs[/\\]index\.mdx?$/i.test(
    sourcePath,
  );

  let route = linkPath.replace(/(^|\/)index\.mdx?$/i, "$1");
  if (route === linkPath) {
    route = linkPath.replace(/\.mdx?$/i, "/");
  }
  if (route === "") {
    route = "./";
  }

  // The root page is published at `/`, while every other authored document
  // keeps the established `/docs/` public namespace.
  if (sourceIsDocsRoot && !route.startsWith("/")) {
    route = route === "./" ? "./" : `docs/${route.replace(/^\.\//, "")}`;
  } else if (!sourceIsIndex && !route.startsWith("/")) {
    // A non-index source page is rendered as a directory URL. Move up once
    // before resolving source-relative links.
    route = route === "./" ? "../" : `../${route.replace(/^\.\//, "")}`;
  }

  return fragment ? `${route}#${fragment}` : route;
}

function splitFragment(url) {
  const index = url.indexOf("#");
  return index === -1 ? [url, ""] : [url.slice(0, index), url.slice(index + 1)];
}
