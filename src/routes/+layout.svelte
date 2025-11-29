<script lang="ts">
	import "../app.css";
	import { onMount } from "svelte";

	import TitleBar from "$lib/components/TitleBar.svelte";
	import StatusBar from "$lib/components/StatusBar.svelte";
	import { ModeWatcher } from "mode-watcher";
	import { qrateStore } from "$lib/stores/qrateStore.svelte";
	import { page } from "$app/state";

	let { children } = $props();
	let isRestoring = $state(true);

	onMount(async () => {
		try {
			await qrateStore.restoreWorkspace();
		} catch (err) {
			console.warn("Failed to restore workspace:", err);
		} finally {
			isRestoring = false;
		}
	});
</script>

<ModeWatcher />

{#if !page.route.id?.includes("projects")}
	<div class="flex h-screen w-screen flex-col overflow-hidden">
		<!-- Custom titlebar with menus -->
		<TitleBar />
		<!-- Main content area -->
		<div class="relative min-h-0 flex-1 overflow-hidden">
			{#if isRestoring}
				<div
					class="flex h-full w-full items-center justify-center text-sm text-muted-foreground"
				>
					<span>Restoring workspace...</span>
				</div>
			{:else}
				{@render children()}
			{/if}
		</div>
		<!-- Status bar at bottom -->
		<StatusBar />
	</div>
{:else}
	{@render children()}
{/if}
