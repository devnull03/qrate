<script lang="ts">
	import { onMount } from "svelte";
	import { Button } from "$lib/components/ui/button/index.js";
	import { qrateStore } from "$lib/stores/qrateStore.svelte";
	import {
		loadSettings,
		subscribeToSettings,
		resolveFilePath,
		defaultSettings,
	} from "$lib/stores/appSettings";
	import FileIcon from "@lucide/svelte/icons/file";
	import FileTextIcon from "@lucide/svelte/icons/file-text";
	import ImageIcon from "@lucide/svelte/icons/image";
	import VideoIcon from "@lucide/svelte/icons/video";
	import MusicIcon from "@lucide/svelte/icons/music";
	import ExternalLinkIcon from "@lucide/svelte/icons/external-link";
	import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
	import RowsIcon from "@lucide/svelte/icons/rows-3";
	import { openPath, revealItemInDir } from "@tauri-apps/plugin-opener";

	interface FileItem {
		fileName: string;
		filePath: string;
		fileType: string;
	}

	let filesFolder = $state(String(defaultSettings.filesFolder || ""));
	let filePathPattern = $state(
		String(
			defaultSettings.filePathPattern || "{files_folder}/{file_column}",
		),
	);
	let fileColumnName = $state(
		String(defaultSettings.fileColumnName || "file"),
	);

	const fileTypeMap: Record<string, string[]> = {
		image: ["jpg", "jpeg", "png", "gif", "bmp", "webp", "svg"],
		video: ["mp4", "webm", "avi", "mov", "mkv"],
		audio: ["mp3", "wav", "ogg", "flac", "aac"],
		document: ["pdf", "doc", "docx", "txt", "md"],
	};

	const iconMap: Record<string, typeof FileIcon> = {
		image: ImageIcon,
		video: VideoIcon,
		audio: MusicIcon,
		document: FileTextIcon,
		file: FileIcon,
	};

	onMount(() => {
		if (qrateStore.isFileOpen) loadSettings();

		return subscribeToSettings((settings) => {
			filesFolder = String(settings.filesFolder || "");
			filePathPattern = String(
				settings.filePathPattern || "{files_folder}/{file_column}",
			);
			fileColumnName = String(settings.fileColumnName || "file");
		});
	});

	$effect(() => {
		if (qrateStore.isFileOpen && qrateStore.currentFilePath) loadSettings();
	});

	let selectedRow = $derived(
		qrateStore.selectedRowId !== null
			? qrateStore.rows.find(
					(row) => row.row_id === qrateStore.selectedRowId,
				)
			: null,
	);

	let rowFiles = $derived.by((): FileItem[] => {
		if (!selectedRow || !filesFolder || !fileColumnName) return [];

		const colName = String(fileColumnName).toLowerCase();
		const fileColumn = qrateStore.columns.find(
			(col) =>
				col.name.toLowerCase() === colName || col.id === fileColumnName,
		);
		if (!fileColumn) return [];

		const fileValue = selectedRow[fileColumn.id];
		if (!fileValue) return [];

		const fileName = String(fileValue);
		return [
			{
				fileName,
				filePath: resolveFilePath(
					filePathPattern || "",
					filesFolder,
					selectedRow,
					fileColumn.id,
				),
				fileType: getFileType(fileName),
			},
		];
	});

	let rowFields = $derived(
		selectedRow
			? qrateStore.columns
					.filter((col) => !col.hidden && col.id !== "_rowNum")
					.map((col) => ({
						id: col.id,
						name: col.name,
						value: selectedRow[col.id],
					}))
			: [],
	);

	function getFileType(filename: string): string {
		const ext = filename.split(".").pop()?.toLowerCase() || "";
		return (
			Object.entries(fileTypeMap).find(([_, exts]) =>
				exts.includes(ext),
			)?.[0] || "file"
		);
	}

	async function openFile(filePath: string) {
		await openPath(filePath).catch((err) =>
			console.error("Failed to open file:", err),
		);
	}

	async function openFileLocation(filePath: string) {
		await revealItemInDir(filePath).catch((err) =>
			console.error("Failed to open location:", err),
		);
	}
</script>

<div class="flex h-full flex-col overflow-hidden bg-background">
	<div
		class="flex items-center gap-2 border-b border-border px-3 py-2 text-sm font-medium"
	>
		<RowsIcon class="size-4 text-muted-foreground" />
		{#if qrateStore.selectedRowId !== null}
			<span>Row #{qrateStore.selectedRowId}</span>
		{:else}
			<span class="text-muted-foreground">No row selected</span>
		{/if}
	</div>

	<div class="min-h-0 flex-1 overflow-y-auto">
		{#if qrateStore.selectedRowId === null}
			<div
				class="flex h-full flex-col items-center justify-center gap-3 p-4 text-muted-foreground"
			>
				<RowsIcon class="size-12 opacity-50" />
				<p class="text-sm">Select a row to view details</p>
			</div>
		{:else if !filesFolder}
			<div
				class="flex h-full flex-col items-center justify-center gap-3 p-4 text-muted-foreground"
			>
				<FolderOpenIcon class="size-12 opacity-50" />
				<p class="text-sm">No files folder configured</p>
				<p class="text-xs">Configure in View → Settings</p>
			</div>
		{:else}
			{#if rowFiles.length > 0}
				<div class="border-b border-border p-3">
					<h3
						class="mb-2 text-xs font-medium uppercase text-muted-foreground"
					>
						Files
					</h3>
					<div class="space-y-1">
						{#each rowFiles as file}
							{@const IconComponent =
								iconMap[file.fileType] || FileIcon}
							<div
								class="group flex items-center gap-2 rounded-md p-2 transition-colors hover:bg-accent"
							>
								<button
									class="flex min-w-0 flex-1 items-center gap-2 text-left"
									onclick={() => openFile(file.filePath)}
								>
									<div
										class="flex size-8 shrink-0 items-center justify-center rounded bg-muted"
									>
										<IconComponent
											class="size-4 text-muted-foreground"
										/>
									</div>
									<div class="min-w-0 flex-1">
										<p class="truncate text-sm font-medium">
											{file.fileName}
										</p>
										<p
											class="truncate text-xs text-muted-foreground"
										>
											{file.filePath}
										</p>
									</div>
								</button>
								<Button
									variant="ghost"
									size="icon-sm"
									class="size-7 shrink-0 opacity-0 transition-opacity group-hover:opacity-100"
									onclick={() =>
										openFileLocation(file.filePath)}
									title="Open file location"
								>
									<ExternalLinkIcon class="size-3.5" />
								</Button>
							</div>
						{/each}
					</div>
				</div>
			{/if}

			<div class="p-3">
				<h3
					class="mb-2 text-xs font-medium uppercase text-muted-foreground"
				>
					Row Data
				</h3>
				<div class="space-y-2">
					{#each rowFields as field}
						<div class="rounded-md bg-muted/50 p-2">
							<div
								class="mb-0.5 text-xs font-medium text-muted-foreground"
							>
								{field.name}
							</div>
							<div class="wrap-break-word text-sm">
								{#if field.value !== null && field.value !== undefined && field.value !== ""}
									{field.value}
								{:else}
									<span class="italic text-muted-foreground"
										>Empty</span
									>
								{/if}
							</div>
						</div>
					{/each}
				</div>
			</div>
		{/if}
	</div>
</div>
