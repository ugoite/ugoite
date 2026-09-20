/** Escape and render Markdown text for read-only previews. */
function escapeHtml(text: string): string {
  return text
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}

export function renderMarkdownPreview(
  markdown: string | undefined | null,
): string {
  const safeContent = escapeHtml(markdown ?? "");
  return safeContent
    .replace(/^# (.+)$/gm, '<h1 class="text-2xl font-bold mb-2">$1</h1>')
    .replace(
      /^## (.+)$/gm,
      '<h2 class="text-xl font-semibold mb-2 mt-4">$1</h2>',
    )
    .replace(
      /^### (.+)$/gm,
      '<h3 class="text-lg font-medium mb-1 mt-3">$1</h3>',
    )
    .replace(/\*\*(.+?)\*\*/g, "<strong>$1</strong>")
    .replace(/\*(.+?)\*/g, "<em>$1</em>")
    .replace(/`(.+?)`/g, '<code class="ui-code">$1</code>')
    .replace(/\n/g, "<br>");
}
