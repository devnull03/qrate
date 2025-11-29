<script lang="ts">
	import { Button } from "$lib/components/ui/button/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import * as Card from "$lib/components/ui/card/index.js";
	import { Skeleton } from "$lib/components/ui/skeleton/index.js";
	import { qrateStore } from "$lib/stores/qrateStore.svelte";
	import { appSettingsStore, resolveFilePath } from "$lib/stores/appSettings";
	import { get } from "svelte/store";
	import FileIcon from "@lucide/svelte/icons/file";
	import FileTextIcon from "@lucide/svelte/icons/file-text";
	import ImageIcon from "@lucide/svelte/icons/image";
	import VideoIcon from "@lucide/svelte/icons/video";
	import MusicIcon from "@lucide/svelte/icons/music";
	import GridIcon from "@lucide/svelte/icons/layout-grid";
	import ListIcon from "@lucide/svelte/icons/list";
	import SearchIcon from "@lucide/svelte/icons/search";
	import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
	import ExternalLinkIcon from "@lucide/svelte/icons/external-link";
	import { openPath, revealItemInDir } from "@tauri-apps/plugin-opener";

	interface FileItem {
		rowId: number;
		fileName: string;
		filePath: string;
		fileType: string;
		rowData: Record<string, any>;
	}

	// View mode state
	let viewMode = $state<"grid" | "list">("grid");

	// Search filter
	let searchQuery = $state("");

	// Settings from store
	let filesFolder = $state("");
	let filePathPattern = $state("{files_folder}/{file_column}");
	let fileColumnName = $state("file");

	// Load settings from store
	$effect(() => {
		const unsubscribe = appSettingsStore.subscribe((settings) => {
			filesFolder = settings.filesFolder || "";
			filePathPattern =
				settings.filePathPattern || "{files_folder}/{file_column}";
			fileColumnName = settings.fileColumnName || "file";
		});
		return unsubscribe;
	});

	// Get all files from all rows
	let allFiles = $derived.by((): FileItem[] => {
		if (!qrateStore.isFileOpen || !filesFolder) return [];

		// Find the file column
		const fileColumn = qrateStore.columns.find(
			(col) =>
				col.name.toLowerCase() === fileColumnName.toLowerCase() ||
				col.id === fileColumnName,
		);

		if (!fileColumn) return [];

		const files: FileItem[] = [];

		for (const row of qrateStore.rows) {
			const fileValue = row[fileColumn.id];
			if (!fileValue) continue;

			const filePath = resolveFilePath(
				filePathPattern,
				filesFolder,
				row,
				fileColumn.id,
			);

			files.push({
				rowId: row.row_id,
				fileName: fileValue,
				filePath: filePath,
				fileType: getFileType(fileValue),
				rowData: row,
			});
		}

		return files;
	});

	// Filtered files based on search
	let filteredFiles = $derived.by(() => {
		if (!searchQuery.trim()) return allFiles;

		const query = searchQuery.toLowerCase();
		return allFiles.filter(
			(file) =>
				file.fileName.toLowerCase().includes(query) ||
				file.filePath.toLowerCase().includes(query),
		);
	});

	// Get file type based on extension
	function getFileType(filename: string): string {
		const ext = filename.split(".").pop()?.toLowerCase() || "";
		const imageExts = ["jpg", "jpeg", "png", "gif", "bmp", "webp", "svg"];
		const videoExts = ["mp4", "webm", "avi", "mov", "mkv"];
		const audioExts = ["mp3", "wav", "ogg", "flac", "aac"];
		const docExts = ["pdf", "doc", "docx", "txt", "md"];

		if (imageExts.includes(ext)) return "image";
		if (videoExts.includes(ext)) return "video";
		if (audioExts.includes(ext)) return "audio";
		if (docExts.includes(ext)) return "document";
		return "file";
	}

	// Get file icon component based on type
	function getFileIcon(type: string) {
		switch (type) {
			case "image":
				return ImageIcon;
			case "video":
				return VideoIcon;
			case "audio":
				return MusicIcon;
			case "document":
				return FileTextIcon;
			default:
				return FileIcon;
		}
	}

	// Open file with default application
	async function openFile(filePath: string) {
		try {
			await openPath(filePath);
		} catch (err) {
			console.error("Failed to open file:", err);
		}
	}

	// Open file location in explorer
	async function openFileLocation(filePath: string) {
		try {
			await revealItemInDir(filePath);
		} catch (err) {
			console.error("Failed to open location:", err);
		}
	}
</script>

