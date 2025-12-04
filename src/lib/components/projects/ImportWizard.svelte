<script lang="ts">
	import { invoke } from "@tauri-apps/api/core";
	import { open, save } from "@tauri-apps/plugin-dialog";
	import { qrateStore } from "$lib/stores/qrateStore.svelte";
	import { saveSettings, defaultSettings } from "$lib/stores/appSettings";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import { Label } from "$lib/components/ui/label/index.js";
	import * as Card from "$lib/components/ui/card/index.js";
	import { Separator } from "$lib/components/ui/separator/index.js";
	import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
	import UploadIcon from "@lucide/svelte/icons/upload";
	import FileIcon from "@lucide/svelte/icons/file";
	import CheckIcon from "@lucide/svelte/icons/check";
	import ArrowLeftIcon from "@lucide/svelte/icons/arrow-left";
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

	let isProcessing = $state(false);
	let importStep = $state<"select-csv" | "configure">("select-csv");
	let selectedCsvFile = $state<string | null>(null);
	let selectedFilesFolder = $state<string>(
		String(defaultSettings.filesFolder ?? ""),
	);
	let selectedFileColumn = $state<string>(
		String(defaultSettings.fileColumnName ?? "file"),
	);
	let selectedPathPattern = $state<string>(
		String(
			defaultSettings.filePathPattern ?? "{files_folder}/{file_column}",
		),
	);

	// CSV preview data
	let csvHeaders = $state<string[]>([]);
	let csvFirstRow = $state<string[] | null>(null);

	// File extension warning
	let extensionWarning = $state<string | null>(null);

	/**
	 * Get filename from path
	 */
	function getFileName(path: string): string {
		return path.split(/[/\\]/).pop() || path;
	}

	/**
	 * Check if a string has a file extension
	 */
	function hasFileExtension(value: string): boolean {
		if (!value) return false;
		// Check if the value ends with a dot followed by 1-5 alphanumeric characters
		const extPattern = /\.[a-zA-Z0-9]{1,5}$/;
		return extPattern.test(value.trim());
	}

	/**
	 * Check if the pattern contains a file extension placeholder or static extension
	 */
	function patternHasExtension(pattern: string): boolean {
		// Check for common extension placeholders like {extension}, {ext}, {file_extension}
		const extPlaceholders = [
			/{extension}/i,
			/{ext}/i,
			/{file_extension}/i,
			/{file_ext}/i,
		];
		if (extPlaceholders.some((p) => p.test(pattern))) {
			return true;
		}

		// Check if pattern ends with a static extension like .jpg, .png, etc.
		const staticExtPattern = /\.[a-zA-Z0-9]{1,5}$/;
		return staticExtPattern.test(pattern);
	}

	/**
	 * Validate file extension in the file column value
	 */
	function validateFileExtension() {
		extensionWarning = null;

		if (!csvFirstRow || csvHeaders.length === 0) return;

		// Find the index of the file column
		const fileColIndex = csvHeaders.findIndex(
			(h) =>
				h.toLowerCase() === selectedFileColumn.toLowerCase() ||
				h === selectedFileColumn,
		);

		if (fileColIndex === -1) {
			// Column not found, but that's a different validation
			return;
		}

		const fileValue = csvFirstRow[fileColIndex];

		// Check if the file column value has an extension
		if (!hasFileExtension(fileValue)) {
			// Check if the pattern provides an extension
			if (!patternHasExtension(selectedPathPattern)) {
				extensionWarning = `The file column "${selectedFileColumn}" value "${fileValue}" does not include a file extension. Consider updating the File Path Pattern to include one, for example: {files_folder}/{file_column}.jpg or {files_folder}/{file}.{extension}`;
			}
		}
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

				// Preview the CSV to get headers and first row
				try {
					const preview = await invoke<CsvPreviewResponse>(
						"preview_csv",
						{
							csvPath: csvFile,
						},
					);
					csvHeaders = preview.headers;
					csvFirstRow = preview.first_row;

					// Auto-detect file column if possible
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
					if (foundCol) {
						selectedFileColumn = foundCol;
					}
				} catch (err) {
					console.error("Failed to preview CSV:", err);
					// Continue anyway, just won't have preview data
				}

				importStep = "configure";
			}
		} catch (err) {
			console.error("Failed to select CSV file:", err);
			onError(err instanceof Error ? err.message : String(err));
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
				// Re-validate when folder changes
				validateFileExtension();
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

		// Validate files folder is set
		if (!selectedFilesFolder || selectedFilesFolder.trim() === "") {
			onError(
				"Files Folder is required. Please select a folder containing your files.",
			);
			return;
		}

		try {
			isProcessing = true;

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

			await onComplete();
		} catch (err) {
			console.error("Failed to import CSV:", err);
			onError(err instanceof Error ? err.message : String(err));
		} finally {
			isProcessing = false;
		}
	}

	// Watch for changes that affect extension validation
	$effect(() => {
		if (selectedFileColumn || selectedPathPattern) {
			validateFileExtension();
		}
	});

	// Computed: Check if files folder is valid
	let filesFolderValid = $derived(
		selectedFilesFolder && selectedFilesFolder.trim() !== "",
	);

	// Computed: Check if file column exists in CSV
	let fileColumnExists = $derived(
		csvHeaders.length === 0 ||
			csvHeaders.some(
				(h) =>
					h.toLowerCase() === selectedFileColumn.toLowerCase() ||
					h === selectedFileColumn,
			),
	);
