import { createContext, useContext } from "solid-js";
import type { Accessor } from "solid-js";
import type { EntryStore } from "~/lib/entry-store";
import type { Form } from "~/lib/types";
import type { SpaceStore } from "~/lib/space-store";

export interface EntriesRouteContextValue {
	spaceStore: SpaceStore;
	spaceId: Accessor<string>;
	entryStore: EntryStore;
	forms: Accessor<Form[]>;
	loadingForms: Accessor<boolean>;
	columnTypes: Accessor<string[]>;
	refetchForms: () => void;
}

export const EntriesRouteContext = createContext<EntriesRouteContextValue>();

export function useEntriesRouteContext(): EntriesRouteContextValue {
	const ctx = useContext(EntriesRouteContext);
	if (!ctx) {
		throw new Error(
			"EntriesRouteContext is missing. Ensure it is provided by the /spaces/{space_id}/entries route.",
		);
	}
	return ctx;
}
