import { useNavigate, useParams } from "@solidjs/router";
import { createMemo, createResource } from "solid-js";
import { CreateEntryDialog } from "~/components/create-dialogs";
import { SpaceShell } from "~/components/SpaceShell";
import {
  buildEntryMarkdownByMode,
  type EntryInputMode,
} from "~/lib/entry-input";
import { createEntryStore } from "~/lib/entry-store";
import { filterCreatableEntryForms } from "~/lib/metadata-forms";
import { formApi, spaceApi } from "~/lib/ugoite-client";

export default function NewEntryRoute() {
  const params = useParams<{ space_id: string }>();
  const navigate = useNavigate();
  const spaceId = () => params.space_id;
  const store = createEntryStore(spaceId);
  const [space] = createResource(spaceId, spaceApi.get);
  const [forms] = createResource(spaceId, formApi.list);
  const available = createMemo(() => filterCreatableEntryForms(forms() ?? []));

  const createEntry = async (
    title: string,
    formName: string,
    values: Record<string, string>,
    mode: EntryInputMode = "webform",
  ) => {
    const form = available().find((candidate) => candidate.name === formName);
    if (!form) throw new Error("Select a Form before entering content.");
    const result = await store.createEntry(
      buildEntryMarkdownByMode(form, title, values, mode),
    );
    navigate(`/spaces/${spaceId()}/entries/${encodeURIComponent(result.id)}`);
  };

  return (
    <SpaceShell spaceId={spaceId()} activeNavigation="forms" title="New Entry">
      <div class="screenHead">
        <div class="screenTitle">
          <div class="eyebrow">{space()?.name || spaceId()}</div>
          <h1>New Entry</h1>
        </div>
      </div>
      <CreateEntryDialog
        open
        forms={available()}
        defaultForm={typeof space()?.settings?.default_form === "string"
          ? space()?.settings?.default_form
          : available()[0]?.name}
        spaceId={spaceId()}
        onClose={() =>
          navigate(-1)}
        onSubmit={createEntry}
      />
    </SpaceShell>
  );
}