</script>

<Card.Root>
	<Card.Header>
		<div class="flex items-center gap-3">
			<Button variant="ghost" size="icon-sm" onclick={onCancel}>
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
				<UploadIcon class="size-12 text-muted-foreground" />
				<p class="text-center text-sm text-muted-foreground">
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
					<FileIcon class="size-5 text-muted-foreground" />
					<div class="min-w-0 flex-1">
						<p class="truncate text-sm font-medium">
							{selectedCsvFile
								? getFileName(selectedCsvFile)
								: ""}
						</p>
						<p class="truncate text-xs text-muted-foreground">
							{selectedCsvFile}
						</p>
					</div>
					<CheckIcon class="size-5 text-green-500" />
				</div>

				<Separator />

				<!-- Files Folder (Required) -->
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
							class={!filesFolderValid
								? "border-destructive"
								: ""}
						/>
						<Button variant="outline" onclick={browseFilesFolder}>
							<FolderOpenIcon class="size-4" />
						</Button>
					</div>
					<p class="text-xs text-muted-foreground">
						The folder containing files referenced in your CSV
						(required)
					</p>
					{#if !filesFolderValid}
						<p class="text-xs text-destructive">
							Files folder is required
						</p>
					{/if}
				</div>

				<!-- File Column Name -->
				<div class="space-y-2">
					<Label for="file-column">File Column Name</Label>
					<Input
						id="file-column"
						bind:value={selectedFileColumn}
						placeholder="file"
						class={!fileColumnExists ? "border-destructive" : ""}
					/>
					<p class="text-xs text-muted-foreground">
						The CSV column containing file names (e.g., "file",
						"filename", "image")
					</p>
					{#if csvHeaders.length > 0}
						<p class="text-xs text-muted-foreground">
							Available columns: {csvHeaders.join(", ")}
						</p>
					{/if}
					{#if !fileColumnExists}
						<p class="text-xs text-destructive">
							Column "{selectedFileColumn}" not found in CSV
						</p>
					{/if}
				</div>

				<!-- Path Pattern -->
				<div class="space-y-2">
					<Label for="path-pattern">File Path Pattern</Label>
					<Input
						id="path-pattern"
						bind:value={selectedPathPattern}
						placeholder={"{files_folder}/{file_column}"}
					/>
					<p class="text-xs text-muted-foreground">
						Pattern for locating files. Use
						&#123;files_folder&#125;, &#123;file_column&#125;, or
						any column name.
					</p>
				</div>

				<!-- File Extension Warning -->
				{#if extensionWarning}
					<div
						class="flex items-start gap-3 rounded-md border border-amber-500/50 bg-amber-500/10 p-3"
					>
						<AlertTriangleIcon
							class="mt-0.5 size-5 shrink-0 text-amber-500"
						/>
						<div class="space-y-1">
							<p
								class="text-sm font-medium text-amber-700 dark:text-amber-400"
							>
								File Extension Warning
							</p>
							<p
								class="text-xs text-amber-600 dark:text-amber-300"
							>
								{extensionWarning}
							</p>
						</div>
					</div>
				{/if}
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
				disabled={isProcessing ||
					!filesFolderValid ||
					!fileColumnExists}
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
