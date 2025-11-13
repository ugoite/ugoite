import { createSignal, onMount } from "solid-js";

export default function HelloWorld() {
  const [message, setMessage] = createSignal<string>('');

  onMount(async () => {
    try {
      const res = await fetch('http://localhost:8000/');
      const data = await res.json();
      setMessage(data.message ?? 'No message');
    } catch (e) {
      console.error(e);
      setMessage('Error fetching');
    }
  });

  return (
    <div class="mt-4">
      <h2 class="text-xl font-semibold">Backend says:</h2>
      <p class="text-gray-600">{message()}</p>
    </div>
  );
}
