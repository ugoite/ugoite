import { useNavigate, useParams, useSearchParams } from "@solidjs/router";
import {
  createEffect,
  createMemo,
  createSignal,
  Show,
} from "solid-js";
import { EntryDetailPane } from "~/components/EntryDetailPane";
import { SpaceShell } from "~/components/SpaceShell";
import { filterCreatableEntryForms } from "~/lib/metadata-forms";
import { formApi, spaceApi } from "~/lib/ugoite-client";
import { createResource } from "~/lib/recoverable-resource";

export default function NewEntryRoute() {
  const params = useParams<{ space_id: string }>();
  const navigate = useNavigate();
  const [searchParams] = useSearchParams();
  const spaceId = () => params.space_id;
  const [space, { refetch: refetchSpace }] = createResource(
    spaceId,
    spaceApi.get,
  );
  const [forms, { refetch: refetchForms }] = createResource(
    spaceId,
    formApi.list,
  );
  const available = createMemo(() => filterCreatableEntryForms(forms() ?? []));
  const requestedForm = () =>
    typeof searchParams.form === "string" ? searchParams.form : "";
  const configuredDefault = () =>
    typeof space()?.settings?.default_form === "string"
      ? space()?.settings?.default_form as string
      : "";
  const defaultForm = createMemo(() => {
    return available().find((form) => form.name === requestedForm())?.name ??
      available().find((form) => form.name === configuredDefault())?.name ??
      available()[0]?.name;
  });
  const [selectedFormName, setSelectedFormName] = createSignal<
    string | undefined
  >();
  createEffect(() => {
    const fallback = defaultForm();
    if (!fallback) {
      setSelectedFormName(undefined);
      return;
    }
    if (!available().some((form) => form.name === selectedFormName())) {
      setSelectedFormName(fallback);
    }
  });
  const selectedForm = createMemo(() =>
    available().find((form) => form.name === selectedFormName()) ??
      available().find((form) => form.name === defaultForm())
  );

  return (
    <SpaceShell spaceId={spaceId()} activeNavigation="forms" title="New Entry">
      <Show
        when={!space.loading && !forms.loading}
        fallback={
          <div class="surface emptyState" role="status">
            Loading entry form…
          </div>
        }
      >
        <Show
          when={!space.error && !forms.error}
          fallback={
            <section class="surface emptyState" role="alert">
              <p>Could not load the Space or its Forms.</p>
              <button
                class="btn"
                type="button"
                onClick={() => {
                  void refetchSpace();
                  void refetchForms();
                }}
              >
                Retry
              </button>
            </section>
          }
        >
          <Show
            when={selectedForm()}
            fallback={
              <section class="surface emptyState" role="alert">
                <p>No creatable Forms are available in this Space.</p>
                <button
                  class="btn"
                  type="button"
                  onClick={() => navigate(`/spaces/${spaceId()}/forms`)}
                >
                  Back to Forms
                </button>
              </section>
            }
          >
            {(form) => (
              <EntryDetailPane
                spaceId={spaceId}
                forms={available}
                createForm={() => form()}
                onCreateFormChange={setSelectedFormName}
                onDeleted={() =>
                  navigate(`/spaces/${spaceId()}/forms`)}
                onCreated={({ id: entryId }) =>
                  navigate(
                    `/spaces/${spaceId()}/entries/${
                      encodeURIComponent(entryId)
                    }`,
                    { replace: true },
                  )}
              />
            )}
          </Show>
        </Show>
      </Show>
    </SpaceShell>
  );
}
