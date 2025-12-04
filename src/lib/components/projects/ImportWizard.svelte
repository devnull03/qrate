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
	let selectedFilesFolder = $state(String(defaultSettings.filesFolder ?? ""));
	let selectedFileColumn = $state(
		String(defaultSettings.fileColumnName ?? "file"),
	);
	let selectedPathPattern = $state(
		String(
			defaultSettings.filePathPattern ?? "{files_folder}/{file_column}",
		),
	);
	let csvHeaders = $state<string[]>([]);
	let csvFirstRow = $state<string[] | null>(null);
	let extensionWarning = $state<string | null>(null);

	const getFileName = (path: string) => path.split(/[/\\]/).pop() || path;

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
			extensionWarning = `The file column "${selectedFileColumn}" value "${fileValue}" does not include a file extension. Consider updating the File Path Pattern to include one, for example: {files_folder}/{file_column}.jpg or {files_folder}/{file}.{extension}`;
		}
	}

	async function selectCsvFile() {
		const csvFile = await open({
			multiple: false,
			filters: [{ name: "CSV Files", extensions: ["csv"] }],
		}).catch((err) => {
			onError(err instanceof Error ? err.message : String(err));
			return null;
		});

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

		importStep = "configure";
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

	async function completeImport() {
		if (!selectedCsvFile) return;

		if (!selectedFilesFolder?.trim()) {
			onError(
				"Files Folder is required. Please select a folder containing your files.",
			);
			return;
		}

		isProcessing = true;

		try {
			const qrateFile = await save({
				filters: [{ name: "Qrate Files", extensions: ["qrate"] }],
				defaultPath: selectedCsvFile.replace(/\.csv$/i, ".qrate"),
			});

			if (!qrateFile) {
				isProcessing = false;
				return;
			}

			await qrateStore.importCsv(qrateFile, selectedCsvFile);
			await saveSettings({
				filesFolder: selectedFilesFolder,
				fileColumnName: selectedFileColumn,
				filePathPattern: selectedPathPattern,
			});
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

	let filesFolderValid = $derived(!!selectedFilesFolder?.trim());
	let fileColumnExists = $derived(
		!csvHeaders.length ||
			csvHeaders.some(
				(h) => h.toLowerCase() === selectedFileColumn.toLowerCase(),
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
					{importStep === "select-csv"
						? "Select a CSV file to import"
						: "Configure file settings"}
				</Card.Description>
			</div>
		</div>
	</Card.Header>
	<Card.Content class="space-y-6">
		{#if importStep === "select-csv"}
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
						<p class="truncate text-xs text-muted-foreground">
							{selectedCsvFile}
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
				{isProcessing ? "Importing..." : "Import CSV"}
			</Button>
		</Card.Footer>
	{/if}
</Card.Root>
