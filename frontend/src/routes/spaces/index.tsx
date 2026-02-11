import { A, useNavigate } from "@solidjs/router";
import { createEffect, createResource, createSignal, For, Show } from "solid-js";
import { spaceApi } from "~/lib/space-api";
import type { SampleSpaceJob } from "~/lib/types";

export default function SpacesIndexRoute() {
	const navigate = useNavigate();
	const [newSpaceName, setNewSpaceName] = createSignal("");
	const [createError, setCreateError] = createSignal<string | null>(null);
	const [isCreating, setIsCreating] = createSignal(false);
	const [sampleSpaceName, setSampleSpaceName] = createSignal("");
	const [sampleScenario, setSampleScenario] = createSignal("renewable-ops");
	const [sampleEntryCount, setSampleEntryCount] = createSignal("5000");
	const [sampleSeed, setSampleSeed] = createSignal("");
	const [sampleError, setSampleError] = createSignal<string | null>(null);
	const [isGeneratingSample, setIsGeneratingSample] = createSignal(false);
	const [sampleJob, setSampleJob] = createSignal<SampleSpaceJob | null>(null);

	const buildSamplePayload = () => {
		const name = sampleSpaceName().trim();
		if (!name) {
			return { error: "Space name is required." };
		}
		const entryCountValue = Number(sampleEntryCount());
		if (!Number.isFinite(entryCountValue) || entryCountValue < 100) {
			return { error: "Entry count must be at least 100." };
		}
		const seedValue = sampleSeed().trim();
		const seedNumber = seedValue ? Number(seedValue) : undefined;
		if (seedValue && (!Number.isFinite(seedNumber) || seedNumber < 0)) {
			return { error: "Seed must be a non-negative number." };
		}
		return {
			payload: {
				space_id: name,
				scenario: sampleScenario(),
				entry_count: entryCountValue,
				seed: seedNumber,
			},
		};
	};

	const [spaces, { refetch }] = createResource(async () => {
		return await spaceApi.list();
	});

	const [scenarios] = createResource(async () => {
		return await spaceApi.listSampleScenarios();
	});

	createEffect(() => {
		const list = scenarios();
		if (!list || list.length === 0) return;
		if (!list.some((scenario) => scenario.id === sampleScenario())) {
			setSampleScenario(list[0].id);
		}
	});

	const handleCreate = async () => {
		const name = newSpaceName().trim();
		if (!name) return;
		setIsCreating(true);
		setCreateError(null);
		try {
			const created = await spaceApi.create(name);
			await refetch();
			setNewSpaceName("");
			navigate(`/spaces/${created.id}/dashboard`);
		} catch (err) {
			setCreateError(err instanceof Error ? err.message : "Failed to create space");
		} finally {
			setIsCreating(false);
		}
	};

	const pollSampleJob = async (jobId: string) => {
		const pollIntervalMs = 1200;
		while (true) {
			const latest = await spaceApi.getSampleSpaceJob(jobId);
			setSampleJob(latest);
			if (latest.status === "completed") {
				if (latest.summary) {
					await refetch();
					setSampleSpaceName("");
					navigate(`/spaces/${latest.summary.space_id}/dashboard`);
				}
				return;
			}
			if (latest.status === "failed") {
				throw new Error(latest.error || "Sample data generation failed.");
			}
			await new Promise((resolve) => setTimeout(resolve, pollIntervalMs));
		}
	};

	const handleCreateSample = async () => {
		const result = buildSamplePayload();
		if (!result.payload) {
			setSampleError(result.error ?? "Invalid sample data settings.");
			return;
		}

		setSampleError(null);
		setIsGeneratingSample(true);
		setSampleJob(null);
		try {
			const job = await spaceApi.createSampleSpaceJob(result.payload);
			setSampleJob(job);
			await pollSampleJob(job.job_id);
		} catch (err) {
			setSampleError(err instanceof Error ? err.message : "Failed to create sample space");
		} finally {
			setIsGeneratingSample(false);
		}
	};

	return (
		<main class="mx-auto max-w-4xl ui-page ui-stack">
			<div class="flex flex-wrap items-center justify-between gap-3">
				<h1 class="ui-page-title">Spaces</h1>
				<A href="/" class="ui-muted text-sm">
					Back to Home
				</A>
			</div>

			<section class="ui-card">
				<h2 class="text-lg font-semibold mb-2">Create Space</h2>
				<div class="flex flex-col gap-3 sm:flex-row sm:items-center">
					<input
						type="text"
						class="ui-input flex-1"
						placeholder="Space name"
						value={newSpaceName()}
						onInput={(e) => setNewSpaceName(e.currentTarget.value)}
						onKeyDown={(e) => {
							if (e.key === "Enter") handleCreate();
						}}
					/>
					<button
						type="button"
						class="ui-button ui-button-primary"
						disabled={isCreating()}
						onClick={handleCreate}
					>
						{isCreating() ? "Creating..." : "Create"}
					</button>
				</div>
				<Show when={createError()}>
					<p class="ui-alert ui-alert-error text-sm mt-2">{createError()}</p>
				</Show>
			</section>

			<section class="ui-card">
				<h2 class="text-lg font-semibold mb-2">Create Sample Space</h2>
				<p class="text-sm ui-muted mb-3">
					Generate a neutral operations dataset (no personal data) with multiple forms and ~5,000
					entries to explore the app.
				</p>
				<div class="grid gap-3 sm:grid-cols-2">
					<input
						type="text"
						class="ui-input"
						placeholder="Sample space name"
						value={sampleSpaceName()}
						onInput={(e) => setSampleSpaceName(e.currentTarget.value)}
					/>
					<select
						class="ui-input"
						value={sampleScenario()}
						onChange={(e) => setSampleScenario(e.currentTarget.value)}
					>
						<For each={scenarios() || []}>
							{(scenario) => <option value={scenario.id}>{scenario.label}</option>}
						</For>
					</select>
					<input
						type="number"
						class="ui-input"
						min="100"
						placeholder="Entry count"
						value={sampleEntryCount()}
						onInput={(e) => setSampleEntryCount(e.currentTarget.value)}
					/>
					<input
						type="number"
						class="ui-input"
						min="0"
						placeholder="Seed (optional)"
						value={sampleSeed()}
						onInput={(e) => setSampleSeed(e.currentTarget.value)}
					/>
				</div>
				<div class="flex flex-wrap items-center gap-2 mt-3">
					<button
						type="button"
						class="ui-button ui-button-primary"
						disabled={isGeneratingSample()}
						onClick={handleCreateSample}
					>
						{isGeneratingSample() ? "Generating..." : "Generate Sample Space"}
					</button>
					<Show when={sampleError()}>
						<p class="ui-alert ui-alert-error text-sm">{sampleError()}</p>
					</Show>
				</div>
				<Show when={sampleJob()}>
					<div class="mt-3 text-sm ui-muted">
						<div>Status: {sampleJob()?.status_message || sampleJob()?.status}</div>
						<div>
							Progress: {sampleJob()?.processed_entries} / {sampleJob()?.total_entries}
						</div>
					</div>
				</Show>
			</section>

			<section class="ui-card">
				<h2 class="text-lg font-semibold mb-3">Available Spaces</h2>
				<Show when={spaces.loading}>
					<p class="text-sm ui-muted">Loading spaces...</p>
				</Show>
				<Show when={spaces.error}>
					<p class="ui-alert ui-alert-error text-sm">Failed to load spaces.</p>
				</Show>
				<Show when={spaces() && spaces()?.length === 0}>
					<p class="text-sm ui-muted">No spaces yet. Create one above.</p>
				</Show>
				<ul class="ui-stack-sm">
					<For each={spaces() || []}>
						{(space) => (
							<li class="ui-card flex flex-col sm:flex-row sm:items-center sm:justify-between gap-3">
								<div>
									<h3 class="font-medium">{space.name || space.id}</h3>
									<p class="text-xs ui-muted">ID: {space.id}</p>
								</div>
								<div class="flex flex-wrap gap-2">
									<A
										href={`/spaces/${space.id}/settings`}
										class="ui-button ui-button-secondary text-sm"
									>
										Settings
									</A>
									<A
										href={`/spaces/${space.id}/dashboard`}
										class="ui-button ui-button-primary text-sm"
									>
										Open Space
									</A>
								</div>
							</li>
						)}
					</For>
				</ul>
			</section>
		</main>
	);
}
