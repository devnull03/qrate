<script lang="ts">
	import { onMount, onDestroy } from "svelte";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import * as Card from "$lib/components/ui/card/index.js";
	import { qrateStore } from "$lib/stores/qrateStore.svelte";
	import { thumbnailService } from "$lib/services/thumbnails";
	import {
		loadSettings,
		subscribeToSettings,
		resolveFilePath,
		defaultSettings,
	} from "$lib/stores/appSettings";
	import { getThumbnailUrl } from "$lib/utils";

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
	import LoaderCircleIcon from "@lucide/svelte/icons/loader-circle";
	import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
	import { openPath, revealItemInDir } from "@tauri-apps/plugin-opener";

	interface FileItem {
		rowId: number;
		fileName: string;
		filePath: string;
		fileType: string;
	}

	let viewMode = $state<"grid" | "list">("grid");
	let searchQuery = $state("");
	let refreshKey = $state(0);
	let thumbnailUrls = $state<Record<string, string>>({});
	let thumbnailStatus = $state<Record<string, "loading" | "failed">>({});

	$effect(() => {
		qrateStore.filesGridSearchQuery = searchQuery;
	});

	let filesFolder = $state(defaultSettings.filesFolder);
	let filePathPattern = $state(defaultSettings.filePathPattern);
	let fileColumnName = $state(defaultSettings.fileColumnName);

	const thumbnailExts = [
		"jpg",
		"jpeg",
		"png",
		"gif",
		"bmp",
		"webp",
		"tiff",
		"tif",
	];

	let settingsUnsubscribe: (() => void) | null = null;

	onMount(() => {
		if (qrateStore.isFileOpen) {
			loadSettings();
		}

		settingsUnsubscribe = subscribeToSettings(async (settings) => {
			const newFilesFolder = String(
				settings.filesFolder || defaultSettings.filesFolder,
			);
			const folderChanged = filesFolder !== newFilesFolder;

			filesFolder = newFilesFolder;
			filePathPattern =
				settings.filePathPattern || defaultSettings.filePathPattern;
			fileColumnName =
				settings.fileColumnName || defaultSettings.fileColumnName;

			if (folderChanged && newFilesFolder) {
				await thumbnailService.startProcessing(newFilesFolder);
			}
		});

		return () => settingsUnsubscribe?.();
	});

	onDestroy(() => {
		thumbnailService.dispose();
	});

	$effect(() => {
		if (qrateStore.isFileOpen && qrateStore.currentFilePath) {
			loadSettings();
		}
	});

	$effect(() => {
		if (qrateStore.isFileOpen && filesFolder) {
			thumbnailService.startProcessing(filesFolder as string);
		}
	});

	$effect(() => {
		if (filteredFiles.length > 0) {
			loadThumbnailUrls();
		}
	});

	async function loadThumbnailUrls() {
		const imagePaths = filteredFiles
			.filter(
				(f) =>
					supportsThumbnail(f.fileName) && !thumbnailUrls[f.filePath],
			)
			.map((f) => f.filePath);

		if (imagePaths.length === 0) return;

		const results = await Promise.all(
			imagePaths.map(async (filePath) => {
				const url = await getThumbnailUrl(filePath);
				return { filePath, url };
			}),
		);

		const newUrls: Record<string, string> = {};
		for (const { filePath, url } of results) {
			newUrls[filePath] = url;
		}
		thumbnailUrls = { ...thumbnailUrls, ...newUrls };
	}

	function handleThumbnailError(filePath: string) {
		if (thumbnailService.isProcessing) {
			thumbnailStatus = { ...thumbnailStatus, [filePath]: "loading" };
		} else {
			thumbnailStatus = { ...thumbnailStatus, [filePath]: "failed" };
		}
	}

	let allFiles = $derived.by((): FileItem[] => {
		if (!qrateStore.isFileOpen || !filesFolder) return [];

		const fileColumn = qrateStore.columns.find(
			(col) =>
				col.name.toLowerCase() ===
					(fileColumnName as string).toLowerCase() ||
				col.id === fileColumnName,
		);

		if (!fileColumn) return [];

		const files: FileItem[] = [];

		for (const row of qrateStore.rows) {
			const fileValue = row[fileColumn.id];
			if (!fileValue) continue;

			const filePath = resolveFilePath(
				filePathPattern as string,
				filesFolder as string,
				row,
				fileColumn.id,
			);

			files.push({
				rowId: row.row_id,
				fileName: fileValue,
				filePath: filePath,
				fileType: getFileType(filePath),
			});
		}

		return files;
	});

	let filteredFiles = $derived.by(() => {
		if (!searchQuery.trim()) return allFiles;

		const query = searchQuery.toLowerCase();
		return allFiles.filter(
			(file) =>
				file.fileName.toLowerCase().includes(query) ||
				file.filePath.toLowerCase().includes(query),
		);
	});

	$effect(() => {
		qrateStore.filesGridFilteredCount = filteredFiles.length;
		qrateStore.filesGridTotalCount = allFiles.length;
	});

	function getFileType(filename: string): string {
		const ext = filename.split(".").pop()?.toLowerCase() || "";
		const imageExts = [
			"jpg",
			"jpeg",
			"png",
			"gif",
			"bmp",
			"webp",
			"svg",
			"tiff",
			"tif",
		];
		const videoExts = ["mp4", "webm", "avi", "mov", "mkv"];
		const audioExts = ["mp3", "wav", "ogg", "flac", "aac"];
		const docExts = ["pdf", "doc", "docx", "txt", "md"];

		if (imageExts.includes(ext)) return "image";
		if (videoExts.includes(ext)) return "video";
		if (audioExts.includes(ext)) return "audio";
		if (docExts.includes(ext)) return "document";
		return "file";
	}

	function supportsThumbnail(filename: string): boolean {
		const ext = filename.split(".").pop()?.toLowerCase() || "";
		return thumbnailExts.includes(ext);
	}

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

	function getThumbnailUrlForFile(filePath: string): string | null {
		const status = thumbnailStatus[filePath];
		if (status === "failed") return null;
		const url = thumbnailUrls[filePath];
		return url ? `${url}?v=${refreshKey}` : null;
	}

	function isLoadingThumbnail(filePath: string): boolean {
		return (
			thumbnailStatus[filePath] === "loading" ||
			(thumbnailService.isProcessing &&
				supportsThumbnail(filePath.split("/").pop() || ""))
		);
	}

	async function openFile(filePath: string) {
		await openPath(filePath);
	}

	async function openFileLocation(filePath: string) {
		await revealItemInDir(filePath);
	}

	function refresh() {
		refreshKey++;
		thumbnailUrls = {};
		thumbnailStatus = {};
		if (filesFolder) {
			thumbnailService.startProcessing(filesFolder as string);
		}
	}
