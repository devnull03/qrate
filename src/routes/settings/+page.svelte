<script lang="ts">
	import { onMount } from "svelte";
	import { getCurrentWindow } from "@tauri-apps/api/window";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import { Label } from "$lib/components/ui/label/index.js";
	import * as Card from "$lib/components/ui/card/index.js";
	import { Separator } from "$lib/components/ui/separator/index.js";
	import SimpleTitleBar from "$lib/components/SimpleTitleBar.svelte";
	import { ModeWatcher, setMode, resetMode } from "mode-watcher";
	import {
		loadSettings,
		saveSettings,
		getDefaultProjectSettings,
	} from "$lib/stores/appSettings";
	import {
		initGlobalSettings,
		setGlobalSettings,
		type GlobalSettings,
	} from "$lib/stores/globalSettings";
	import { qrateStore } from "$lib/stores/qrateStore.svelte";
	import { open } from "@tauri-apps/plugin-dialog";
	import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
	import SunIcon from "@lucide/svelte/icons/sun";
	import MoonIcon from "@lucide/svelte/icons/moon";
	import MonitorIcon from "@lucide/svelte/icons/monitor";
	import AlertCircleIcon from "@lucide/svelte/icons/alert-circle";
	import GlobeIcon from "@lucide/svelte/icons/globe";
	import FileIcon from "@lucide/svelte/icons/file";
	import SaveIcon from "@lucide/svelte/icons/save";
	import XIcon from "@lucide/svelte/icons/x";
	import CheckIcon from "@lucide/svelte/icons/check";

	// Global settings state
	let globalTheme = $state<"light" | "dark" | "system">("system");
	let globalDefaultRowLimit = $state(100);
	let globalDefaultFilePathPattern = $state("{files_folder}/{file_column}");
	let globalDefaultFileColumnName = $state("file");
	let globalConfirmBeforeDelete = $state(true);

	// Original global settings (for dirty checking)
	let originalGlobalSettings = $state({
		theme: "system" as "light" | "dark" | "system",
		defaultRowLimit: 100,
		defaultFilePathPattern: "{files_folder}/{file_column}",
		defaultFileColumnName: "file",
		confirmBeforeDelete: true,
	});

	// Project settings state
	let projectFilesFolder = $state("");
	let projectFilePathPattern = $state("{files_folder}/{file_column}");
	let projectFileColumnName = $state("file");
	let projectDefaultRowLimit = $state("100");

	// Original project settings (for dirty checking)
	let originalProjectSettings = $state({
		filesFolder: "",
		filePathPattern: "{files_folder}/{file_column}",
		fileColumnName: "file",
		defaultRowLimit: "100",
	});

	// UI state
	let isLoaded = $state(false);
	let hasFile = $state(false);
	let loadError = $state<string | null>(null);
	let saveSuccess = $state<string | null>(null);
	let isSaving = $state(false);

	// Dirty state tracking
	let isGlobalDirty = $derived(
		globalTheme !== originalGlobalSettings.theme ||
			globalDefaultRowLimit !== originalGlobalSettings.defaultRowLimit ||
			globalDefaultFilePathPattern !==
				originalGlobalSettings.defaultFilePathPattern ||
			globalDefaultFileColumnName !==
				originalGlobalSettings.defaultFileColumnName ||
			globalConfirmBeforeDelete !==
				originalGlobalSettings.confirmBeforeDelete,
	);

	let isProjectDirty = $derived(
		hasFile &&
			(projectFilesFolder !== originalProjectSettings.filesFolder ||
				projectFilePathPattern !==
					originalProjectSettings.filePathPattern ||
				projectFileColumnName !==
					originalProjectSettings.fileColumnName ||
				projectDefaultRowLimit !==
					originalProjectSettings.defaultRowLimit),
	);

	let isDirty = $derived(isGlobalDirty || isProjectDirty);

	// Load settings on mount
	onMount(async () => {
		try {
			// Load global settings first
			const globalSettings = await initGlobalSettings();
			globalTheme =
				(globalSettings.theme as "light" | "dark" | "system") ??
				"system";
			globalDefaultRowLimit = Number(
				globalSettings.defaultRowLimit ?? 100,
			);
			globalDefaultFilePathPattern = String(
				globalSettings.defaultFilePathPattern ??
					"{files_folder}/{file_column}",
			);
			globalDefaultFileColumnName = String(
				globalSettings.defaultFileColumnName ?? "file",
			);
			globalConfirmBeforeDelete =
				String(globalSettings.confirmBeforeDelete ?? "true") === "true";

			// Store original values
			originalGlobalSettings = {
				theme: globalTheme,
				defaultRowLimit: globalDefaultRowLimit,
				defaultFilePathPattern: globalDefaultFilePathPattern,
				defaultFileColumnName: globalDefaultFileColumnName,
				confirmBeforeDelete: globalConfirmBeforeDelete,
			};

			// Apply theme
			if (globalTheme === "system") {
				resetMode();
			} else {
				setMode(globalTheme);
			}

			// Settings window is a separate Tauri window, so we need to sync state from backend
			let synced = await qrateStore.syncFromBackend();

			// If backend has no file open, try to restore from persisted workspace
			if (!synced) {
				synced = await qrateStore.restoreWorkspace(true);
			}

			hasFile = !!qrateStore.currentFilePath;

			if (hasFile) {
				try {
					const defaults = await getDefaultProjectSettings();
					const settings = await loadSettings();
					projectFilesFolder = String(
						settings.filesFolder ?? defaults.filesFolder,
					);
					projectFilePathPattern = String(
						settings.filePathPattern ?? defaults.filePathPattern,
					);
					projectFileColumnName = String(
						settings.fileColumnName ?? defaults.fileColumnName,
					);
					projectDefaultRowLimit = String(
						settings.defaultRowLimit ?? defaults.defaultRowLimit,
					);

					// Store original values
					originalProjectSettings = {
						filesFolder: projectFilesFolder,
						filePathPattern: projectFilePathPattern,
						fileColumnName: projectFileColumnName,
						defaultRowLimit: projectDefaultRowLimit,
					};
				} catch (err) {
					const errorMsg =
						err instanceof Error ? err.message : String(err);
					loadError = `Failed to load project settings: ${errorMsg}`;
				}
			} else {
				const defaults = await getDefaultProjectSettings();
				projectFilesFolder = String(defaults.filesFolder ?? "");
				projectFilePathPattern = String(
					defaults.filePathPattern ?? "{files_folder}/{file_column}",
				);
				projectFileColumnName = String(
					defaults.fileColumnName ?? "file",
				);
				projectDefaultRowLimit = String(
					defaults.defaultRowLimit ?? "100",
				);

				originalProjectSettings = {
					filesFolder: projectFilesFolder,
					filePathPattern: projectFilePathPattern,
					fileColumnName: projectFileColumnName,
					defaultRowLimit: projectDefaultRowLimit,
				};
			}
		} catch (err) {
			const errorMsg = err instanceof Error ? err.message : String(err);
			loadError = `Initialization error: ${errorMsg}`;
		}

		isLoaded = true;
	});

	// Handle theme change (applies immediately but still needs save)
	function handleThemeChange(newTheme: "light" | "dark" | "system") {
		globalTheme = newTheme;

		if (newTheme === "system") {
			resetMode();
		} else {
			setMode(newTheme);
		}
	}

	// Browse for files folder
	async function browseFilesFolder() {
		try {
			const folder = await open({
				directory: true,
				multiple: false,
				title: "Select Files Folder",
			});

			if (folder && typeof folder === "string") {
				projectFilesFolder = folder;
			}
		} catch (err) {
			console.error("[Settings] Failed to select folder:", err);
		}
	}

	// Save all settings
	async function handleSave() {
		isSaving = true;
		saveSuccess = null;
		loadError = null;

		try {
			// Save global settings
			if (isGlobalDirty) {
				await setGlobalSettings({
					theme: globalTheme,
					defaultRowLimit: globalDefaultRowLimit,
					defaultFilePathPattern: globalDefaultFilePathPattern,
					defaultFileColumnName: globalDefaultFileColumnName,
					confirmBeforeDelete: globalConfirmBeforeDelete,
				});

				originalGlobalSettings = {
					theme: globalTheme,
					defaultRowLimit: globalDefaultRowLimit,
					defaultFilePathPattern: globalDefaultFilePathPattern,
					defaultFileColumnName: globalDefaultFileColumnName,
					confirmBeforeDelete: globalConfirmBeforeDelete,
				};
			}

			// Save project settings
			if (isProjectDirty && hasFile) {
				await saveSettings({
					filesFolder: projectFilesFolder,
					filePathPattern: projectFilePathPattern,
					fileColumnName: projectFileColumnName,
					defaultRowLimit: projectDefaultRowLimit,
				});

				originalProjectSettings = {
					filesFolder: projectFilesFolder,
					filePathPattern: projectFilePathPattern,
					fileColumnName: projectFileColumnName,
					defaultRowLimit: projectDefaultRowLimit,
				};
			}

			saveSuccess = "Settings saved successfully!";
			setTimeout(() => {
				saveSuccess = null;
			}, 3000);
		} catch (err) {
			const errorMsg = err instanceof Error ? err.message : String(err);
			loadError = `Failed to save settings: ${errorMsg}`;
		} finally {
			isSaving = false;
		}
	}

	// Cancel and revert changes
	function handleCancel() {
		// Revert global settings
		globalTheme = originalGlobalSettings.theme;
		globalDefaultRowLimit = originalGlobalSettings.defaultRowLimit;
		globalDefaultFilePathPattern =
			originalGlobalSettings.defaultFilePathPattern;
		globalDefaultFileColumnName =
			originalGlobalSettings.defaultFileColumnName;
		globalConfirmBeforeDelete = originalGlobalSettings.confirmBeforeDelete;

		// Apply reverted theme
		if (globalTheme === "system") {
			resetMode();
		} else {
			setMode(globalTheme);
		}

		// Revert project settings
		projectFilesFolder = originalProjectSettings.filesFolder;
		projectFilePathPattern = originalProjectSettings.filePathPattern;
		projectFileColumnName = originalProjectSettings.fileColumnName;
		projectDefaultRowLimit = originalProjectSettings.defaultRowLimit;
	}

	// Close window
	async function handleClose() {
		const window = getCurrentWindow();
		await window.close();
	}
