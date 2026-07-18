import { A, useParams } from "@solidjs/router";
import type { RouteSectionProps } from "@solidjs/router";
import { createMemo, createResource } from "solid-js";
import { EntriesRouteContext } from "~/lib/entries-route-context";
import { formApi } from "~/lib/ugoite-client";
import { createEntryStore } from "~/lib/entry-store";
import { createSpaceStore } from "~/lib/space-store";

export default function SpaceEntriesRoute(props: RouteSectionProps) {
  const params = useParams<{ space_id: string }>();
  const spaceStore = createSpaceStore();
  const spaceId = () => params.space_id || "";
  const entryStore = createEntryStore(spaceId);

  const [metadata, { refetch: refetchMetadata }] = createResource(
    () => {
      const wsId = spaceId();
      return wsId ? wsId : null;
    },
    async (wsId) => {
      if (!wsId) return [];
      const forms = await formApi.list(wsId).catch(() => []);
      // The filesystem-backed catalog initializes lazily. Read its metadata
      // before mounting an entry detail request so a page reload never races
      // the same catalog initialization from two endpoints.
      const columnTypes = await formApi.listTypes(wsId).catch(() => []);
      return { forms, columnTypes };
    },
  );

  const safeForms = createMemo(() => metadata()?.forms || []);
  const safeColumnTypes = createMemo(() => metadata()?.columnTypes || []);
  const loadingForms = createMemo(() => metadata.loading);

  return (
    <EntriesRouteContext.Provider
      value={{
        spaceStore,
        spaceId,
        entryStore,
        forms: safeForms,
        loadingForms,
        columnTypes: safeColumnTypes,
        refetchForms: refetchMetadata,
      }}
    >
      {props.children}
    </EntriesRouteContext.Provider>
  );
}
