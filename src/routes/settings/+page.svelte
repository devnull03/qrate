<script lang="ts">
	import { onMount } from "svelte";
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
		getGlobalSettings,
		setGlobalSetting,
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

	// Global settings state
	let globalTheme = $state<"light" | "dark" | "system">("system");
	let globalDefaultRowLimit = $state(100);
	let globalDefaultFilePathPattern = $state("{files_folder}/{file_column}");
	let globalDefaultFileColumnName = $state("file");
	let globalConfirmBeforeDelete = $state(true);

	// Project settings state
	let projectFilesFolder = $state("");
	let projectFilePathPattern = $state("{files_folder}/{file_column}");
	let projectFileColumnName = $state("file");
	let projectDefaultRowLimit = $state("100");

	// UI state
	let isLoaded = $state(false);
	let hasFile = $state(false);
	let loadError = $state<string | null>(null);

	// Load settings on mount
	onMount(async () => {
		console.log("[Settings] onMount starting...");

		try {
			// Load global settings first
			console.log("[Settings] Loading global settings...");
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

			// Apply theme
			if (globalTheme === "system") {
				resetMode();
			} else {
				setMode(globalTheme);
			}

			// Settings window is a separate Tauri window, so we need to sync state from backend
			console.log("[Settings] Syncing state from backend...");
			const synced = await qrateStore.syncFromBackend();
			console.log("[Settings] Backend sync result:", synced);

			hasFile = !!qrateStore.currentFilePath;
			console.log("[Settings] hasFile:", hasFile);

			if (hasFile) {
				console.log("[Settings] Loading project settings from file...");
				try {
					const defaults = await getDefaultProjectSettings();
					const settings = await loadSettings();
					console.log(
						"[Settings] Loaded project settings:",
						settings,
					);
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
					console.log(
						"[Settings] Project settings applied successfully",
					);
				} catch (err) {
					const errorMsg =
						err instanceof Error ? err.message : String(err);
					console.error(
						"[Settings] Failed to load project settings:",
						errorMsg,
					);
					loadError = `Failed to load project settings: ${errorMsg}`;
				}
			} else {
				console.log(
					"[Settings] No file open, using defaults for project settings",
				);
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
			}
		} catch (err) {
			const errorMsg = err instanceof Error ? err.message : String(err);
			console.error("[Settings] Error during initialization:", errorMsg);
			loadError = `Initialization error: ${errorMsg}`;
		}

		console.log("[Settings] Setting isLoaded = true");
		isLoaded = true;
	});

	// Handle theme change (global setting)
	async function handleThemeChange(newTheme: "light" | "dark" | "system") {
		console.log("[Settings] Theme change:", newTheme);
		globalTheme = newTheme;

		if (newTheme === "system") {
			resetMode();
		} else {
			setMode(newTheme);
		}

		try {
			await setGlobalSetting("theme", newTheme);
		} catch (err) {
			console.error("[Settings] Failed to save theme:", err);
		}
	}

	// Save global defaults
	async function handleSaveGlobalDefaults() {
		console.log("[Settings] Saving global defaults");
		try {
			await setGlobalSettings({
				defaultRowLimit: globalDefaultRowLimit,
				defaultFilePathPattern: globalDefaultFilePathPattern,
				defaultFileColumnName: globalDefaultFileColumnName,
				confirmBeforeDelete: globalConfirmBeforeDelete,
			});
			console.log("[Settings] Global defaults saved");
		} catch (err) {
			console.error("[Settings] Failed to save global defaults:", err);
		}
	}

	// Browse for files folder (project setting)
	async function browseFilesFolder() {
		console.log("[Settings] browseFilesFolder called");
		try {
			const folder = await open({
				directory: true,
				multiple: false,
				title: "Select Files Folder",
			});
			console.log("[Settings] Folder selected:", folder);

			if (folder && typeof folder === "string") {
				projectFilesFolder = folder;
				await handleSaveProjectSettings();
			}
		} catch (err) {
			console.error("[Settings] Failed to select folder:", err);
		}
	}

	// Save project settings to the .qrate file
	async function handleSaveProjectSettings() {
		console.log(
			"[Settings] handleSaveProjectSettings called, hasFile:",
			hasFile,
		);
		if (!hasFile) {
			console.log("[Settings] No file open, skipping save");
			return;
		}

		try {
			console.log("[Settings] Saving project settings:", {
				filesFolder: projectFilesFolder,
				filePathPattern: projectFilePathPattern,
				fileColumnName: projectFileColumnName,
				defaultRowLimit: projectDefaultRowLimit,
			});
			await saveSettings({
				filesFolder: projectFilesFolder,
				filePathPattern: projectFilePathPattern,
				fileColumnName: projectFileColumnName,
				defaultRowLimit: projectDefaultRowLimit,
			});
			console.log("[Settings] Project settings saved successfully");
		} catch (err) {
			console.error("[Settings] Failed to save project settings:", err);
		}
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
				<div>
					<h1 class="text-2xl font-bold">Settings</h1>
					<p class="text-sm text-muted-foreground">
						Configure your qRate preferences
					</p>
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

				<!-- Global Settings Section -->
				<div class="space-y-4">
					<div class="flex items-center gap-2">
						<GlobeIcon class="size-5 text-primary" />
						<h2 class="text-lg font-semibold">Global Settings</h2>
					</div>
					<p class="text-sm text-muted-foreground">
						These settings apply to all projects and persist across
						app restarts.
					</p>

					<!-- Appearance -->
					<Card.Root>
						<Card.Header>
							<Card.Title>Appearance</Card.Title>
							<Card.Description>
								Customize the look and feel of the application
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
										class="flex-1 gap-2"
										onclick={() =>
											handleThemeChange("light")}
									>
										<SunIcon class="size-4" />
										Light
									</Button>
									<Button
										variant={globalTheme === "dark"
											? "default"
											: "outline"}
										size="sm"
										class="flex-1 gap-2"
										onclick={() =>
											handleThemeChange("dark")}
									>
										<MoonIcon class="size-4" />
										Dark
									</Button>
									<Button
										variant={globalTheme === "system"
											? "default"
											: "outline"}
										size="sm"
										class="flex-1 gap-2"
										onclick={() =>
											handleThemeChange("system")}
									>
										<MonitorIcon class="size-4" />
										System
									</Button>
								</div>
							</div>
						</Card.Content>
					</Card.Root>

					<!-- Default Values for New Projects -->
					<Card.Root>
						<Card.Header>
							<Card.Title>Default Values</Card.Title>
							<Card.Description>
								Default settings used when creating new projects
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
									onchange={handleSaveGlobalDefaults}
								/>
								<p class="text-xs text-muted-foreground">
									Default number of rows to load at a time
									(50-1000)
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
									onchange={handleSaveGlobalDefaults}
								/>
								<p class="text-xs text-muted-foreground">
									Default column name for file references
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
									onchange={handleSaveGlobalDefaults}
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
						These settings are stored in the .qrate file and are
						specific to the current project.
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
									No project file is open. Open a .qrate file
									to configure project-specific settings.
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
									Base folder containing all files referenced
									in this project
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
									onchange={handleSaveProjectSettings}
									disabled={!hasFile}
								/>
								<p class="text-xs text-muted-foreground">
									The column name containing file names in
									your CSV
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
									onchange={handleSaveProjectSettings}
									disabled={!hasFile}
								/>
								<p class="text-xs text-muted-foreground">
									Pattern for locating files. Use
									&#123;files_folder&#125;,
									&#123;file_column&#125;, or any column name.
								</p>
							</div>

							<div class="rounded-md bg-muted p-3">
								<p class="mb-2 text-xs font-medium">
									Pattern Examples:
								</p>
								<div
									class="space-y-1 text-xs text-muted-foreground"
								>
									<p>
										<code class="rounded bg-background px-1"
											>&#123;files_folder&#125;/&#123;file_column&#125;</code
										>
										- Files directly in folder
									</p>
									<p>
										<code class="rounded bg-background px-1"
											>&#123;files_folder&#125;/&#123;category&#125;/&#123;file_column&#125;</code
										>
										- Organized by category
									</p>
									<p>
										<code class="rounded bg-background px-1"
											>&#123;files_folder&#125;/&#123;year&#125;/&#123;month&#125;/&#123;file_column&#125;</code
										>
										- Date-based organization
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
									onchange={handleSaveProjectSettings}
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
			</div>
		</div>
	{/if}
</div>
