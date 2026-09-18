import { useNavigate, useParams, useSearchParams } from "@solidjs/router";
import { createEffect, createMemo, createSignal, Show } from "solid-js";
import { EntryDetailPane } from "~/components/EntryDetailPane";
import { LocalBusyIndicator } from "~/components/LocalBusyIndicator";
import { filterCreatableEntryForms } from "~/lib/metadata-forms";
import { formApi, spaceApi } from "~/lib/ugoite-client";
import { createResource } from "~/lib/recoverable-resource";
import { t } from "~/lib/i18n";
import { spaceRoute } from "~/lib/space-shell-route";

export const route = spaceRoute({ navigation: "forms", title: "newEntry" });

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
  // Entry creation never consults a configured Space default Form. The only
  // explicit preselection is the requested `?form=` parameter; otherwise the
  // editor offers the available Forms without a hidden preference.
  const defaultForm = createMemo(() => {
    return available().find((form) => form.name === requestedForm())?.name ??
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
  const returnToForms = () => searchParams.returnTo === "forms";
  const formsHref = () => {
    const formName = selectedFormName() ?? selectedForm()?.name;
    const query = formName ? `?form=${encodeURIComponent(formName)}` : "";
    return `/spaces/${encodeURIComponent(spaceId())}/forms${query}`;
  };

  return (
    <>
      <Show
        when={!space.loading && !forms.loading}
        fallback={
          <div class="surface emptyState" aria-busy="true">
            {/* Spinner only: label is sr-only for assistive technology. */}
            <LocalBusyIndicator label={t("entryPage.loadingForm")} />
          </div>
        }
      >
        <Show
          when={!space.error && !forms.error}
          fallback={
            <section class="surface emptyState" role="alert">
              <p>{t("entryPage.failedLoad")}</p>
              <button
                class="btn"
                type="button"
                onClick={() => {
                  void refetchSpace();
                  void refetchForms();
                }}
              >
                {t("common.retry")}
              </button>
            </section>
          }
        >
          <Show
            when={selectedForm()}
            fallback={
              <section class="surface emptyState" role="alert">
                <p>{t("entryPage.noForms")}</p>
                <button
                  class="btn"
                  type="button"
                  onClick={() =>
                    navigate(`/spaces/${encodeURIComponent(spaceId())}/forms`)}
                >
                  {t("entryPage.backToForms")}
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
                  navigate(
                    returnToForms()
                      ? formsHref()
                      : `/spaces/${encodeURIComponent(spaceId())}/forms`,
                  )}
                onCreated={({ id: entryId }) => {
                  if (returnToForms()) {
                    navigate(formsHref(), { replace: true });
                    return;
                  }
                  navigate(
                    `/spaces/${encodeURIComponent(spaceId())}/entries/${
                      encodeURIComponent(entryId)
                    }`,
                    { replace: true },
                  );
                }}
              />
            )}
          </Show>
        </Show>
      </Show>
    </>
  );
}
