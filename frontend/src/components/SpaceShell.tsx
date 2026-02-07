import { A } from "@solidjs/router";
import type { JSX } from "solid-js";
import { Show, createEffect, createSignal, onMount } from "solid-js";
import { loadingState } from "~/lib/loading";
import { applyTheme, persistTheme, resolveTheme, type UiTheme, type UiTone } from "~/lib/theme";

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
	"px-4 py-2 text-sm font-medium rounded-full transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-offset-2 focus-visible:ring-sky-500 focus-visible:ring-offset-slate-50 dark:focus-visible:ring-offset-slate-900";

export function SpaceShell(props: SpaceShellProps) {
	const [menuOpen, setMenuOpen] = createSignal(false);
	const [theme, setTheme] = createSignal<UiTheme>("materialize");
	const [tone, setTone] = createSignal<UiTone>("light");

	onMount(() => {
		const resolved = resolveTheme();
		setTheme(resolved.theme);
		setTone(resolved.tone);
		applyTheme(resolved.theme, resolved.tone);
	});

	createEffect(() => {
		applyTheme(theme(), tone());
		persistTheme(theme(), tone());
	});

	const topTabForm = (tab: SpaceTopTab) =>
		props.activeTopTab === tab
			? `${tabBaseForms} bg-sky-600 text-white shadow`
			: `${tabBaseForms} text-slate-700 hover:bg-slate-100`;

	const bottomTabForm = (tab: SpaceBottomTab) =>
		props.activeBottomTab === tab
			? `${tabBaseForms} bg-accent-strong text-white shadow`
			: `${tabBaseForms} text-slate-700 hover:bg-slate-100`;

	const closeMenu = () => setMenuOpen(false);

	return (
		<main class="min-h-screen bg-slate-50 text-slate-900 relative">
			<Show when={loadingState.isLoading()}>
				<div class="fixed top-0 left-0 right-0 z-50">
					<div class="h-1 w-full bg-sky-500 animate-pulse" />
				</div>
			</Show>
			<div class="fixed top-3 sm:top-5 left-1/2 -translate-x-1/2 z-50 max-w-[92vw]">
				<div class="flex flex-wrap items-center gap-2 rounded-full bg-slate-50/90 shadow-lg ring-1 ring-slate-200 px-2 py-2 backdrop-blur">
					<A href={`/spaces/${props.spaceId}/dashboard`} class={topTabForm("dashboard")}>
						dashboard
					</A>
					<A href={`/spaces/${props.spaceId}/search`} class={topTabForm("search")}>
						search
					</A>
				</div>
			</div>

			<div class="fixed top-3 sm:top-5 right-3 sm:right-5 z-50">
				<div class="relative">
					<button
						type="button"
						class="rounded-full bg-slate-50/90 p-3 shadow-lg ring-1 ring-slate-200 backdrop-blur hover:bg-slate-50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sky-500"
						aria-label="Space settings"
						aria-haspopup="menu"
						aria-expanded={menuOpen()}
						onClick={() => setMenuOpen((open) => !open)}
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
					</button>

					<Show when={menuOpen()}>
						<div class="absolute right-0 mt-3 w-72 max-w-[85vw] rounded-2xl border border-slate-200 bg-slate-50/95 p-4 shadow-xl backdrop-blur">
							<div class="flex items-center justify-between">
								<h3 class="text-sm font-semibold text-slate-700">UI Theme</h3>
								<button
									type="button"
									class="text-xs text-slate-500 hover:text-slate-700"
									onClick={closeMenu}
								>
									Close
								</button>
							</div>
							<div class="mt-3 space-y-4">
								<fieldset class="space-y-2">
									<legend class="text-xs font-semibold uppercase tracking-wide text-slate-500">
										Theme
									</legend>
									<div class="space-y-2">
										<label class="flex items-center gap-2 text-sm text-slate-700">
											<input
												type="radio"
												name="ui-theme"
												value="materialize"
												checked={theme() === "materialize"}
												onChange={() => {
													setTheme("materialize");
													closeMenu();
												}}
											/>
											Materialize
										</label>
										<label class="flex items-center gap-2 text-sm text-slate-700">
											<input
												type="radio"
												name="ui-theme"
												value="classic"
												checked={theme() === "classic"}
												onChange={() => {
													setTheme("classic");
													closeMenu();
												}}
											/>
											Classic
										</label>
										<label class="flex items-center gap-2 text-sm text-slate-700">
											<input
												type="radio"
												name="ui-theme"
												value="pop"
												checked={theme() === "pop"}
												onChange={() => {
													setTheme("pop");
													closeMenu();
												}}
											/>
											Pop
										</label>
									</div>
								</fieldset>
								<fieldset class="space-y-2">
									<legend class="text-xs font-semibold uppercase tracking-wide text-slate-500">
										Tone
									</legend>
									<div class="space-y-2">
										<label class="flex items-center gap-2 text-sm text-slate-700">
											<input
												type="radio"
												name="ui-tone"
												value="light"
												checked={tone() === "light"}
												onChange={() => {
													setTone("light");
													closeMenu();
												}}
											/>
											Light
										</label>
										<label class="flex items-center gap-2 text-sm text-slate-700">
											<input
												type="radio"
												name="ui-tone"
												value="dark"
												checked={tone() === "dark"}
												onChange={() => {
													setTone("dark");
													closeMenu();
												}}
											/>
											Dark
										</label>
									</div>
								</fieldset>
								<div>
									<A
										href={`/spaces/${props.spaceId}/settings`}
										class="text-xs font-medium text-slate-500 hover:text-slate-700"
										onClick={closeMenu}
									>
										Open space settings
									</A>
								</div>
							</div>
						</div>
					</Show>
				</div>
			</div>

			<div
				class={`px-4 sm:px-6 ${props.showBottomTabs ? "pb-24 sm:pb-24" : "pb-12"} pt-20 sm:pt-24`}
			>
				{props.children}
			</div>

			{props.showBottomTabs && (
				<div class="fixed bottom-4 sm:bottom-6 left-1/2 -translate-x-1/2 z-50 max-w-[92vw]">
					<div class="flex flex-wrap items-center gap-2 rounded-full bg-slate-50/90 shadow-lg ring-1 ring-slate-200 px-2 py-2 backdrop-blur">
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
