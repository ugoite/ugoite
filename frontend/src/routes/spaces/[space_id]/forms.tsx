import { A, useParams } from "@solidjs/router";
import type { RouteSectionProps } from "@solidjs/router";
import { createMemo, createResource } from "solid-js";
import { formApi } from "~/lib/ugoite-client";
import { EntriesRouteContext } from "~/lib/entries-route-context";
import { createEntryStore } from "~/lib/entry-store";
import { createSpaceStore } from "~/lib/space-store";

export default function SpaceFormsRoute(props: RouteSectionProps) {
  const params = useParams<{ space_id: string }>();
  const spaceStore = createSpaceStore();
  const spaceId = () => params.space_id || "";
  const entryStore = createEntryStore(spaceId);

  const [metadata, { refetch: refetchForms }] = createResource(
    () => {
      const wsId = spaceId();
      return wsId ? wsId : null;
    },
    async (wsId) => {
      if (!wsId) return { forms: [], columnTypes: [] };
      const forms = await formApi.list(wsId);
      // The filesystem-backed catalog initializes lazily. Fetch metadata in
      // sequence so the two endpoints cannot race the same initialization.
      const columnTypes = await formApi.listTypes(wsId);
      return { forms, columnTypes };
    },
  );

  const safeForms = createMemo(() => metadata()?.forms || []);
  const loadingForms = createMemo(() => metadata.loading);
  const formsError = () => metadata.error;
  const columnTypes = createMemo(() => metadata()?.columnTypes || []);

  return (
    <EntriesRouteContext.Provider
      value={{
        spaceStore,
        spaceId,
        entryStore,
        forms: safeForms,
        loadingForms,
        formsError,
        columnTypes,
        refetchForms,
      }}
    >
      {props.children}
    </EntriesRouteContext.Provider>
  );
}
