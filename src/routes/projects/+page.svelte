<script lang="ts">
	import { onMount } from "svelte";
	import { invoke } from "@tauri-apps/api/core";
	import { open, save } from "@tauri-apps/plugin-dialog";
	import { qrateStore } from "$lib/stores/qrateStore.svelte";
	import {
		initRecentFiles,
		subscribeToRecentFiles,
		removeRecentFile,
		type RecentFile,
	} from "$lib/stores/recentFiles";
	import { saveSettings, defaultSettings } from "$lib/stores/appSettings";
	import { initGlobalSettings } from "$lib/stores/globalSettings";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import { Label } from "$lib/components/ui/label/index.js";
	import * as Card from "$lib/components/ui/card/index.js";
	import { Separator } from "$lib/components/ui/separator/index.js";
	import CommandIcon from "@lucide/svelte/icons/command";
	import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
	import FilePlusIcon from "@lucide/svelte/icons/file-plus";
	import UploadIcon from "@lucide/svelte/icons/upload";
	import FileIcon from "@lucide/svelte/icons/file";
	import ClockIcon from "@lucide/svelte/icons/clock";
	import XIcon from "@lucide/svelte/icons/x";
	import CheckIcon from "@lucide/svelte/icons/check";
	import ArrowLeftIcon from "@lucide/svelte/icons/arrow-left";
	import { ModeWatcher } from "mode-watcher";
	import ModeToggle from "$lib/components/ModeToggle.svelte";
	import SimpleTitleBar from "$lib/components/SimpleTitleBar.svelte";

	let isProcessing = $state(false);
	let error = $state<string | null>(null);
	let recentFiles = $state<RecentFile[]>([]);

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

	// Import wizard state
	let showImportWizard = $state(false);
	let importStep = $state<"select-csv" | "configure">("select-csv");
	let selectedCsvFile = $state<string | null>(null);
	let selectedFilesFolder = $state<string>(defaultSettings.filesFolder);
	let selectedFileColumn = $state<string>(defaultSettings.fileColumnName);
	let selectedPathPattern = $state<string>(defaultSettings.filePathPattern);

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
	 * Start the import CSV wizard
	 */
	function startImportWizard() {
		showImportWizard = true;
		importStep = "select-csv";
		selectedCsvFile = null;
		selectedFilesFolder = defaultSettings.filesFolder;
		selectedFileColumn = defaultSettings.fileColumnName;
		selectedPathPattern = defaultSettings.filePathPattern;
		error = null;
	}

	/**
	 * Cancel the import wizard
	 */
	function cancelImportWizard() {
		showImportWizard = false;
		selectedCsvFile = null;
	}

	/**
	 * Select a CSV file for import
	 */
	async function selectCsvFile() {
		try {
			const csvFile = await open({
				multiple: false,
				filters: [
					{
						name: "CSV Files",
						extensions: ["csv"],
					},
				],
			});

			if (csvFile && typeof csvFile === "string") {
				selectedCsvFile = csvFile;
				importStep = "configure";
			}
		} catch (err) {
			console.error("Failed to select CSV file:", err);
			error = err instanceof Error ? err.message : String(err);
		}
	}

	/**
	 * Browse for files folder
	 */
	async function browseFilesFolder() {
		try {
			const folder = await open({
				directory: true,
				multiple: false,
				title: "Select Files Folder",
			});

			if (folder && typeof folder === "string") {
				selectedFilesFolder = folder;
			}
		} catch (err) {
			console.error("Failed to select folder:", err);
		}
	}

	/**
	 * Complete the CSV import
	 */
	async function completeImport() {
		if (!selectedCsvFile) return;

		try {
			isProcessing = true;
			error = null;

			const qrateFile = await save({
				filters: [
					{
						name: "Qrate Files",
						extensions: ["qrate"],
					},
				],
				defaultPath: selectedCsvFile.replace(/\.csv$/i, ".qrate"),
			});

			if (!qrateFile) {
				isProcessing = false;
				return;
			}

			await qrateStore.importCsv(qrateFile, selectedCsvFile);

			// Save settings after importing (now that file is open)
			await saveSettings({
				filesFolder: selectedFilesFolder,
				fileColumnName: selectedFileColumn,
				filePathPattern: selectedPathPattern,
			});
			showImportWizard = false;
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

	/**
	 * Get filename from path
	 */
	function getFileName(path: string): string {
		return path.split(/[/\\]/).pop() || path;
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

			{#if showImportWizard}
				<!-- Import Wizard -->
				<Card.Root>
					<Card.Header>
						<div class="flex items-center gap-3">
							<Button
								variant="ghost"
								size="icon-sm"
								onclick={cancelImportWizard}
							>
								<ArrowLeftIcon class="size-4" />
							</Button>
							<div>
								<Card.Title>Import CSV</Card.Title>
								<Card.Description>
									{#if importStep === "select-csv"}
										Select a CSV file to import
									{:else}
										Configure file settings
									{/if}
								</Card.Description>
							</div>
						</div>
					</Card.Header>
					<Card.Content class="space-y-6">
						{#if importStep === "select-csv"}
							<!-- Step 1: Select CSV -->
							<div class="flex flex-col items-center gap-4 py-8">
								<UploadIcon
									class="size-12 text-muted-foreground"
								/>
								<p
									class="text-center text-sm text-muted-foreground"
								>
									Choose a CSV file to import into qRate
								</p>
								<Button onclick={selectCsvFile}>
									<FolderOpenIcon class="mr-2 size-4" />
									Select CSV File
								</Button>
							</div>
						{:else}
							<!-- Step 2: Configure -->
							<div class="space-y-4">
								<!-- Selected File -->
								<div
									class="flex items-center gap-3 rounded-md border border-border bg-muted/50 p-3"
								>
									<FileIcon
										class="size-5 text-muted-foreground"
									/>
									<div class="min-w-0 flex-1">
										<p class="truncate text-sm font-medium">
											{selectedCsvFile
												? getFileName(selectedCsvFile)
												: ""}
										</p>
										<p
											class="truncate text-xs text-muted-foreground"
										>
											{selectedCsvFile}
										</p>
									</div>
									<CheckIcon class="size-5 text-green-500" />
								</div>

								<Separator />

								<!-- Files Folder -->
								<div class="space-y-2">
									<Label for="files-folder">
										Files Folder (Optional)
									</Label>
									<div class="flex gap-2">
										<Input
											id="files-folder"
											bind:value={selectedFilesFolder}
											placeholder="Select folder containing files..."
											readonly
										/>
										<Button
											variant="outline"
											onclick={browseFilesFolder}
										>
											<FolderOpenIcon class="size-4" />
										</Button>
									</div>
									<p class="text-xs text-muted-foreground">
										The folder containing files referenced
										in your CSV
									</p>
								</div>

								<!-- File Column Name -->
								<div class="space-y-2">
									<Label for="file-column">
										File Column Name
									</Label>
									<Input
										id="file-column"
										bind:value={selectedFileColumn}
										placeholder="file"
									/>
									<p class="text-xs text-muted-foreground">
										The CSV column containing file names
										(e.g., "file", "filename", "image")
									</p>
								</div>

								<!-- Path Pattern -->
								<div class="space-y-2">
									<Label for="path-pattern">
										File Path Pattern
									</Label>
									<Input
										id="path-pattern"
										bind:value={selectedPathPattern}
										placeholder={"{files_folder}/{file_column}"}
									/>
									<p class="text-xs text-muted-foreground">
										Pattern for locating files. Use
										&#123;files_folder&#125;,
										&#123;file_column&#125;, or any column
										name.
									</p>
								</div>
							</div>
						{/if}
					</Card.Content>
					{#if importStep === "configure"}
						<Card.Footer class="flex justify-end gap-2">
							<Button
								variant="outline"
								onclick={() => (importStep = "select-csv")}
							>
								Back
							</Button>
							<Button
								onclick={completeImport}
								disabled={isProcessing}
							>
								{#if isProcessing}
									Importing...
								{:else}
									Import CSV
								{/if}
							</Button>
						</Card.Footer>
					{/if}
				</Card.Root>
			{:else}
				<!-- Actions -->
				<Card.Root>
					<Card.Header>
						<Card.Title>Get Started</Card.Title>
						<Card.Description>
							Create a new project or open an existing one
						</Card.Description>
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
								onclick={startImportWizard}
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
				{#if recentFiles.length > 0}
					<Card.Root>
						<Card.Header>
							<Card.Title class="flex items-center gap-2">
								<ClockIcon class="size-4" />
								Recent Projects
							</Card.Title>
						</Card.Header>
						<Card.Content>
							<div class="space-y-2">
								{#each recentFiles as file (file.path)}
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
													class="truncate text-left font-medium"
												>
													{file.name}
												</p>
												<p
													class="truncate text-left text-xs text-muted-foreground"
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
											onclick={async () =>
												await removeRecentFile(
													file.path,
												)}
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