<div class="flex h-full flex-col">
	<!-- Header with search and view toggle -->
	<div class="flex items-center gap-3 border-b border-border p-3">
		<div class="relative flex-1">
			<SearchIcon
				class="absolute left-2.5 top-1/2 size-4 -translate-y-1/2 text-muted-foreground"
			/>
			<Input
				bind:value={searchQuery}
				placeholder="Search files..."
				class="h-9 pl-9"
			/>
		</div>
		<div class="flex gap-1">
			<Button
				variant={viewMode === "list" ? "secondary" : "ghost"}
				size="icon-sm"
				onclick={() => (viewMode = "list")}
			>
				<ListIcon class="size-4" />
			</Button>
			<Button
				variant={viewMode === "grid" ? "secondary" : "ghost"}
				size="icon-sm"
				onclick={() => (viewMode = "grid")}
			>
				<GridIcon class="size-4" />
			</Button>
		</div>
	</div>

	<!-- Files content -->
	<div class="min-h-0 flex-1 overflow-y-auto p-3">
		{#if !qrateStore.isFileOpen}
			<div
				class="flex h-full flex-col items-center justify-center gap-3 text-muted-foreground"
			>
				<FileIcon class="size-12 opacity-50" />
				<p class="text-sm">No project open</p>
			</div>
		{:else if !filesFolder}
			<div
				class="flex h-full flex-col items-center justify-center gap-3 text-muted-foreground"
			>
				<FolderOpenIcon class="size-12 opacity-50" />
				<p class="text-sm">No files folder configured</p>
				<p class="text-xs">
					Configure the files folder in the sidebar settings
				</p>
			</div>
		{:else if filteredFiles.length === 0}
			<div
				class="flex h-full flex-col items-center justify-center gap-3 text-muted-foreground"
			>
				<SearchIcon class="size-12 opacity-50" />
				<p class="text-sm">
					{searchQuery
						? "No files match your search"
						: "No files found"}
				</p>
			</div>
		{:else if viewMode === "grid"}
			<!-- Grid View -->
			<div
				class="grid grid-cols-2 gap-3 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6"
			>
				{#each filteredFiles as file (file.rowId + "-" + file.fileName)}
					{@const IconComponent = getFileIcon(file.fileType)}
					<Card.Root
						class="group cursor-pointer transition-colors hover:bg-accent"
					>
						<button
							class="flex w-full flex-col items-center p-4 text-center"
							onclick={() => openFile(file.filePath)}
						>
							<div
								class="mb-3 flex size-16 items-center justify-center rounded-lg bg-muted"
							>
								<IconComponent
									class="size-8 text-muted-foreground"
								/>
							</div>
							<p class="w-full truncate text-sm font-medium">
								{file.fileName}
							</p>
							<p class="text-xs text-muted-foreground">
								Row #{file.rowId}
							</p>
						</button>
						<Card.Footer class="justify-center gap-1 p-2 pt-0">
							<Button
								variant="ghost"
								size="icon-sm"
								class="size-7 opacity-0 transition-opacity group-hover:opacity-100"
								onclick={(e: MouseEvent) => {
									e.stopPropagation();
									openFileLocation(file.filePath);
								}}
								title="Open file location"
							>
								<ExternalLinkIcon class="size-3.5" />
							</Button>
						</Card.Footer>
					</Card.Root>
				{/each}
			</div>
		{:else}
			<!-- List View -->
			<div class="space-y-1">
				{#each filteredFiles as file (file.rowId + "-" + file.fileName)}
					{@const IconComponent = getFileIcon(file.fileType)}
					<button
						class="group flex w-full items-center gap-3 rounded-md p-2 text-left transition-colors hover:bg-accent"
						onclick={() => openFile(file.filePath)}
					>
						<div
							class="flex size-10 shrink-0 items-center justify-center rounded-md bg-muted"
						>
							<IconComponent
								class="size-5 text-muted-foreground"
							/>
						</div>
						<div class="min-w-0 flex-1">
							<p class="truncate text-sm font-medium">
								{file.fileName}
							</p>
							<p class="truncate text-xs text-muted-foreground">
								{file.filePath}
							</p>
						</div>
						<span class="shrink-0 text-xs text-muted-foreground">
							Row #{file.rowId}
						</span>
						<Button
							variant="ghost"
							size="icon-sm"
							class="size-7 shrink-0 opacity-0 transition-opacity group-hover:opacity-100"
							onclick={(e: MouseEvent) => {
								e.stopPropagation();
								openFileLocation(file.filePath);
							}}
							title="Open file location"
						>
							<ExternalLinkIcon class="size-3.5" />
						</Button>
					</button>
				{/each}
			</div>
		{/if}
	</div>

	<!-- Footer with stats -->
	<div
		class="flex items-center justify-between border-t border-border px-3 py-2 text-xs text-muted-foreground"
	>
		<span>
			{filteredFiles.length} file{filteredFiles.length !== 1 ? "s" : ""}
			{searchQuery ? `matching "${searchQuery}"` : ""}
		</span>
		{#if allFiles.length !== filteredFiles.length}
			<span>{allFiles.length} total</span>
		{/if}
	</div>
</div>
