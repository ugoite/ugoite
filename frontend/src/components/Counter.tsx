import { createSignal } from "solid-js";

export default function Counter() {
	const [count, setCount] = createSignal(0);
	return (
		<button
			type="button"
			class="ui-button ui-button-secondary w-[200px]"
			onClick={() => setCount(count() + 1)}
		>
			Clicks: {count()}
		</button>
	);
}
