import { createSignal, onMount } from "solid-js";
import { apiFetch } from "../lib/api";

export default function HelloWorld() {
	const [message, setMessage] = createSignal<string>("");

	onMount(async () => {
		try {
			const res = await apiFetch("/");
			const data = await res.json();
			setMessage(data.message ?? "No message");
		} catch (_e) {
			setMessage("Error fetching");
		}
	});

	return (
		<div class="mt-4">
			<h2 class="text-xl font-semibold">Backend says:</h2>
			<p class="text-gray-600">{message()}</p>
		</div>
	);
}
