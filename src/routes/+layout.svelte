<script lang="ts">
	import "../app.css";
	import { onMount } from "svelte";

	import TitleBar from "$lib/components/TitleBar.svelte";
	import StatusBar from "$lib/components/StatusBar.svelte";
	import AppSidebar from "$lib/components/app-sidebar.svelte";
	import * as Sidebar from "$lib/components/ui/sidebar/index.js";
	import { ModeWatcher } from "mode-watcher";
	import { qrateStore } from "$lib/stores/qrateStore.svelte";
	import { page } from "$app/state";
	import { getCurrentWindow } from "@tauri-apps/api/window";

	let { children } = $props();
	let isLoading = $state(true);
	let sidebarOpen = $state(true);

	let unlistenFocus: (() => void) | null = null;

	function toggleSidebar() {
		sidebarOpen = !sidebarOpen;
	}

	onMount(() => {
		// Initial sync from backend
		qrateStore.syncFromBackend().finally(() => {
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

{#if !page.route.id?.includes("projects") && !page.route.id?.includes("settings")}
	<div class="flex h-screen w-screen flex-col overflow-hidden">
		<!-- Custom titlebar with menus - always at top -->
		<TitleBar onToggleSidebar={toggleSidebar} {sidebarOpen} />

		<!-- Main content area with sidebar -->
		<div class="relative min-h-0 flex-1 overflow-hidden">
			{#if isLoading}
				<div
					class="flex h-full w-full items-center justify-center text-sm text-muted-foreground"
				>
					<span>Loading...</span>
				</div>
			{:else}
				<Sidebar.Provider
					bind:open={sidebarOpen}
					class="min-h-0! h-full"
				>
					<div class="relative flex h-full w-full overflow-hidden">
						<AppSidebar />
						<Sidebar.Inset
							class="flex min-h-0 flex-1 flex-col overflow-hidden"
						>
							<div
								class="flex min-h-0 flex-1 flex-col overflow-hidden"
							>
								{@render children()}
							</div>
						</Sidebar.Inset>
					</div>
				</Sidebar.Provider>
			{/if}
		</div>

		<!-- Status bar at bottom - always at bottom -->
		<StatusBar />
	</div>
{:else}
	{@render children()}
{/if}
