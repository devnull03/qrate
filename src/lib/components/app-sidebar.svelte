<script lang="ts">
	import * as Sidebar from "$lib/components/ui/sidebar/index.js";
	import { Button } from "$lib/components/ui/button/index.js";
	import type { ComponentProps } from "svelte";
	import { qrateStore } from "$lib/stores/qrateStore.svelte";
	import { appSettingsStore, resolveFilePath } from "$lib/stores/appSettings";
	import { openPath, revealItemInDir } from "@tauri-apps/plugin-opener";
	import FileIcon from "@lucide/svelte/icons/file";
	import FileTextIcon from "@lucide/svelte/icons/file-text";
	import ImageIcon from "@lucide/svelte/icons/image";
	import VideoIcon from "@lucide/svelte/icons/video";
	import MusicIcon from "@lucide/svelte/icons/music";
	import GridIcon from "@lucide/svelte/icons/layout-grid";
	import ListIcon from "@lucide/svelte/icons/list";
	import ExternalLinkIcon from "@lucide/svelte/icons/external-link";

	let {
		ref = $bindable(null),
		...restProps
	}: ComponentProps<typeof Sidebar.Root> = $props();

	// View mode for files
	let viewMode = $state<"list" | "grid">("list");

	// Selected row from store
	let selectedRowId = $derived(qrateStore.selectedRowId);

	// Settings state (sync with store)
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

	// Get files for current row
	let currentRowFiles = $derived.by(() => {
		if (!selectedRowId || !qrateStore.isFileOpen) return [];

		const row = qrateStore.rows.find((r) => r.row_id === selectedRowId);
		if (!row) return [];

		// Find the file column value
		const fileColumn = qrateStore.columns.find(
			(col) =>
				col.name.toLowerCase() === fileColumnName.toLowerCase() ||
				col.id === fileColumnName,
		);

		if (!fileColumn) return [];

		const fileValue = row[fileColumn.id];
		if (!fileValue) return [];

		// Resolve the file path
		const filePath = resolveFilePath(
			filePathPattern,
			filesFolder,
			row,
			fileColumn.id,
		);

		return [
			{
				name: fileValue,
				path: filePath,
				type: getFileType(fileValue),
			},
		];
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
	async function handleOpenFile(filePath: string) {
		try {
			await openPath(filePath);
		} catch (err) {
			console.error("Failed to open file:", err);
		}
	}

	// Open file location in explorer
	async function handleRevealFile(filePath: string) {
		try {
			await revealItemInDir(filePath);
		} catch (err) {
			console.error("Failed to reveal file:", err);
		}
	}
</script>

<Sidebar.Root bind:ref collapsible="icon" {...restProps}>
	<Sidebar.Content class="overflow-auto">
		<div class="p-2 group-data-[collapsible=icon]:hidden">
			<!-- View Mode Toggle -->
			<div class="mb-3 flex items-center justify-between">
				<span class="text-xs font-medium text-muted-foreground">
					{#if selectedRowId !== null}
						Row #{selectedRowId}
					{:else}
						Row Files
					{/if}
				</span>
				<div class="flex gap-1">
					<Button
						variant={viewMode === "list" ? "secondary" : "ghost"}
						size="icon-sm"
						onclick={() => (viewMode = "list")}
					>
						<ListIcon class="size-3.5" />
					</Button>
					<Button
						variant={viewMode === "grid" ? "secondary" : "ghost"}
						size="icon-sm"
						onclick={() => (viewMode = "grid")}
					>
						<GridIcon class="size-3.5" />
					</Button>
				</div>
			</div>

			{#if !qrateStore.isFileOpen}
				<div class="py-8 text-center text-sm text-muted-foreground">
					No project open
				</div>
			{:else if !filesFolder}
				<div class="py-8 text-center text-sm text-muted-foreground">
					<p class="mb-2">No files folder configured</p>
					<p class="text-xs">Configure in View → Settings</p>
				</div>
			{:else if selectedRowId === null}
				<div class="py-8 text-center text-sm text-muted-foreground">
					Select a row to view files
				</div>
			{:else if currentRowFiles.length === 0}
				<div class="py-8 text-center text-sm text-muted-foreground">
					No files for this row
				</div>
			{:else}
				<!-- Files List/Grid View -->
				{#if viewMode === "list"}
					<div class="space-y-1">
						{#each currentRowFiles as file}
							{@const IconComponent = getFileIcon(file.type)}
							<div
								class="group flex w-full items-center gap-2 rounded-md p-2 text-left text-sm transition-colors hover:bg-sidebar-accent"
							>
								<button
									class="flex min-w-0 flex-1 items-center gap-2"
									onclick={() => handleOpenFile(file.path)}
								>
									<IconComponent
										class="size-4 shrink-0 text-muted-foreground"
									/>
									<span class="min-w-0 flex-1 truncate">
										{file.name}
									</span>
								</button>
								<Button
									variant="ghost"
									size="icon-sm"
									class="size-6 shrink-0 opacity-0 group-hover:opacity-100"
									onclick={() => handleRevealFile(file.path)}
									title="Show in folder"
								>
									<ExternalLinkIcon class="size-3" />
								</Button>
							</div>
						{/each}
					</div>
				{:else}
					<div class="grid grid-cols-2 gap-2">
						{#each currentRowFiles as file}
							{@const IconComponent = getFileIcon(file.type)}
							<button
								class="group flex aspect-square flex-col items-center justify-center gap-2 rounded-md border border-border p-2 text-center transition-colors hover:bg-sidebar-accent"
								onclick={() => handleOpenFile(file.path)}
							>
								<IconComponent
									class="size-8 text-muted-foreground"
								/>
								<span class="w-full truncate text-xs">
									{file.name}
								</span>
							</button>
						{/each}
					</div>
				{/if}
			{/if}
		</div>

		<!-- Collapsed state icon -->
		<div class="hidden p-2 group-data-[collapsible=icon]:block">
			<div class="flex flex-col items-center gap-2">
				<FileIcon class="size-4 text-muted-foreground" />
				{#if currentRowFiles.length > 0}
					<span class="text-[10px] text-muted-foreground">
						{currentRowFiles.length}
					</span>
				{/if}
			</div>
		</div>
	</Sidebar.Content>
</Sidebar.Root>
