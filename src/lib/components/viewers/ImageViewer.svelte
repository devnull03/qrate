<script lang="ts">
	import { invoke, convertFileSrc } from "@tauri-apps/api/core";
	import { openPath } from "@tauri-apps/plugin-opener";
	import { untrack } from "svelte";
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

	type LoadState =
		| { status: "idle" }
		| { status: "loading"; path: string }
		| { status: "loaded"; path: string; data: string }
		| { status: "error"; path: string; message: string };

	let loadState = $state<LoadState>({ status: "idle" });

	const loading = $derived(loadState.status === "loading");
	const error = $derived(
		loadState.status === "error" ? loadState.message : null,
	);
	const imageData = $derived(
		loadState.status === "loaded" ? loadState.data : null,
	);

	const fetchFullImage = async (path: string): Promise<string> => {
		const result = await invoke<{ data: string; mime_type: string }>(
			"load_image",
			{ filePath: path },
		);
		return `data:${result.mime_type};base64,${result.data}`;
	};

	const fetchThumbnail = async (path: string): Promise<string | null> => {
		const thumbPath = await invoke<string | null>("get_thumbnail_path", {
			filePath: path,
		});
		return thumbPath ? convertFileSrc(thumbPath) : null;
	};

	const fetchImage = async (
		path: string,
		useThumbnail: boolean,
	): Promise<string> => {
		if (useThumbnail) {
			const thumbnailUrl = await fetchThumbnail(path);
			if (thumbnailUrl) return thumbnailUrl;
		}
		return fetchFullImage(path);
	};

	$effect(() => {
		const path = filePath;
		const useThumbnail = thumbnail;

		untrack(() => {
			if (!path) return;
			if (loadState.status === "loading" && loadState.path === path)
				return;
			if (loadState.status === "loaded" && loadState.path === path)
				return;

			loadState = { status: "loading", path };

			fetchImage(path, useThumbnail)
				.then((data) => {
					if (
						loadState.status === "loading" &&
						loadState.path === path
					) {
						loadState = { status: "loaded", path, data };
					}
				})
				.catch((err) => {
					if (
						loadState.status === "loading" &&
						loadState.path === path
					) {
						console.error("Failed to load image:", path, err);
						const message =
							err instanceof Error ? err.message : String(err);
						loadState = { status: "error", path, message };
					}
				});
		});
	});

	const openExternally = () => openPath(filePath);
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
