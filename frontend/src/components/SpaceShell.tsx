import { A } from "@solidjs/router";
import type { JSX } from "solid-js";
import { Show } from "solid-js";
import { loadingState } from "~/lib/loading";

export type SpaceTopTab = "dashboard" | "search";
export type SpaceBottomTab = "object" | "grid";

interface SpaceShellProps {
	spaceId: string;
	activeTopTab?: SpaceTopTab;
	activeBottomTab?: SpaceBottomTab;
	showBottomTabs?: boolean;
	bottomTabHrefSuffix?: string;
	children: JSX.Element;
}

const tabBaseForms =
	"px-4 py-2 text-sm font-medium rounded-full transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-offset-2 focus-visible:ring-sky-500";

export function SpaceShell(props: SpaceShellProps) {
	const topTabForm = (tab: SpaceTopTab) =>
		props.activeTopTab === tab
			? `${tabBaseForms} bg-sky-600 text-white shadow`
			: `${tabBaseForms} text-slate-700 hover:bg-slate-100`;

	const bottomTabForm = (tab: SpaceBottomTab) =>
		props.activeBottomTab === tab
			? `${tabBaseForms} bg-slate-900 text-white shadow`
			: `${tabBaseForms} text-slate-700 hover:bg-slate-100`;

	return (
		<main class="min-h-screen bg-slate-50 text-slate-900 relative">
			<Show when={loadingState.isLoading()}>
				<div class="fixed top-0 left-0 right-0 z-50">
					<div class="h-1 w-full bg-sky-500 animate-pulse" />
				</div>
			</Show>
			<div class="fixed top-5 left-1/2 -translate-x-1/2 z-50">
				<div class="flex items-center gap-2 rounded-full bg-white/90 shadow-lg ring-1 ring-slate-200 px-2 py-2 backdrop-blur">
					<A href={`/spaces/${props.spaceId}/dashboard`} class={topTabForm("dashboard")}>
						dashboard
					</A>
					<A href={`/spaces/${props.spaceId}/search`} class={topTabForm("search")}>
						search
					</A>
				</div>
			</div>

			<A
				href={`/spaces/${props.spaceId}/settings`}
				class="fixed top-5 right-5 z-50 rounded-full bg-white/90 p-3 shadow-lg ring-1 ring-slate-200 backdrop-blur hover:bg-white"
				aria-label="Space settings"
			>
				<svg
					class="h-5 w-5"
					fill="none"
					stroke="currentColor"
					viewBox="0 0 24 24"
					aria-hidden="true"
				>
					<path
						stroke-linecap="round"
						stroke-linejoin="round"
						stroke-width="2"
						d="M12 6.75a5.25 5.25 0 100 10.5 5.25 5.25 0 000-10.5zm0-4.5v2.25m0 15v2.25m9.75-9h-2.25M4.5 12H2.25m16.22-6.72l-1.59 1.59M7.37 16.63l-1.59 1.59m0-11.28l1.59 1.59m11.28 11.28l1.59 1.59"
					/>
				</svg>
			</A>

			<div class={`px-6 ${props.showBottomTabs ? "pb-24" : "pb-12"} pt-24`}>{props.children}</div>

			{props.showBottomTabs && (
				<div class="fixed bottom-6 left-1/2 -translate-x-1/2 z-50">
					<div class="flex items-center gap-2 rounded-full bg-white/90 shadow-lg ring-1 ring-slate-200 px-2 py-2 backdrop-blur">
						<A
							href={`/spaces/${props.spaceId}/entries${props.bottomTabHrefSuffix || ""}`}
							class={bottomTabForm("object")}
						>
							object
						</A>
						<A
							href={`/spaces/${props.spaceId}/forms${props.bottomTabHrefSuffix || ""}`}
							class={bottomTabForm("grid")}
						>
							grid
						</A>
					</div>
				</div>
			)}
		</main>
	);
}
