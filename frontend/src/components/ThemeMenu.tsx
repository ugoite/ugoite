import { A } from "@solidjs/router";
import { For, Show, createSignal, onCleanup, onMount } from "solid-js";
import { isServer } from "solid-js/web";
import { colorMode, setColorMode, setUiTheme, uiTheme } from "~/lib/ui-theme";
import { UI_THEMES } from "~/themes/registry";
import type { ColorMode, UiTheme } from "~/lib/ui-theme";

const themes: { value: UiTheme; label: string }[] = UI_THEMES.map((theme) => ({
	value: theme.id,
	label: theme.label,
}));

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
				aria-label="Theme settings"
				aria-haspopup="menu"
				aria-expanded={open()}
				onClick={() => setOpen((value) => !value)}
			>
				<span class="ui-sr-only">Theme settings</span>
				<svg class="h-5 w-5" viewBox="0 0 24 24" fill="none" stroke="currentColor">
					<path
						stroke-linecap="round"
						stroke-linejoin="round"
						stroke-width="2"
						d="M10.75 3.75h2.5l.38 2.27a7.5 7.5 0 0 1 1.95.8l1.92-1.32 1.77 1.77-1.32 1.92c.36.62.62 1.27.8 1.95l2.27.38v2.5l-2.27.38a7.5 7.5 0 0 1-.8 1.95l1.32 1.92-1.77 1.77-1.92-1.32a7.5 7.5 0 0 1-1.95.8l-.38 2.27h-2.5l-.38-2.27a7.5 7.5 0 0 1-1.95-.8l-1.92 1.32-1.77-1.77 1.32-1.92a7.5 7.5 0 0 1-.8-1.95l-2.27-.38v-2.5l2.27-.38a7.5 7.5 0 0 1 .8-1.95L5.66 6.57l1.77-1.77 1.92 1.32a7.5 7.5 0 0 1 1.95-.8l.45-2.57zM12 9a3 3 0 1 0 0 6 3 3 0 0 0 0-6z"
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
										<span>{theme.label}</span>
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
