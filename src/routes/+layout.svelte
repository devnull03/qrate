<script lang="ts">
	import "../app.css";
	import { onMount } from "svelte";

	import AppSidebar from "$lib/components/app-sidebar.svelte";
	import Header from "$lib/components/Header.svelte";
	import * as Sidebar from "$lib/components/ui/sidebar/index.js";

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

{#if page.route.id !== "/projects"}
	<div class="flex h-screen w-screen overflow-hidden">
		<Sidebar.Provider open={false} style="--sidebar-width: 350px;">
			<AppSidebar />
			<Sidebar.Inset
				class="flex h-screen w-full flex-col overflow-hidden"
			>
				<Header />
				<ModeWatcher />
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
			</Sidebar.Inset>
		</Sidebar.Provider>
	</div>
{:else}
	{@render children()}
{/if}
