<script lang="ts">
	import { invoke } from "@tauri-apps/api/core";
	import { join } from "@tauri-apps/api/path";
	import { open } from "@tauri-apps/plugin-dialog";
	import { qrateStore } from "$lib/stores/qrateStore.svelte";
	import { saveSettings, defaultSettings } from "$lib/stores/appSettings";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import { Label } from "$lib/components/ui/label/index.js";
	import * as Card from "$lib/components/ui/card/index.js";
	import * as RadioGroup from "$lib/components/ui/radio-group/index.js";
	import { Separator } from "$lib/components/ui/separator/index.js";
	import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
	import UploadIcon from "@lucide/svelte/icons/upload";
	import FileIcon from "@lucide/svelte/icons/file";
	import ImageIcon from "@lucide/svelte/icons/image";
	import CheckIcon from "@lucide/svelte/icons/check";
	import ArrowLeftIcon from "@lucide/svelte/icons/arrow-left";
	import ArrowRightIcon from "@lucide/svelte/icons/arrow-right";
	import LinkIcon from "@lucide/svelte/icons/link";
	import CopyIcon from "@lucide/svelte/icons/copy";
	import AlertTriangleIcon from "@lucide/svelte/icons/triangle-alert";

	interface CsvPreviewResponse {
		headers: string[];
		first_row: string[] | null;
	}

	interface Props {
		onComplete: () => Promise<void>;
		onCancel: () => void;
		onError: (error: string) => void;
	}

	let { onComplete, onCancel, onError }: Props = $props();

	type Step = "name-location" | "source" | "configure" | "mode";
	type SourceType = "csv" | "images" | "blank";

	let isProcessing = $state(false);
	let currentStep = $state<Step>("name-location");

	// Step 1: Name & Location
	let projectName = $state("");
	let projectLocation = $state("");

	// Step 2: Source
	let sourceType = $state<SourceType>("csv");
	let selectedCsvFile = $state<string | null>(null);
	let selectedImagesFolder = $state<string | null>(null);
	let csvHeaders = $state<string[]>([]);
	let csvFirstRow = $state<string[] | null>(null);

	// Step 3: Configure (for CSV)
	let selectedFilesFolder = $state(String(defaultSettings.filesFolder ?? ""));
	let selectedFileColumn = $state(
		String(defaultSettings.fileColumnName ?? "file"),
	);
	let selectedPathPattern = $state(
		String(
			defaultSettings.filePathPattern ?? "{files_folder}/{file_column}",
		),
	);
	let extensionWarning = $state<string | null>(null);

	// Step 4: Mode
	let projectMode = $state<"reference" | "copy">("reference");

	const getFileName = (path: string) => path.split(/[/\\]/).pop() || path;
	const getFolderName = (path: string) => path.split(/[/\\]/).pop() || path;

	const hasFileExtension = (value: string) =>
		value ? /\.[a-zA-Z0-9]{1,5}$/.test(value.trim()) : false;

	const patternHasExtension = (pattern: string) =>
		[/{extension}/i, /{ext}/i, /{file_extension}/i, /{file_ext}/i].some(
			(p) => p.test(pattern),
		) || /\.[a-zA-Z0-9]{1,5}$/.test(pattern);

	function validateFileExtension() {
		extensionWarning = null;
		if (!csvFirstRow || !csvHeaders.length) return;

		const fileColIndex = csvHeaders.findIndex(
			(h) => h.toLowerCase() === selectedFileColumn.toLowerCase(),
		);
		if (fileColIndex === -1) return;

		const fileValue = csvFirstRow[fileColIndex];
		if (
			!hasFileExtension(fileValue) &&
			!patternHasExtension(selectedPathPattern)
		) {
			extensionWarning = `The file column "${selectedFileColumn}" value "${fileValue}" does not include a file extension.`;
		}
	}

	async function selectProjectLocation() {
		const folder = await open({
			directory: true,
			multiple: false,
			title: "Select project location",
		}).catch(() => null);

		if (folder && typeof folder === "string") {
			projectLocation = folder;
			if (!projectName) {
				projectName = "New Project";
			}
		}
	}

	async function selectCsvFile() {
		const csvFile = await open({
			multiple: false,
			filters: [{ name: "CSV Files", extensions: ["csv"] }],
		}).catch(() => null);

		if (!csvFile || typeof csvFile !== "string") return;

		selectedCsvFile = csvFile;

		const preview = await invoke<CsvPreviewResponse>("preview_csv", {
			csvPath: csvFile,
		}).catch(() => null);

		if (preview) {
			csvHeaders = preview.headers;
			csvFirstRow = preview.first_row;

			const possibleFileColumns = [
				"file",
				"filename",
				"image",
				"path",
				"file_name",
				"filepath",
			];
			const foundCol = csvHeaders.find((h) =>
				possibleFileColumns.includes(h.toLowerCase()),
			);
			if (foundCol) selectedFileColumn = foundCol;
		}
	}

	async function selectImagesFolder() {
		const folder = await open({
			directory: true,
			multiple: false,
			title: "Select images folder",
		}).catch(() => null);

		if (folder && typeof folder === "string") {
			selectedImagesFolder = folder;
			selectedFilesFolder = folder;
		}
	}

	async function browseFilesFolder() {
		const folder = await open({
			directory: true,
			multiple: false,
			title: "Select Files Folder",
		}).catch(() => null);

		if (folder && typeof folder === "string") {
			selectedFilesFolder = folder;
			validateFileExtension();
		}
	}

	function goToNextStep() {
		if (currentStep === "name-location") {
			currentStep = "source";
		} else if (currentStep === "source") {
			if (sourceType === "csv" && selectedCsvFile) {
				currentStep = "configure";
			} else if (sourceType === "images" && selectedImagesFolder) {
				currentStep = "mode";
			} else if (sourceType === "blank") {
				currentStep = "mode";
			}
		} else if (currentStep === "configure") {
			currentStep = "mode";
		}
	}

	function goToPrevStep() {
		if (currentStep === "source") {
			currentStep = "name-location";
		} else if (currentStep === "configure") {
			currentStep = "source";
		} else if (currentStep === "mode") {
			if (sourceType === "csv") {
				currentStep = "configure";
			} else {
				currentStep = "source";
			}
		}
	}

	async function completeWizard() {
		if (!projectLocation || !projectName) {
			onError("Project name and location are required.");
			return;
		}

		isProcessing = true;

		try {
			const projectPath = await join(projectLocation, projectName);
			const imagesRoot =
				sourceType === "images"
					? selectedImagesFolder
					: selectedFilesFolder || null;

			if (sourceType === "csv" && selectedCsvFile) {
				await qrateStore.importCsv(projectPath, selectedCsvFile);
				await saveSettings({
					filesFolder: selectedFilesFolder,
					fileColumnName: selectedFileColumn,
					filePathPattern: selectedPathPattern,
				});
			} else {
				await qrateStore.createFile(projectPath, {
					name: projectName,
					mode: projectMode,
					imagesRoot: imagesRoot ?? undefined,
				});

				if (imagesRoot) {
					await saveSettings({
						filesFolder: imagesRoot,
					});
				}
			}

			await onComplete();
		} catch (err) {
			onError(err instanceof Error ? err.message : String(err));
		} finally {
			isProcessing = false;
		}
	}

	$effect(() => {
		if (selectedFileColumn || selectedPathPattern) validateFileExtension();
	});

	let canProceedFromNameLocation = $derived(
		!!projectName.trim() && !!projectLocation,
	);
	let canProceedFromSource = $derived(
		sourceType === "blank" ||
			(sourceType === "csv" && !!selectedCsvFile) ||
			(sourceType === "images" && !!selectedImagesFolder),
	);
	let canProceedFromConfigure = $derived(
		!!selectedFilesFolder?.trim() &&
			csvHeaders.some(
				(h) => h.toLowerCase() === selectedFileColumn.toLowerCase(),
			),
	);

	const stepTitles: Record<Step, string> = {
		"name-location": "Name & Location",
		source: "Select Source",
		configure: "Configure Import",
		mode: "Project Mode",
	};

	const stepDescriptions: Record<Step, string> = {
		"name-location": "Choose a name and location for your project",
		source: "Select how to populate your project",
		configure: "Configure file mapping settings",
		mode: "Choose how files are stored",
	};
