<script lang="ts">
	import { onMount } from "svelte";
	import { invoke } from "@tauri-apps/api/core";
	import { listen } from "@tauri-apps/api/event";
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

	onMount(() => {
		let unsubscribeRecent: (() => void) | null = null;
		let unsubscribeOpenFile: (() => void) | null = null;

		(async () => {
			await initGlobalSettings();
			await initRecentFiles();
			unsubscribeRecent = subscribeToRecentFiles((files) => {
				recentFiles = files;
			});

			unsubscribeOpenFile = await listen<string>("open-file", (event) => {
				handleOpenFilePath(event.payload);
			});
		})();

		return () => {
			unsubscribeRecent?.();
			unsubscribeOpenFile?.();
		};
	});

	async function handleOpenFilePath(path: string) {
		isProcessing = true;
		error = null;

		try {
			await qrateStore.openFile(path);
			await showMainWindow();
		} catch (err) {
			error = err instanceof Error ? err.message : String(err);
		} finally {
			isProcessing = false;
		}
	}

	async function showMainWindow() {
		await invoke("show_main_window").catch((err) => {
			error = err instanceof Error ? err.message : String(err);
		});
	}

	async function handleOpenQrate() {
		isProcessing = true;
		error = null;

		try {
			const selected = await open({
				multiple: false,
				filters: [{ name: "Qrate Files", extensions: ["qrate"] }],
			});

			if (selected && typeof selected === "string") {
				await qrateStore.openFile(selected);
				await showMainWindow();
			}
		} catch (err) {
			error = err instanceof Error ? err.message : String(err);
		} finally {
			isProcessing = false;
		}
	}

	async function handleOpenRecent(path: string) {
		isProcessing = true;
		error = null;

		try {
			await qrateStore.openFile(path);
			await showMainWindow();
		} catch (err) {
			error = err instanceof Error ? err.message : String(err);
			removeRecentFile(path);
		} finally {
			isProcessing = false;
		}
	}

	async function handleImportComplete() {
		showImportWizard = false;
		await showMainWindow();
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
			<ProjectsHeader />

			{#if error}
				<div
					class="rounded-md border border-destructive/50 bg-destructive/10 p-4 text-sm text-destructive"
				>
					{error}
				</div>
			{/if}

			{#if showImportWizard}
				<ImportWizard
					onComplete={handleImportComplete}
					onCancel={() => (showImportWizard = false)}
					onError={(err) => (error = err)}
				/>
			{:else}
				<ProjectActions
					{isProcessing}
					onNewProject={() => {
						showImportWizard = true;
						error = null;
					}}
					onOpenProject={handleOpenQrate}
				/>

				<RecentProjects
					{recentFiles}
					{isProcessing}
					onOpenRecent={handleOpenRecent}
					onRemoveRecent={removeRecentFile}
				/>
			{/if}

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
