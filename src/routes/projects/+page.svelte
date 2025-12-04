<script lang="ts">
	import { onMount } from "svelte";
	import { invoke } from "@tauri-apps/api/core";
	import { open } from "@tauri-apps/plugin-dialog";
	import { qrateStore } from "$lib/stores/qrateStore.svelte";
	import {
		initRecentFiles,
		subscribeToRecentFiles,
		removeRecentFile,
		type RecentFile,
	} from "$lib/stores/recentFiles";
	import { initGlobalSettings } from "$lib/stores/globalSettings";
	import { ModeWatcher } from "mode-watcher";
	import SimpleTitleBar from "$lib/components/SimpleTitleBar.svelte";
	import ProjectsHeader from "$lib/components/projects/ProjectsHeader.svelte";
	import ProjectActions from "$lib/components/projects/ProjectActions.svelte";
	import RecentProjects from "$lib/components/projects/RecentProjects.svelte";
	import ImportWizard from "$lib/components/projects/ImportWizard.svelte";

	let isProcessing = $state(false);
	let error = $state<string | null>(null);
	let recentFiles = $state<RecentFile[]>([]);
	let showImportWizard = $state(false);

	// Initialize stores on mount
	onMount(() => {
		console.log("[Projects] Initializing...");

		let unsubscribe: (() => void) | null = null;

		// Initialize stores asynchronously
		(async () => {
			// Initialize global settings
			await initGlobalSettings();

			// Initialize and subscribe to recent files
			await initRecentFiles();
			unsubscribe = subscribeToRecentFiles((files) => {
				recentFiles = files;
			});
		})();

		return () => {
			if (unsubscribe) {
				unsubscribe();
			}
		};
	});

	/**
	 * After successfully loading a project, show the main window
	 */
	async function showMainWindow() {
		try {
			await invoke("show_main_window");
		} catch (err) {
			console.error("Failed to show main window:", err);
			error = err instanceof Error ? err.message : String(err);
		}
	}

	/**
	 * Open an existing .qrate file
	 */
	async function handleOpenQrate() {
		try {
			isProcessing = true;
			error = null;

			const selected = await open({
				multiple: false,
				filters: [
					{
						name: "Qrate Files",
						extensions: ["qrate"],
					},
				],
			});

			if (selected && typeof selected === "string") {
				await qrateStore.openFile(selected);
				await showMainWindow();
			}
		} catch (err) {
			console.error("Failed to open .qrate file:", err);
			error = err instanceof Error ? err.message : String(err);
		} finally {
			isProcessing = false;
		}
	}

	/**
	 * Start the import wizard
	 */
	function startImportWizard() {
		showImportWizard = true;
		error = null;
	}

	/**
	 * Cancel the import wizard
	 */
	function cancelImportWizard() {
		showImportWizard = false;
	}

	/**
	 * Handle import completion
	 */
	async function handleImportComplete() {
		showImportWizard = false;
		await showMainWindow();
	}

	/**
	 * Handle import/wizard error
	 */
	function handleError(err: string) {
		error = err;
	}

	/**
	 * Open a recent file
	 */
	async function handleOpenRecent(path: string) {
		try {
			isProcessing = true;
			error = null;

			await qrateStore.openFile(path);
			await showMainWindow();
		} catch (err) {
			console.error("Failed to open recent file:", err);
			error = err instanceof Error ? err.message : String(err);
			// Remove from recent files if it no longer exists
			removeRecentFile(path);
		} finally {
			isProcessing = false;
		}
	}

	/**
	 * Remove a file from recent list
	 */
	async function handleRemoveRecent(path: string) {
		await removeRecentFile(path);
	}
</script>

<ModeWatcher />

<div class="flex h-full w-screen flex-col bg-background">
	<SimpleTitleBar title="qRate - Projects" />
	<div
		class="flex flex-1 flex-col items-center overflow-auto p-8"
		class:justify-center={!showImportWizard}
	>
		<div class="w-full max-w-2xl space-y-8">
			<!-- Header -->
			<ProjectsHeader />

			<!-- Error Display -->
			{#if error}
				<div
					class="rounded-md border border-destructive/50 bg-destructive/10 p-4 text-sm text-destructive"
				>
					{error}
				</div>
			{/if}

			{#if showImportWizard}
				<!-- Import Wizard -->
				<ImportWizard
					onComplete={handleImportComplete}
					onCancel={cancelImportWizard}
					onError={handleError}
				/>
			{:else}
				<!-- Actions -->
				<ProjectActions
					{isProcessing}
					onNewProject={startImportWizard}
					onOpenProject={handleOpenQrate}
				/>

				<!-- Recent Files -->
				<RecentProjects
					{recentFiles}
					{isProcessing}
					onOpenRecent={handleOpenRecent}
					onRemoveRecent={handleRemoveRecent}
				/>
			{/if}

			<!-- Loading Overlay -->
			{#if isProcessing}
				<div
					class="fixed inset-0 flex items-center justify-center bg-background/80 backdrop-blur-sm"
				>
					<div class="flex items-center gap-3 text-muted-foreground">
						<div
							class="size-5 animate-spin rounded-full border-2 border-current border-t-transparent"
						></div>
						<span>Loading...</span>
					</div>
				</div>
			{/if}
		</div>
	</div>
</div>
