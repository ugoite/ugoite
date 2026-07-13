import { useNavigate, useParams, useSearchParams } from "@solidjs/router";
import { createMemo, createResource, createSignal, For, Show } from "solid-js";
import { SpaceShell } from "~/components/SpaceShell";
import { UiIcon } from "~/components/UiIcon";
import { buildEntryMarkdownByMode, type EntryInputMode } from "~/lib/entry-input";
import { createEntryStore } from "~/lib/entry-store";
import { filterCreatableEntryForms } from "~/lib/metadata-forms";
import { formApi, spaceApi } from "~/lib/ugoite-client";

export default function NewEntryRoute() {
  const params = useParams<{ space_id: string }>();
  const navigate = useNavigate();
  const [search] = useSearchParams();
  const spaceId = () => params.space_id;
  const store = createEntryStore(spaceId);
  const [space] = createResource(spaceId, spaceApi.get);
  const [forms] = createResource(spaceId, formApi.list);
  const available = createMemo(() => filterCreatableEntryForms(forms() ?? []));
  const configuredDefault = createMemo(() => typeof space()?.settings?.default_form === "string" ? String(space()?.settings?.default_form) : "");
  const [manualForm, setManualForm] = createSignal(String(search.form || ""));
  const selectedName = createMemo(() => manualForm() || configuredDefault() || (available().length === 1 ? available()[0]?.name ?? "" : ""));
  const selected = createMemo(() => available().find((form) => form.name === selectedName()));
  const [title, setTitle] = createSignal("");
  const [mode, setMode] = createSignal<EntryInputMode>("webform");
  const [markdown, setMarkdown] = createSignal("");
  const [values, setValues] = createSignal<Record<string,string>>({});
  const [focusIndex, setFocusIndex] = createSignal(0);
  const [error, setError] = createSignal("");
  const fields = createMemo(() => Object.entries(selected()?.fields ?? {}));
  const setValue = (name: string, value: string) => setValues((current) => ({ ...current, [name]: value }));
  const submit = async (event: Event) => {
    event.preventDefault(); setError("");
    const form = selected();
    if (!form) { setError("Select a Form before entering content."); return; }
    try {
      const content = mode() === "markdown" && markdown().trim() ? markdown() : buildEntryMarkdownByMode(form, title(), values(), mode());
      const result = await store.createEntry(content);
      navigate(`/spaces/${spaceId()}/entries/${encodeURIComponent(result.id)}`);
    } catch (cause) { setError(cause instanceof Error ? cause.message : "Failed to create entry."); }
  };
  return (
    <SpaceShell spaceId={spaceId()} activeNavigation="forms" title="New Entry">
      <div class="screenHead"><div class="screenTitle"><div class="eyebrow">{space()?.name || spaceId()}</div><h1>New Entry</h1></div></div>
      <form class="newEntryLayout" onSubmit={submit}>
        <aside class="formPicker surface"><div class="paneHead"><b>Form</b></div><For each={available()} fallback={<p class="ui-muted p-3">Create a Form first.</p>}>{(form) => <button class="formItem" classList={{ active: selectedName() === form.name }} type="button" onClick={() => { setManualForm(form.name); setValues({}); setFocusIndex(0); }}><span class="glyph" classList={{ active: selectedName() === form.name }}>{form.name.slice(0,1).toUpperCase()}</span><span><b>{form.name}</b><small>{Object.keys(form.fields).join(" · ")}</small></span><span>›</span></button>}</For></aside>
        <Show when={selected()} fallback={<section class="entryComposer surface"><p class="ui-muted">Select a Form to display its entry fields.</p></section>}>
          {(form) => <section class="entryComposer surface ui-stack">
            <div class="contextBar"><div class="contextLeft"><span class="glyph active">{form().name.slice(0,1).toUpperCase()}</span><span><b>{form().name}</b><small>New Entry</small></span></div><div class="modeBar"><button type="button" classList={{ active: mode() === "webform" }} onClick={() => setMode("webform")}>Form</button><button type="button" classList={{ active: mode() === "markdown" }} onClick={() => setMode("markdown")}>Markdown</button><button type="button" classList={{ active: mode() === "chat" }} onClick={() => setMode("chat")}>Focus</button></div></div>
            <label class="fieldCard wide"><span class="fieldLabel"><UiIcon name="entry" /> Title</span><input value={title()} onInput={(event) => setTitle(event.currentTarget.value)} required /></label>
            <Show when={mode() === "markdown"}><label class="fieldCard wide"><span class="fieldLabel">Markdown</span><textarea value={markdown()} onInput={(event) => setMarkdown(event.currentTarget.value)} placeholder={`# ${title() || "Entry title"}`} /></label></Show>
            <Show when={mode() === "webform"}><div class="entryGrid"><For each={fields()}>{([name, field]) => <label class="fieldCard" classList={{ wide: field.type === "markdown" }}><span class="fieldLabel">{name}{field.required ? " *" : ""}</span>{field.type === "markdown" ? <textarea value={values()[name] ?? ""} onInput={(event) => setValue(name, event.currentTarget.value)} required={field.required} /> : <input value={values()[name] ?? ""} onInput={(event) => setValue(name, event.currentTarget.value)} required={field.required} />}</label>}</For></div></Show>
            <Show when={mode() === "chat"}><div class="fieldCard wide"><Show when={fields()[focusIndex()]} fallback={<p>All fields are ready.</p>}>{(current) => <label class="ui-field"><span class="fieldLabel">{current()[0]}{current()[1].required ? " *" : ""}</span><textarea autofocus value={values()[current()[0]] ?? ""} onInput={(event) => setValue(current()[0], event.currentTarget.value)} /><div class="actions"><button class="btn" type="button" disabled={focusIndex() === 0} onClick={() => setFocusIndex((value) => value - 1)}>Previous</button><button class="btn primary" type="button" disabled={focusIndex() >= fields().length - 1} onClick={() => setFocusIndex((value) => value + 1)}>Next</button></div></label>}</Show></div></Show>
            <Show when={error()}><p class="ui-alert ui-alert-error">{error()}</p></Show>
            <div class="actions"><button class="btn" type="button" onClick={() => navigate(-1)}>Cancel</button><button class="btn primary" type="submit">Create Entry</button></div>
          </section>}
        </Show>
      </form>
    </SpaceShell>
  );
}
