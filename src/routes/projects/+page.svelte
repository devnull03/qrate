<script lang="ts">
	import { invoke } from "@tauri-apps/api/core";
	import { open, save } from "@tauri-apps/plugin-dialog";
	import { qrateStore } from "$lib/stores/qrateStore.svelte";
	import {
		recentFilesStore,
		removeRecentFile,
	} from "$lib/stores/recentFiles";
	import { Button } from "$lib/components/ui/button/index.js";
	import * as Card from "$lib/components/ui/card/index.js";
	import CommandIcon from "@lucide/svelte/icons/command";
	import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
	import FilePlusIcon from "@lucide/svelte/icons/file-plus";
	import UploadIcon from "@lucide/svelte/icons/upload";
	import FileIcon from "@lucide/svelte/icons/file";
	import ClockIcon from "@lucide/svelte/icons/clock";
	import XIcon from "@lucide/svelte/icons/x";
	import { ModeWatcher } from "mode-watcher";
	import ModeToggle from "$lib/components/ModeToggle.svelte";
	import SimpleTitleBar from "$lib/components/SimpleTitleBar.svelte";

	let isProcessing = $state(false);
	let error = $state<string | null>(null);

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
	 * Create a new .qrate file
	 */
	async function handleCreateQrate() {
		try {
			isProcessing = true;
			error = null;

			const selected = await save({
				filters: [
					{
						name: "Qrate Files",
						extensions: ["qrate"],
					},
				],
				defaultPath: "untitled.qrate",
			});

			if (selected) {
				await qrateStore.createFile(selected);
				await showMainWindow();
			}
		} catch (err) {
			console.error("Failed to create .qrate file:", err);
			error = err instanceof Error ? err.message : String(err);
		} finally {
			isProcessing = false;
		}
	}

	/**
	 * Import a CSV file into a new .qrate file
	 */
	async function handleImportCsv() {
		try {
			isProcessing = true;
			error = null;

			const csvFile = await open({
				multiple: false,
				filters: [
					{
						name: "CSV Files",
						extensions: ["csv"],
					},
				],
			});

			if (!csvFile || typeof csvFile !== "string") {
				isProcessing = false;
				return;
			}

			const qrateFile = await save({
				filters: [
					{
						name: "Qrate Files",
						extensions: ["qrate"],
					},
				],
				defaultPath: csvFile.replace(/\.csv$/i, ".qrate"),
			});

			if (!qrateFile) {
				isProcessing = false;
				return;
			}

			await qrateStore.importCsv(qrateFile, csvFile);
			await showMainWindow();
		} catch (err) {
			console.error("Failed to import CSV:", err);
			error = err instanceof Error ? err.message : String(err);
		} finally {
			isProcessing = false;
		}
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
	 * Format a timestamp as a relative time string
	 */
	function formatRelativeTime(timestamp: number): string {
		const now = Date.now();
		const diff = now - timestamp;
		const seconds = Math.floor(diff / 1000);
		const minutes = Math.floor(seconds / 60);
		const hours = Math.floor(minutes / 60);
		const days = Math.floor(hours / 24);

		if (days > 0) return `${days} day${days > 1 ? "s" : ""} ago`;
		if (hours > 0) return `${hours} hour${hours > 1 ? "s" : ""} ago`;
		if (minutes > 0)
			return `${minutes} minute${minutes > 1 ? "s" : ""} ago`;
		return "Just now";
	}
</script>

<ModeWatcher />

<div class="flex h-screen w-screen flex-col bg-background">
	<SimpleTitleBar title="qRate - Projects" />
	<div
		class="flex flex-1 flex-col items-center justify-center overflow-auto p-8"
	>
		<div class="w-full max-w-2xl space-y-8">
			<!-- Header -->
			<div class="flex items-center justify-between">
				<div class="flex items-center gap-4">
					<div
						class="flex size-12 items-center justify-center rounded-xl bg-primary text-primary-foreground"
					>
						<CommandIcon class="size-6" />
					</div>
					<div>
						<h1 class="text-2xl font-bold">qRate</h1>
						<p class="text-sm text-muted-foreground">
							Digital Archival Workspace
						</p>
					</div>
				</div>
				<ModeToggle />
			</div>

			<!-- Error Display -->
			{#if error}
				<div
					class="rounded-md border border-destructive/50 bg-destructive/10 p-4 text-sm text-destructive"
				>
					{error}
				</div>
			{/if}

			<!-- Actions -->
			<Card.Root>
				<Card.Header>
					<Card.Title>Get Started</Card.Title>
					<Card.Description
						>Create a new project or open an existing one</Card.Description
					>
				</Card.Header>
				<Card.Content>
					<div class="grid grid-cols-3 gap-4">
						<Button
							onclick={handleCreateQrate}
							disabled={isProcessing}
							variant="outline"
							class="flex h-24 flex-col items-center justify-center gap-2"
						>
							<FilePlusIcon class="size-6" />
							<span>New Project</span>
						</Button>

						<Button
							onclick={handleOpenQrate}
							disabled={isProcessing}
							variant="outline"
							class="flex h-24 flex-col items-center justify-center gap-2"
						>
							<FolderOpenIcon class="size-6" />
							<span>Open Project</span>
						</Button>

						<Button
							onclick={handleImportCsv}
							disabled={isProcessing}
							variant="outline"
							class="flex h-24 flex-col items-center justify-center gap-2"
						>
							<UploadIcon class="size-6" />
							<span>Import CSV</span>
						</Button>
					</div>
				</Card.Content>
			</Card.Root>

			<!-- Recent Files -->
			{#if $recentFilesStore.files.length > 0}
				<Card.Root>
					<Card.Header>
						<Card.Title class="flex items-center gap-2">
							<ClockIcon class="size-4" />
							Recent Projects
						</Card.Title>
					</Card.Header>
					<Card.Content>
						<div class="space-y-2">
							{#each $recentFilesStore.files as file (file.path)}
								<div
									class="group flex w-full items-center gap-3 rounded-md p-3 text-left transition-colors hover:bg-muted"
								>
									<button
										onclick={() =>
											handleOpenRecent(file.path)}
										disabled={isProcessing}
										class="flex min-w-0 flex-1 items-center gap-3 disabled:opacity-50"
									>
										<FileIcon
											class="size-5 shrink-0 text-muted-foreground"
										/>
										<div class="min-w-0 flex-1">
											<p
												class="truncate font-medium text-left"
											>
												{file.name}
											</p>
											<p
												class="truncate text-xs text-muted-foreground text-left"
												title={file.path}
											>
												{file.path}
											</p>
										</div>
										<span
											class="shrink-0 text-xs text-muted-foreground"
										>
											{formatRelativeTime(
												file.lastOpened,
											)}
										</span>
									</button>
									<button
										onclick={() =>
											removeRecentFile(file.path)}
										class="shrink-0 rounded p-1 opacity-0 transition-opacity hover:bg-muted-foreground/20 group-hover:opacity-100"
										title="Remove from recent"
									>
										<XIcon
											class="size-4 text-muted-foreground"
										/>
									</button>
								</div>
							{/each}
						</div>
					</Card.Content>
				</Card.Root>
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
