import { useParams } from "@solidjs/router";
import type { RouteSectionProps } from "@solidjs/router";
import { createMemo, createResource } from "solid-js";
import { formApi } from "~/lib/form-api";
import { EntriesRouteContext } from "~/lib/entries-route-context";
import { createEntryStore } from "~/lib/entry-store";
import { createSpaceStore } from "~/lib/space-store";

export default function SpaceFormsRoute(props: RouteSectionProps) {
	const params = useParams<{ space_id: string }>();
	const spaceStore = createSpaceStore();
	const spaceId = () => params.space_id || "";
	const entryStore = createEntryStore(spaceId);

	const [forms, { refetch: refetchForms }] = createResource(
		() => {
			const wsId = spaceId();
			return wsId ? wsId : null;
		},
		async (wsId) => {
			if (!wsId) return [];
			return await formApi.list(wsId);
		},
	);

	const [columnTypes] = createResource(
		() => spaceId(),
		async (wsId) => {
			if (!wsId) return [];
			return await formApi.listTypes(wsId);
		},
	);

	const safeForms = createMemo(() => forms() || []);
	const loadingForms = createMemo(() => forms.loading);

	return (
		<EntriesRouteContext.Provider
			value={{
				spaceStore,
				spaceId,
				entryStore,
				forms: safeForms,
				loadingForms,
				columnTypes: () => columnTypes() || [],
				refetchForms,
			}}
		>
			{props.children}
		</EntriesRouteContext.Provider>
	);
}