</script>

<ModeWatcher />

<div class="flex h-screen w-screen flex-col bg-background text-foreground">
	<SimpleTitleBar title="qRate - Settings" />

	{#if !isLoaded}
		<div class="flex flex-1 items-center justify-center">
			<p class="text-sm text-muted-foreground">Loading settings...</p>
		</div>
	{:else}
		<div class="flex-1 overflow-auto p-6">
			<div class="mx-auto max-w-2xl space-y-6">
				<div class="flex items-center justify-between">
					<div>
						<h1 class="text-2xl font-bold">Settings</h1>
						<p class="text-sm text-muted-foreground">
							Configure global and project-specific settings
						</p>
					</div>
					<div class="flex items-center gap-2">
						{#if isDirty}
							<span class="text-xs text-amber-500"
								>Unsaved changes</span
							>
						{/if}
					</div>
				</div>

				{#if loadError}
					<Card.Root class="border-red-500/50 bg-red-500/10">
						<Card.Content class="flex items-center gap-3 pt-6">
							<AlertCircleIcon class="size-5 text-red-500" />
							<p class="text-sm text-red-700 dark:text-red-400">
								{loadError}
							</p>
						</Card.Content>
					</Card.Root>
				{/if}

				{#if saveSuccess}
					<Card.Root class="border-green-500/50 bg-green-500/10">
						<Card.Content class="flex items-center gap-3 pt-6">
							<CheckIcon class="size-5 text-green-500" />
							<p
								class="text-sm text-green-700 dark:text-green-400"
							>
								{saveSuccess}
							</p>
						</Card.Content>
					</Card.Root>
				{/if}

				<!-- Global Settings Section -->
				<div class="space-y-4">
					<div class="flex items-center gap-2">
						<GlobeIcon class="size-5 text-primary" />
						<h2 class="text-lg font-semibold">Global Settings</h2>
					</div>
					<p class="text-sm text-muted-foreground">
						These settings apply across all projects and are stored
						locally on your machine.
					</p>

					<!-- Appearance -->
					<Card.Root>
						<Card.Header>
							<Card.Title>Appearance</Card.Title>
							<Card.Description>
								Customize how the application looks
							</Card.Description>
						</Card.Header>
						<Card.Content class="space-y-4">
							<div class="space-y-2">
								<Label>Theme</Label>
								<div class="flex gap-2">
									<Button
										variant={globalTheme === "light"
											? "default"
											: "outline"}
										size="sm"
										onclick={() =>
											handleThemeChange("light")}
									>
										<SunIcon class="mr-1 size-4" />
										Light
									</Button>
									<Button
										variant={globalTheme === "dark"
											? "default"
											: "outline"}
										size="sm"
										onclick={() =>
											handleThemeChange("dark")}
									>
										<MoonIcon class="mr-1 size-4" />
										Dark
									</Button>
									<Button
										variant={globalTheme === "system"
											? "default"
											: "outline"}
										size="sm"
										onclick={() =>
											handleThemeChange("system")}
									>
										<MonitorIcon class="mr-1 size-4" />
										System
									</Button>
								</div>
							</div>
						</Card.Content>
					</Card.Root>

					<!-- Global Defaults -->
					<Card.Root>
						<Card.Header>
							<Card.Title>Defaults</Card.Title>
							<Card.Description>
								Default values for new projects
							</Card.Description>
						</Card.Header>
						<Card.Content class="space-y-4">
							<div class="space-y-2">
								<Label for="global-row-limit"
									>Default Row Limit</Label
								>
								<Input
									id="global-row-limit"
									type="number"
									bind:value={globalDefaultRowLimit}
									min={50}
									max={1000}
								/>
								<p class="text-xs text-muted-foreground">
									Default number of rows to load (50-1000)
								</p>
							</div>

							<Separator />

							<div class="space-y-2">
								<Label for="global-file-column"
									>Default File Column Name</Label
								>
								<Input
									id="global-file-column"
									bind:value={globalDefaultFileColumnName}
									placeholder="file"
								/>
								<p class="text-xs text-muted-foreground">
									Default column name containing file
									references
								</p>
							</div>

							<Separator />

							<div class="space-y-2">
								<Label for="global-path-pattern"
									>Default File Path Pattern</Label
								>
								<Input
									id="global-path-pattern"
									bind:value={globalDefaultFilePathPattern}
									placeholder="&#123;files_folder&#125;/&#123;file_column&#125;"
								/>
								<p class="text-xs text-muted-foreground">
									Default pattern for locating files in new
									projects
								</p>
							</div>
						</Card.Content>
					</Card.Root>
				</div>

				<Separator class="my-8" />

				<!-- Project Settings Section -->
				<div class="space-y-4">
					<div class="flex items-center gap-2">
						<FileIcon class="size-5 text-primary" />
						<h2 class="text-lg font-semibold">Project Settings</h2>
					</div>
					<p class="text-sm text-muted-foreground">
						These settings are stored in the project's .qrate folder
						and are specific to the current project.
					</p>

					{#if !hasFile}
						<Card.Root class="border-amber-500/50 bg-amber-500/10">
							<Card.Content class="flex items-center gap-3 pt-6">
								<AlertCircleIcon
									class="size-5 text-amber-500"
								/>
								<p
									class="text-sm text-amber-700 dark:text-amber-400"
								>
									No project is currently open. Open a project
									from the main window to configure
									project-specific settings.
								</p>
							</Card.Content>
						</Card.Root>
					{:else}
						<Card.Root class="border-green-500/50 bg-green-500/10">
							<Card.Content class="flex items-center gap-3 pt-6">
								<CheckIcon class="size-5 text-green-500" />
								<p
									class="text-sm text-green-700 dark:text-green-400"
								>
									Project loaded: {qrateStore.currentFilePath}
								</p>
							</Card.Content>
						</Card.Root>
					{/if}

					<!-- Files Configuration -->
					<Card.Root
						class={!hasFile ? "pointer-events-none opacity-50" : ""}
					>
						<Card.Header>
							<Card.Title>Files</Card.Title>
							<Card.Description>
								Configure how files are located and displayed
								for this project
							</Card.Description>
						</Card.Header>
						<Card.Content class="space-y-4">
							<div class="space-y-2">
								<Label for="files-folder">Files Folder</Label>
								<div class="flex gap-2">
									<Input
										id="files-folder"
										bind:value={projectFilesFolder}
										placeholder="Select folder..."
										readonly
										disabled={!hasFile}
									/>
									<Button
										variant="outline"
										onclick={browseFilesFolder}
										disabled={!hasFile}
									>
										<FolderOpenIcon class="size-4" />
									</Button>
								</div>
								<p class="text-xs text-muted-foreground">
									Root folder containing the files referenced
									in your data
								</p>
							</div>

							<Separator />

							<div class="space-y-2">
								<Label for="file-column">File Column Name</Label
								>
								<Input
									id="file-column"
									bind:value={projectFileColumnName}
									placeholder="file"
									disabled={!hasFile}
								/>
								<p class="text-xs text-muted-foreground">
									Column name containing file references
								</p>
							</div>

							<Separator />

							<div class="space-y-2">
								<Label for="path-pattern"
									>File Path Pattern</Label
								>
								<Input
									id="path-pattern"
									bind:value={projectFilePathPattern}
									placeholder="&#123;files_folder&#125;/&#123;file_column&#125;"
									disabled={!hasFile}
								/>
								<p class="text-xs text-muted-foreground">
									Pattern for constructing full file paths
								</p>
							</div>

							<div class="rounded-md bg-muted p-3">
								<p class="mb-2 text-xs font-medium">
									Available variables:
								</p>
								<div
									class="space-y-1 text-xs text-muted-foreground"
								>
									<p>
										<code class="rounded bg-background px-1"
											>&#123;files_folder&#125;</code
										>
										- The files folder path
									</p>
									<p>
										<code class="rounded bg-background px-1"
											>&#123;file_column&#125;</code
										>
										- Value from the file column
									</p>
									<p>
										<code class="rounded bg-background px-1"
											>&#123;column_name&#125;</code
										>
										- Value from any column
									</p>
								</div>
							</div>
						</Card.Content>
					</Card.Root>

					<!-- Data Settings -->
					<Card.Root
						class={!hasFile ? "pointer-events-none opacity-50" : ""}
					>
						<Card.Header>
							<Card.Title>Data</Card.Title>
							<Card.Description>
								Configure data loading settings for this project
							</Card.Description>
						</Card.Header>
						<Card.Content class="space-y-4">
							<div class="space-y-2">
								<Label for="row-limit">Row Limit</Label>
								<Input
									id="row-limit"
									type="number"
									bind:value={projectDefaultRowLimit}
									min={50}
									max={1000}
									disabled={!hasFile}
								/>
								<p class="text-xs text-muted-foreground">
									Number of rows to load at a time for this
									project (50-1000)
								</p>
							</div>
						</Card.Content>
					</Card.Root>
				</div>

				<Separator class="my-8" />

				<!-- About -->
				<Card.Root>
					<Card.Header>
						<Card.Title>About</Card.Title>
					</Card.Header>
					<Card.Content>
						<div class="space-y-2 text-sm">
							<p>
								<span class="font-medium">qRate</span> - Digital Archival
								Workspace
							</p>
							<p class="text-muted-foreground">Version 0.1.0</p>
						</div>
					</Card.Content>
				</Card.Root>

				<!-- Spacer for bottom buttons -->
				<div class="h-20"></div>
			</div>
		</div>

		<!-- Fixed bottom action bar -->
		<div
			class="sticky bottom-0 flex items-center justify-end gap-3 border-t bg-background px-6 py-4"
		>
			<Button
				variant="outline"
				onclick={handleCancel}
				disabled={!isDirty}
			>
				<XIcon class="mr-1 size-4" />
				Cancel
			</Button>
			<Button onclick={handleSave} disabled={!isDirty || isSaving}>
				{#if isSaving}
					<span class="mr-1 size-4 animate-spin">⏳</span>
					Saving...
				{:else}
					<SaveIcon class="mr-1 size-4" />
					Save Changes
				{/if}
			</Button>
		</div>
	{/if}
</div>