</script>

<div class="flex h-full flex-col">
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
				variant="ghost"
				size="icon-sm"
				onclick={refresh}
				title="Refresh"
			>
				<RefreshCwIcon class="size-4" />
			</Button>
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
					Configure the files folder in View → Settings
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
			<div
				class="grid grid-cols-2 gap-3 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6"
			>
				{#each filteredFiles as file (file.rowId + "-" + file.fileName + "-" + refreshKey)}
					{@const IconComponent = getFileIcon(file.fileType)}
					<!-- {@const thumbnailUrl = } -->
					<Card.Root
						class="group cursor-pointer transition-colors hover:bg-accent"
					>
						<button
							class="flex w-full flex-col items-center p-4 text-center"
							onclick={() => openFile(file.filePath)}
						>
							<div
								class="relative mb-3 flex size-16 items-center justify-center overflow-hidden rounded-lg bg-muted"
							>
								{#await getThumbnailUrl(file.filePath)}
									<LoaderCircleIcon
										class="size-6 animate-spin text-muted-foreground"
									/>
								{:then thumbnailUrl}
									{#if thumbnailStatus[file.filePath] !== "failed"}
										<img
											src={thumbnailUrl}
											alt={file.fileName}
											class="size-full object-cover"
											loading="lazy"
											decoding="async"
											onerror={() =>
												handleThumbnailError(
													file.filePath,
												)}
										/>
									{:else}
										<IconComponent
											class="size-8 text-muted-foreground"
										/>
									{/if}
								{:catch}
									<IconComponent
										class="size-8 text-muted-foreground"
									/>
								{/await}
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
			<div class="space-y-1">
				{#each filteredFiles as file (file.rowId + "-" + file.fileName + "-" + refreshKey)}
					{@const IconComponent = getFileIcon(file.fileType)}
					{@const thumbnailUrl = supportsThumbnail(file.fileName)
						? getThumbnailUrlForFile(file.filePath)
						: null}
					{@const isLoading = isLoadingThumbnail(file.filePath)}
					<button
						class="group flex w-full items-center gap-3 rounded-md p-2 text-left transition-colors hover:bg-accent"
						onclick={() => openFile(file.filePath)}
					>
						<div
							class="relative flex size-10 shrink-0 items-center justify-center overflow-hidden rounded-md bg-muted"
						>
							{#if thumbnailUrl}
								<!-- {#if file} -->
								<img
									src={thumbnailUrl}
									alt={file.fileName}
									class="size-full object-cover"
									loading="lazy"
									decoding="async"
									onerror={() =>
										handleThumbnailError(file.filePath)}
								/>
							{:else if isLoading}
								<LoaderCircleIcon
									class="size-4 animate-spin text-muted-foreground"
								/>
							{:else}
								<IconComponent
									class="size-5 text-muted-foreground"
								/>
							{/if}
						</div>
						<div class="min-w-0 flex-1">
							<p class="truncate text-sm font-medium">
								{file.fileName}
							</p>
							<p class="truncate text-xs text-muted-foreground">
								{file.filePath}
							</p>
						</div>
						<span class="shrink-0 text-xs text-muted-foreground"
							>Row #{file.rowId}</span
						>
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
</div>
