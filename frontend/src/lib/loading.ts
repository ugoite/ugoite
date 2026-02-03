import { createMemo, createRoot, createSignal } from "solid-js";

const loadingStore = createRoot(() => {
	const [count, setCount] = createSignal(0);
	const isLoading = createMemo(() => count() > 0);

	const start = () => setCount((prev) => prev + 1);
	const stop = () => setCount((prev) => Math.max(0, prev - 1));

	return { count, isLoading, start, stop };
});

export const loadingState = {
	count: loadingStore.count,
	isLoading: loadingStore.isLoading,
	start: loadingStore.start,
	stop: loadingStore.stop,
};
