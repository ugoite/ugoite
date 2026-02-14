"""Entry input mode helpers shared by API layers."""

_FRONTMATTER_MIN_LINES = 3


def _replace_first_h1(markdown: str, title: str) -> str:
    lines = markdown.splitlines()
    for index, line in enumerate(lines):
        if line.startswith("# "):
            lines[index] = f"# {title}"
            return "\n".join(lines)
    return f"# {title}\n\n{markdown}".strip()


def _ensure_form_frontmatter(markdown: str, form_name: str) -> str:
    lines = markdown.splitlines()
    if len(lines) >= _FRONTMATTER_MIN_LINES and lines[0].strip() == "---":
        end_index = None
        for idx in range(1, len(lines)):
            if lines[idx].strip() == "---":
                end_index = idx
                break
        if end_index is not None:
            frontmatter = lines[1:end_index]
            has_form = any(item.strip().startswith("form:") for item in frontmatter)
            if not has_form:
                frontmatter.append(f"form: {form_name}")
            return "\n".join(["---", *frontmatter, "---", *lines[end_index + 1 :]])
    return f"---\nform: {form_name}\n---\n\n{markdown}".strip()


def _update_h2(markdown: str, name: str, value: str) -> str:
    lines = markdown.splitlines()
    header = f"## {name}"
    for index, line in enumerate(lines):
        if line.strip() != header:
            continue
        next_header = None
        for cursor in range(index + 1, len(lines)):
            if lines[cursor].startswith("## "):
                next_header = cursor
                break
        before = lines[: index + 1]
        after = lines[next_header:] if next_header is not None else []
        return "\n".join([*before, value, *after])
    suffix = f"\n\n{header}\n{value}" if markdown.strip() else f"{header}\n{value}"
    return f"{markdown.rstrip()}{suffix}"


def compose_entry_markdown_from_fields(
    template: str,
    form_name: str,
    title: str,
    field_values: dict[str, str],
) -> str:
    """Build entry markdown from web form style field values."""
    content = _ensure_form_frontmatter(_replace_first_h1(template, title), form_name)
    for name, value in field_values.items():
        if not name.startswith("__") and value.strip():
            content = _update_h2(content, name, value.strip())
    return content


def compose_entry_markdown_from_chat(
    template: str,
    form_name: str,
    title: str,
    qa_answers: dict[str, str],
) -> str:
    """Build entry markdown from chat Q&A style answers."""
    return compose_entry_markdown_from_fields(template, form_name, title, qa_answers)