</script>

<Card.Root>
	<Card.Header>
		<div class="flex items-center gap-3">
			<Button variant="ghost" size="icon-sm" onclick={onCancel}>
				<ArrowLeftIcon class="size-4" />
			</Button>
			<div>
				<Card.Title>{stepTitles[currentStep]}</Card.Title>
				<Card.Description>
					{stepDescriptions[currentStep]}
				</Card.Description>
			</div>
		</div>
	</Card.Header>
	<Card.Content class="space-y-6">
		{#if currentStep === "name-location"}
			<div class="space-y-4">
				<div class="space-y-2">
					<Label for="project-name">Project Name</Label>
					<Input
						id="project-name"
						bind:value={projectName}
						placeholder="My Dataset"
					/>
				</div>

				<div class="space-y-2">
					<Label for="project-location">Location</Label>
					<div class="flex gap-2">
						<Input
							id="project-location"
							value={projectLocation}
							placeholder="Select folder..."
							readonly
						/>
						<Button
							variant="outline"
							onclick={selectProjectLocation}
						>
							<FolderOpenIcon class="size-4" />
						</Button>
					</div>
					<p class="text-xs text-muted-foreground">
						A new folder named "{projectName || "project"}" will be
						created here
					</p>
				</div>
			</div>
		{:else if currentStep === "source"}
			<RadioGroup.Root bind:value={sourceType} class="space-y-3">
				<label
					class="flex cursor-pointer items-start gap-4 rounded-lg border p-4 transition-colors hover:bg-muted/50 {sourceType ===
					'csv'
						? 'border-primary bg-primary/5'
						: ''}"
				>
					<RadioGroup.Item value="csv" class="mt-1" />
					<div class="flex-1 space-y-1">
						<div class="flex items-center gap-2">
							<UploadIcon class="size-5" />
							<span class="font-medium">Import from CSV</span>
						</div>
						<p class="text-sm text-muted-foreground">
							Import metadata from an existing CSV file
						</p>
						{#if sourceType === "csv"}
							<div class="mt-3">
								{#if selectedCsvFile}
									<div
										class="flex items-center gap-2 rounded border bg-muted/50 p-2"
									>
										<FileIcon
											class="size-4 text-muted-foreground"
										/>
										<span class="flex-1 truncate text-sm"
											>{getFileName(
												selectedCsvFile,
											)}</span
										>
										<CheckIcon
											class="size-4 text-green-500"
										/>
									</div>
								{/if}
								<Button
									variant="outline"
									size="sm"
									class="mt-2"
									onclick={selectCsvFile}
								>
									{selectedCsvFile
										? "Change CSV"
										: "Select CSV"}
								</Button>
							</div>
						{/if}
					</div>
				</label>

				<label
					class="flex cursor-pointer items-start gap-4 rounded-lg border p-4 transition-colors hover:bg-muted/50 {sourceType ===
					'images'
						? 'border-primary bg-primary/5'
						: ''}"
				>
					<RadioGroup.Item value="images" class="mt-1" />
					<div class="flex-1 space-y-1">
						<div class="flex items-center gap-2">
							<ImageIcon class="size-5" />
							<span class="font-medium">From Images Folder</span>
						</div>
						<p class="text-sm text-muted-foreground">
							Start with an existing folder of images
						</p>
						{#if sourceType === "images"}
							<div class="mt-3">
								{#if selectedImagesFolder}
									<div
										class="flex items-center gap-2 rounded border bg-muted/50 p-2"
									>
										<FolderOpenIcon
											class="size-4 text-muted-foreground"
										/>
										<span class="flex-1 truncate text-sm"
											>{getFolderName(
												selectedImagesFolder,
											)}</span
										>
										<CheckIcon
											class="size-4 text-green-500"
										/>
									</div>
								{/if}
								<Button
									variant="outline"
									size="sm"
									class="mt-2"
									onclick={selectImagesFolder}
								>
									{selectedImagesFolder
										? "Change Folder"
										: "Select Folder"}
								</Button>
							</div>
						{/if}
					</div>
				</label>

				<label
					class="flex cursor-pointer items-start gap-4 rounded-lg border p-4 transition-colors hover:bg-muted/50 {sourceType ===
					'blank'
						? 'border-primary bg-primary/5'
						: ''}"
				>
					<RadioGroup.Item value="blank" class="mt-1" />
					<div class="flex-1 space-y-1">
						<div class="flex items-center gap-2">
							<FileIcon class="size-5" />
							<span class="font-medium">Blank Project</span>
						</div>
						<p class="text-sm text-muted-foreground">
							Start with an empty project
						</p>
					</div>
				</label>
			</RadioGroup.Root>
		{:else if currentStep === "configure"}
			<div class="space-y-4">
				<div
					class="flex items-center gap-3 rounded-md border bg-muted/50 p-3"
				>
					<FileIcon class="size-5 text-muted-foreground" />
					<div class="min-w-0 flex-1">
						<p class="truncate text-sm font-medium">
							{selectedCsvFile
								? getFileName(selectedCsvFile)
								: ""}
						</p>
					</div>
					<CheckIcon class="size-5 text-green-500" />
				</div>

				<Separator />

				<div class="space-y-2">
					<Label for="files-folder">
						Files Folder <span class="text-destructive">*</span>
					</Label>
					<div class="flex gap-2">
						<Input
							id="files-folder"
							bind:value={selectedFilesFolder}
							placeholder="Select folder containing files..."
							readonly
						/>
						<Button variant="outline" onclick={browseFilesFolder}>
							<FolderOpenIcon class="size-4" />
						</Button>
					</div>
					<p class="text-xs text-muted-foreground">
						The folder containing files referenced in your CSV
					</p>
				</div>

				<div class="space-y-2">
					<Label for="file-column">File Column Name</Label>
					<Input
						id="file-column"
						bind:value={selectedFileColumn}
						placeholder="file"
					/>
					{#if csvHeaders.length > 0}
						<p class="text-xs text-muted-foreground">
							Available: {csvHeaders.join(", ")}
						</p>
					{/if}
				</div>

				<div class="space-y-2">
					<Label for="path-pattern">File Path Pattern</Label>
					<Input
						id="path-pattern"
						bind:value={selectedPathPattern}
						placeholder={"{files_folder}/{file_column}"}
					/>
					<p class="text-xs text-muted-foreground">
						Use &#123;files_folder&#125;, &#123;file_column&#125;,
						or column names
					</p>
				</div>

				{#if extensionWarning}
					<div
						class="flex items-start gap-3 rounded-md border border-amber-500/50 bg-amber-500/10 p-3"
					>
						<AlertTriangleIcon
							class="mt-0.5 size-5 shrink-0 text-amber-500"
						/>
						<p class="text-xs text-amber-600 dark:text-amber-300">
							{extensionWarning}
						</p>
					</div>
				{/if}
			</div>
		{:else if currentStep === "mode"}
			<RadioGroup.Root bind:value={projectMode} class="space-y-3">
				<label
					class="flex cursor-pointer items-start gap-4 rounded-lg border p-4 transition-colors hover:bg-muted/50 {projectMode ===
					'reference'
						? 'border-primary bg-primary/5'
						: ''}"
				>
					<RadioGroup.Item value="reference" class="mt-1" />
					<div class="flex-1 space-y-1">
						<div class="flex items-center gap-2">
							<LinkIcon class="size-5" />
							<span class="font-medium">Reference Mode</span>
						</div>
						<p class="text-sm text-muted-foreground">
							Link to files at their current location. Instant
							setup, no disk duplication, but files must stay in
							place.
						</p>
					</div>
				</label>

				<label
					class="flex cursor-pointer items-start gap-4 rounded-lg border p-4 transition-colors hover:bg-muted/50 {projectMode ===
					'copy'
						? 'border-primary bg-primary/5'
						: ''}"
				>
					<RadioGroup.Item value="copy" class="mt-1" />
					<div class="flex-1 space-y-1">
						<div class="flex items-center gap-2">
							<CopyIcon class="size-5" />
							<span class="font-medium">Copy Mode</span>
						</div>
						<p class="text-sm text-muted-foreground">
							Copy files into the project folder. Self-contained
							and portable, but uses more disk space.
						</p>
					</div>
				</label>
			</RadioGroup.Root>
		{/if}
	</Card.Content>
	<Card.Footer class="flex justify-between">
		<Button
			variant="outline"
			onclick={currentStep === "name-location" ? onCancel : goToPrevStep}
		>
			{currentStep === "name-location" ? "Cancel" : "Back"}
		</Button>

		{#if currentStep === "mode"}
			<Button onclick={completeWizard} disabled={isProcessing}>
				{isProcessing ? "Creating..." : "Create Project"}
			</Button>
		{:else}
			<Button
				onclick={goToNextStep}
				disabled={(currentStep === "name-location" &&
					!canProceedFromNameLocation) ||
					(currentStep === "source" && !canProceedFromSource) ||
					(currentStep === "configure" && !canProceedFromConfigure)}
			>
				Next
				<ArrowRightIcon class="ml-2 size-4" />
			</Button>
		{/if}
	</Card.Footer>
</Card.Root>
