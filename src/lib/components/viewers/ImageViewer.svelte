<script lang="ts">
	import { invoke } from "@tauri-apps/api/core";
	import { openPath } from "@tauri-apps/plugin-opener";
	import { Button } from "$lib/components/ui/button/index.js";
	import ExternalLinkIcon from "@lucide/svelte/icons/external-link";
	import ImageIcon from "@lucide/svelte/icons/image";
	import AlertCircleIcon from "@lucide/svelte/icons/alert-circle";
	import LoaderIcon from "@lucide/svelte/icons/loader";

	interface Props {
		filePath: string;
		alt?: string;
		thumbnail?: boolean;
		showOpenButton?: boolean;
		class?: string;
	}

	let {
		filePath,
		alt = "Image",
		thumbnail = false,
		showOpenButton = !thumbnail,
		class: className = "",
	}: Props = $props();

	// For Rust-side resizing, use reasonable max values
	const maxWidth = thumbnail ? 150 : 1200;
	const maxHeight = thumbnail ? 150 : 900;

	let imageData = $state<string | null>(null);
	let loading = $state(true);
	let error = $state<string | null>(null);
	let lastLoadedPath = $state<string | null>(null);

	async function loadImage(path: string) {
		if (!path) {
			error = "No file path provided";
			loading = false;
			return;
		}

		loading = true;
		error = null;
		imageData = null;

		try {
			const result = await invoke<{ data: string; mime_type: string }>(
				"load_image",
				{
					filePath: path,
					maxWidth,
					maxHeight,
				},
			);
			imageData = `data:${result.mime_type};base64,${result.data}`;
			lastLoadedPath = path;
		} catch (err) {
			console.error("Failed to load image:", path, err);
			error = err instanceof Error ? err.message : String(err);
		} finally {
			loading = false;
		}
	}

	async function openExternally() {
		try {
			await openPath(filePath);
		} catch (err) {
			console.error("Failed to open file:", err);
		}
	}

	// Load image when filePath changes
	$effect(() => {
		if (filePath && filePath !== lastLoadedPath) {
			loadImage(filePath);
		}
	});
</script>

<div
	class="image-viewer group flex flex-col bg-muted/50 {className} relative"
	class:thumbnail
>
	{#if loading}
		<div
			class="flex flex-col items-center justify-center gap-2 p-4 text-muted-foreground"
		>
			<LoaderIcon class="size-6 animate-spin" />
			{#if !thumbnail}
				<span class="text-xs">Loading...</span>
			{/if}
		</div>
	{:else if error}
		<div
			class="flex flex-col items-center justify-center gap-2 p-4 text-muted-foreground"
		>
			<AlertCircleIcon class="size-6 text-destructive" />
			{#if !thumbnail}
				<p class="max-w-full wrap-break-word text-xs text-center px-2">
					{error}
				</p>
			{/if}
		</div>
	{:else if imageData}
		{#if showOpenButton}
			<div
				class="absolute right-2 top-2 opacity-0 transition-opacity group-hover:opacity-100"
			>
				<Button
					variant="secondary"
					size="icon-sm"
					class="size-8 shadow-md"
					onclick={openExternally}
					title="Open externally"
				>
					<ExternalLinkIcon class="size-4" />
				</Button>
			</div>
		{/if}
		<div
			class="flex min-h-0 flex-1 items-center justify-center overflow-hidden"
		>
			<img
				src={imageData}
				{alt}
				class="max-h-full max-w-full object-contain!"
			/>
		</div>
	{:else}
		<div
			class="flex flex-col items-center justify-center gap-2 p-4 text-muted-foreground"
		>
			<ImageIcon class="size-6" />
			{#if !thumbnail}
				<span class="text-xs">No image</span>
			{/if}
		</div>
	{/if}
</div>

<style>
	.image-viewer {
		min-height: 60px;
	}

	.image-viewer.thumbnail {
		min-height: 40px;
		min-width: 40px;
	}
</style>
