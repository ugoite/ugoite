import { A, useParams } from "@solidjs/router";
import type { RouteSectionProps } from "@solidjs/router";
import { createMemo } from "solid-js";
import { EntriesRouteContext } from "~/lib/entries-route-context";
import { formApi } from "~/lib/ugoite-client";
import { createEntryStore } from "~/lib/entry-store";
import { createSpaceStore } from "~/lib/space-store";
import { createResource } from "~/lib/recoverable-resource";
import { spaceRoute } from "~/lib/space-shell-route";

export const route = spaceRoute({ navigation: "forms" });

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
      if (!wsId) return { forms: [], columnTypes: [] };
      const forms = await formApi.list(wsId);
      // The filesystem-backed catalog initializes lazily. Read its metadata
      // before mounting an entry detail request so a page reload never races
      // the same catalog initialization from two endpoints.
      const columnTypes = await formApi.listTypes(wsId);
      return { forms, columnTypes };
    },
  );

  // Reading a rejected resource throws. Check its error first so child routes
  // can render their recovery UI instead of falling through to Solid's error
  // boundary.
  const safeForms = createMemo(() =>
    metadata.error ? [] : metadata()?.forms || []
  );
  const safeColumnTypes = createMemo(() =>
    metadata.error ? [] : metadata()?.columnTypes || []
  );
  const loadingForms = createMemo(() => metadata.loading);
  const formsError = () => metadata.error;

  return (
    <EntriesRouteContext.Provider
      value={{
        spaceStore,
        spaceId,
        entryStore,
        forms: safeForms,
        loadingForms,
        formsError,
        columnTypes: safeColumnTypes,
        refetchForms: refetchMetadata,
      }}
    >
      {props.children}
    </EntriesRouteContext.Provider>
  );
}
