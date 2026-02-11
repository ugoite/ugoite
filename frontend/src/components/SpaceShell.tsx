import { A } from "@solidjs/router";
import type { JSX } from "solid-js";
import { Show } from "solid-js";
import { loadingState } from "~/lib/loading";
import { ThemeMenu } from "~/components/ThemeMenu";

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

export function SpaceShell(props: SpaceShellProps) {
	return (
		<main class="ui-shell">
			<Show when={loadingState.isLoading()}>
				<div class="fixed top-0 left-0 right-0 z-[60] pointer-events-none">
					<div class="ui-loading-bar" />
				</div>
			</Show>

			<header class="ui-topbar">
				<div class="ui-topbar-inner">
					<div class="ui-topbar-center">
						<div class="ui-floating ui-tabs">
							<A
								href={`/spaces/${props.spaceId}/dashboard`}
								class="ui-tab"
								classList={{ "ui-tab-active": props.activeTopTab === "dashboard" }}
							>
								dashboard
							</A>
							<A
								href={`/spaces/${props.spaceId}/search`}
								class="ui-tab"
								classList={{ "ui-tab-active": props.activeTopTab === "search" }}
							>
								search
							</A>
						</div>
					</div>
					<div class="ui-topbar-right">
						<ThemeMenu spaceId={props.spaceId} />
					</div>
				</div>
			</header>

			<div class={`ui-content ${props.showBottomTabs ? "ui-content-with-tabs" : ""}`}>
				{props.children}
			</div>

			{props.showBottomTabs && (
				<div class="ui-bottom-tabs">
					<div class="ui-floating ui-tabs">
						<A
							href={`/spaces/${props.spaceId}/entries${props.bottomTabHrefSuffix || ""}`}
							class="ui-tab ui-tab-secondary"
							classList={{ "ui-tab-active": props.activeBottomTab === "object" }}
						>
							object
						</A>
						<A
							href={`/spaces/${props.spaceId}/forms${props.bottomTabHrefSuffix || ""}`}
							class="ui-tab ui-tab-secondary"
							classList={{ "ui-tab-active": props.activeBottomTab === "grid" }}
						>
							grid
						</A>
					</div>
				</div>
			)}
		</main>
	);
}
