<script lang="ts">
	import "../app.css";
	import { onMount } from "svelte";

	import WorkbenchLayout from "$lib/components/layout/WorkbenchLayout.svelte";
	import { ModeWatcher } from "mode-watcher";
	import { qrateStore } from "$lib/stores/qrateStore.svelte";
	import { initGlobalSettings } from "$lib/stores/globalSettings";
	import { page } from "$app/state";
	import { getCurrentWindow } from "@tauri-apps/api/window";

	let { children } = $props();
	let isLoading = $state(true);

	let unlistenFocus: (() => void) | null = null;

	onMount(() => {
		// Initialize global settings and sync from backend
		Promise.all([
			initGlobalSettings(),
			qrateStore.syncFromBackend(),
		]).finally(() => {
			isLoading = false;
		});

		// Listen for window focus to sync state from backend
		// This handles when the main window is shown after projects window opens a file
		const currentWindow = getCurrentWindow();
		currentWindow
			.onFocusChanged(async ({ payload: focused }) => {
				if (focused && !qrateStore.isFileOpen) {
					await qrateStore.syncFromBackend();
				}
			})
			.then((unlisten) => {
				unlistenFocus = unlisten;
			});

		return () => {
			if (unlistenFocus) {
				unlistenFocus();
			}
		};
	});
</script>

<ModeWatcher />

{#if !page.route.id?.includes("projects") && !page.route.id?.includes("settings") && !page.route.id?.includes("chat")}
	{#if isLoading}
		<div
			class="flex h-screen w-screen items-center justify-center text-sm text-muted-foreground"
		>
			<span>Loading...</span>
		</div>
	{:else}
		<WorkbenchLayout>
			{@render children()}
		</WorkbenchLayout>
	{/if}
{:else}
	{@render children()}
{/if}
