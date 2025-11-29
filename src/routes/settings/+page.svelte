<script lang="ts">
	import { Button } from "$lib/components/ui/button/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import { Label } from "$lib/components/ui/label/index.js";
	import { Switch } from "$lib/components/ui/switch/index.js";
	import * as Card from "$lib/components/ui/card/index.js";
	import { Separator } from "$lib/components/ui/separator/index.js";
	import SimpleTitleBar from "$lib/components/SimpleTitleBar.svelte";
	import { ModeWatcher, setMode, resetMode } from "mode-watcher";
	import { appSettingsStore } from "$lib/stores/appSettings";
	import { open } from "@tauri-apps/plugin-dialog";
	import { get } from "svelte/store";
	import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
	import SunIcon from "@lucide/svelte/icons/sun";
	import MoonIcon from "@lucide/svelte/icons/moon";
	import MonitorIcon from "@lucide/svelte/icons/monitor";

	// Settings state
	let filesFolder = $state("");
	let filePathPattern = $state("{files_folder}/{file_column}");
	let fileColumnName = $state("file");
	let defaultRowLimit = $state(100);
	let theme = $state<"light" | "dark" | "system">("system");

	// Load settings from store
	$effect(() => {
		const unsubscribe = appSettingsStore.subscribe((settings) => {
			filesFolder = settings.filesFolder || "";
			filePathPattern =
				settings.filePathPattern || "{files_folder}/{file_column}";
			fileColumnName = settings.fileColumnName || "file";
			defaultRowLimit = settings.defaultRowLimit || 100;
			theme = settings.theme || "system";
		});
		return unsubscribe;
	});

	// Browse for files folder
	async function browseFilesFolder() {
		try {
			const folder = await open({
				directory: true,
				multiple: false,
				title: "Select Files Folder",
			});

			if (folder && typeof folder === "string") {
				filesFolder = folder;
				saveSettings();
			}
		} catch (err) {
			console.error("Failed to select folder:", err);
		}
	}

	// Save settings to store
	function saveSettings() {
		appSettingsStore.set({
			...get(appSettingsStore),
			filesFolder,
			filePathPattern,
			fileColumnName,
			defaultRowLimit,
			theme,
		});
	}

	// Handle theme change
	function handleThemeChange(newTheme: "light" | "dark" | "system") {
		theme = newTheme;
		if (newTheme === "system") {
			resetMode();
		} else {
			setMode(newTheme);
		}
		saveSettings();
	}
</script>

<ModeWatcher />

<div class="flex h-screen w-screen flex-col bg-background">
	<SimpleTitleBar title="qRate - Settings" />

	<div class="flex-1 overflow-auto p-6">
		<div class="mx-auto max-w-2xl space-y-6">
			<div>
				<h1 class="text-2xl font-bold">Settings</h1>
				<p class="text-sm text-muted-foreground">
					Configure your qRate preferences
				</p>
			</div>

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
								variant={theme === "light"
									? "default"
									: "outline"}
								size="sm"
								class="flex-1 gap-2"
								onclick={() => handleThemeChange("light")}
							>
								<SunIcon class="size-4" />
								Light
							</Button>
							<Button
								variant={theme === "dark"
									? "default"
									: "outline"}
								size="sm"
								class="flex-1 gap-2"
								onclick={() => handleThemeChange("dark")}
							>
								<MoonIcon class="size-4" />
								Dark
							</Button>
							<Button
								variant={theme === "system"
									? "default"
									: "outline"}
								size="sm"
								class="flex-1 gap-2"
								onclick={() => handleThemeChange("system")}
							>
								<MonitorIcon class="size-4" />
								System
							</Button>
						</div>
					</div>
				</Card.Content>
			</Card.Root>

			<!-- Files Configuration -->
			<Card.Root>
				<Card.Header>
					<Card.Title>Files</Card.Title>
					<Card.Description>
						Configure how files are located and displayed
					</Card.Description>
				</Card.Header>
				<Card.Content class="space-y-4">
					<div class="space-y-2">
						<Label for="files-folder">Default Files Folder</Label>
						<div class="flex gap-2">
							<Input
								id="files-folder"
								bind:value={filesFolder}
								placeholder="Select folder..."
								readonly
							/>
							<Button variant="outline" onclick={browseFilesFolder}>
								<FolderOpenIcon class="size-4" />
							</Button>
						</div>
						<p class="text-xs text-muted-foreground">
							Base folder containing all files referenced in your
							projects
						</p>
					</div>

					<Separator />

					<div class="space-y-2">
						<Label for="file-column">File Column Name</Label>
						<Input
							id="file-column"
							bind:value={fileColumnName}
							placeholder="file"
							onchange={saveSettings}
						/>
						<p class="text-xs text-muted-foreground">
							The column name containing file names in your CSV
							(e.g., "file", "filename", "image")
						</p>
					</div>

					<Separator />

					<div class="space-y-2">
						<Label for="path-pattern">File Path Pattern</Label>
						<Input
							id="path-pattern"
							bind:value={filePathPattern}
							placeholder={"{files_folder}/{file_column}"}
							onchange={saveSettings}
						/>
						<p class="text-xs text-muted-foreground">
							Pattern for locating files. Use
							&#123;files_folder&#125;, &#123;file_column&#125;,
							or any column name in braces.
						</p>
					</div>

					<div class="rounded-md bg-muted p-3">
						<p class="mb-2 text-xs font-medium">Pattern Examples:</p>
						<div class="space-y-1 text-xs text-muted-foreground">
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
			<Card.Root>
				<Card.Header>
					<Card.Title>Data</Card.Title>
					<Card.Description>
						Configure data loading and display settings
					</Card.Description>
				</Card.Header>
				<Card.Content class="space-y-4">
					<div class="space-y-2">
						<Label for="row-limit">Default Row Limit</Label>
						<Input
							id="row-limit"
							type="number"
							bind:value={defaultRowLimit}
							min={50}
							max={1000}
							onchange={saveSettings}
						/>
						<p class="text-xs text-muted-foreground">
							Number of rows to load at a time (50-1000)
						</p>
					</div>
				</Card.Content>
			</Card.Root>

			<!-- About -->
			<Card.Root>
				<Card.Header>
					<Card.Title>About</Card.Title>
				</Card.Header>
				<Card.Content>
					<div class="space-y-2 text-sm">
						<p>
							<span class="font-medium">qRate</span> - Digital
							Archival Workspace
						</p>
						<p class="text-muted-foreground">Version 0.1.0</p>
					</div>
				</Card.Content>
			</Card.Root>
		</div>
	</div>
</div>
