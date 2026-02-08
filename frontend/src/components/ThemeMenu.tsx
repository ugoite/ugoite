import { A } from "@solidjs/router";
import { For, Show, createSignal, onCleanup, onMount } from "solid-js";
import { isServer } from "solid-js/web";
import { colorMode, setColorMode, setUiTheme, uiTheme } from "~/lib/ui-theme";
import type { ColorMode, UiTheme } from "~/lib/ui-theme";

const themes: { value: UiTheme; label: string; description: string }[] = [
	{ value: "materialize", label: "materialize", description: "最新マテリアル" },
	{ value: "classic", label: "classic", description: "シンプル" },
	{ value: "pop", label: "pop", description: "躍動感" },
];

const modes: { value: ColorMode; label: string }[] = [
	{ value: "light", label: "light" },
	{ value: "dark", label: "dark" },
];

interface ThemeMenuProps {
	spaceId: string;
}

export function ThemeMenu(props: ThemeMenuProps) {
	const [open, setOpen] = createSignal(false);
	let menuRef: HTMLDivElement | undefined;

	const handleDocumentPointer = (event: PointerEvent) => {
		if (!menuRef || menuRef.contains(event.target as Node)) return;
		setOpen(false);
	};

	const handleDocumentKeydown = (event: KeyboardEvent) => {
		if (event.key === "Escape") {
			setOpen(false);
		}
	};

	onMount(() => {
		if (isServer || typeof document === "undefined") return;
		document.addEventListener("pointerdown", handleDocumentPointer);
		document.addEventListener("keydown", handleDocumentKeydown);
	});

	onCleanup(() => {
		if (isServer || typeof document === "undefined") return;
		document.removeEventListener("pointerdown", handleDocumentPointer);
		document.removeEventListener("keydown", handleDocumentKeydown);
	});

	return (
		<div
			class="ui-menu"
			ref={(el) => {
				menuRef = el;
			}}
		>
			<button
				type="button"
				class="ui-icon-button"
				aria-haspopup="menu"
				aria-expanded={open()}
				onClick={() => setOpen((value) => !value)}
			>
				<svg class="h-5 w-5" viewBox="0 0 24 24" fill="none" stroke="currentColor">
					<path
						stroke-linecap="round"
						stroke-linejoin="round"
						stroke-width="2"
						d="M12 6.75a5.25 5.25 0 100 10.5 5.25 5.25 0 000-10.5zm0-4.5v2.25m0 15v2.25m9.75-9h-2.25M4.5 12H2.25m16.22-6.72l-1.59 1.59M7.37 16.63l-1.59 1.59m0-11.28l1.59 1.59m11.28 11.28l1.59 1.59"
					/>
				</svg>
			</button>
			<Show when={open()}>
				<div class="ui-menu-panel" role="menu">
					<div class="ui-menu-section">
						<p class="ui-menu-title">UI Theme</p>
						<div class="ui-menu-options">
							<For each={themes}>
								{(theme) => (
									<label class="ui-radio">
										<input
											type="radio"
											name="ui-theme"
											value={theme.value}
											checked={uiTheme() === theme.value}
											onChange={() => setUiTheme(theme.value)}
										/>
										<span>
											{theme.label}
											<span class="text-xs ui-muted ml-2">{theme.description}</span>
										</span>
									</label>
								)}
							</For>
						</div>
					</div>
					<div class="ui-menu-section">
						<p class="ui-menu-title">Color Mode</p>
						<div class="ui-menu-options">
							<For each={modes}>
								{(mode) => (
									<label class="ui-radio">
										<input
											type="radio"
											name="color-mode"
											value={mode.value}
											checked={colorMode() === mode.value}
											onChange={() => setColorMode(mode.value)}
										/>
										<span>{mode.label}</span>
									</label>
								)}
							</For>
						</div>
					</div>
					<A class="ui-menu-link" href={`/spaces/${props.spaceId}/settings`}>
						Space settings
					</A>
				</div>
			</Show>
		</div>
	);
}
