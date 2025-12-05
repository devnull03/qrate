<script lang="ts">
	import { onMount } from "svelte";
	import { Button } from "$lib/components/ui/button/index.js";
	import * as Resizable from "$lib/components/ui/resizable/index";
	import { qrateStore } from "$lib/stores/qrateStore.svelte";
	import {
		loadSettings,
		subscribeToSettings,
		resolveFilePath,
		defaultSettings,
	} from "$lib/stores/appSettings";
	import ImageViewer from "$lib/components/viewers/ImageViewer.svelte";
	import FileIcon from "@lucide/svelte/icons/file";
	import FileTextIcon from "@lucide/svelte/icons/file-text";
	import ImageIcon from "@lucide/svelte/icons/image";
	import VideoIcon from "@lucide/svelte/icons/video";
	import MusicIcon from "@lucide/svelte/icons/music";
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
		if (!selectedRow || !filesFolder || !fileColumnName) {
			return [];
		}

		const colName = String(fileColumnName).toLowerCase();
		const fileColumn = qrateStore.columns.find(
			(col) =>
				col.name.toLowerCase() === colName || col.id === fileColumnName,
		);
		if (!fileColumn) {
			return [];
		}

		const fileValue = selectedRow[fileColumn.id];
		if (!fileValue) {
			return [];
		}

		const fileName = String(fileValue);
		const filePath = resolveFilePath(
			filePathPattern || "",
			filesFolder,
			selectedRow,
			fileColumn.id,
		);
		const fileType = getFileType(filePath);

		return [
			{
				fileName,
				filePath,
				fileType,
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

	// Which field is currently being edited in the "Row Data" section
	let editingFieldId = $state<string | null>(null);
	let fieldDraftValues = $state<Record<string, string>>({});
	let editingInput = $state<HTMLTextAreaElement | null>(null);
	let fieldHeights = $state<Record<string, number>>({});

	$effect(() => {
		if (editingInput) {
			editingInput.focus();
			editingInput.style.height = "auto";
			editingInput.style.height = editingInput.scrollHeight + "px";
		}
	});

	function autoResizeTextarea(event: Event) {
		const textarea = event.target as HTMLTextAreaElement;
		textarea.style.height = "auto";
		textarea.style.height = textarea.scrollHeight + "px";
	}

	function captureFieldHeight(element: HTMLElement, fieldId: string) {
		fieldHeights[fieldId] = element.offsetHeight;
		return {};
	}

	function startEditingField(fieldId: string, initialValue: unknown) {
		editingFieldId = fieldId;
		fieldDraftValues = {
			...fieldDraftValues,
			[fieldId]:
				initialValue !== null && initialValue !== undefined
					? String(initialValue)
					: "",
		};
	}

	async function saveEditingField(fieldId: string) {
		if (!selectedRow) return;
		const newValue = fieldDraftValues[fieldId] ?? "";
		await qrateStore.updateCell(selectedRow.row_id, fieldId, newValue);
		editingFieldId = null;
	}

	function cancelEditingField() {
		editingFieldId = null;
	}

	function handleFieldKeydown(event: KeyboardEvent, fieldId: string) {
		if (event.key === "Escape") {
			event.preventDefault();
			cancelEditingField();
		}
	}

	function getFileType(pathOrFilename: string): string {
		const ext = pathOrFilename.split(".").pop()?.toLowerCase() || "";
		return (
			Object.entries(fileTypeMap).find(([_, exts]) =>
				exts.includes(ext),
			)?.[0] || "file"
		);
	}

	async function openFile(filePath: string) {
		try {
			await openPath(filePath);
		} catch (err) {
			console.error("Failed to open file:", err);
		}
	}

	async function openFileLocation(filePath: string) {
		try {
			await revealItemInDir(filePath);
		} catch (err) {
			console.error("Failed to open location:", err);
		}
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

	<div class="min-h-0 flex-1 overflow-hidden">
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
		{:else if rowFiles.length > 0}
			<Resizable.PaneGroup direction="vertical" class="h-full">
				<Resizable.Pane defaultSize={60} minSize={20}>
					<div class="flex h-full flex-col overflow-hidden p-3">
						<h3
							class="mb-2 shrink-0 text-xs font-medium uppercase text-muted-foreground"
						>
							Files
						</h3>
						<div class="flex min-h-0 flex-1 flex-col">
							{#each rowFiles as file}
								{@const IconComponent =
									iconMap[file.fileType] || FileIcon}
								{#if file.fileType === "image"}
									<div
										class="group flex min-h-0 flex-1 flex-col overflow-hidden rounded-md border border-border"
									>
										<ImageViewer
											filePath={file.filePath}
											alt={file.fileName}
											thumbnail={true}
											showOpenButton={true}
											class="min-h-0 flex-1"
										/>
										<div
											class="flex shrink-0 items-center gap-2 bg-muted/30 p-2"
										>
											<ImageIcon
												class="size-4 shrink-0 text-muted-foreground"
											/>
											<div class="min-w-0 flex-1">
												<p
													class="truncate text-sm font-medium"
												>
													{file.fileName}
												</p>
											</div>
											<Button
												variant="ghost"
												size="icon-sm"
												class="size-7 shrink-0"
												onclick={() =>
													openFileLocation(
														file.filePath,
													)}
												title="Open file location"
											>
												<FolderOpenIcon
													class="size-3.5"
												/>
											</Button>
										</div>
									</div>
								{:else}
									<div
										class="group flex items-center gap-2 rounded-md p-2 transition-colors hover:bg-accent"
									>
										<button
											class="flex min-w-0 flex-1 items-center gap-2 text-left"
											onclick={() =>
												openFile(file.filePath)}
										>
											<div
												class="flex size-8 shrink-0 items-center justify-center rounded bg-muted"
											>
												<IconComponent
													class="size-4 text-muted-foreground"
												/>
											</div>
											<div class="min-w-0 flex-1">
												<p
													class="truncate text-sm font-medium"
												>
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
											<FolderOpenIcon class="size-3.5" />
										</Button>
									</div>
								{/if}
							{/each}
						</div>
					</div>
				</Resizable.Pane>

				<Resizable.Handle
					class="h-px bg-border transition-colors hover:bg-primary/50"
				/>

				<Resizable.Pane defaultSize={40} minSize={15}>
					<div class="h-full overflow-y-auto p-3">
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
										{#if editingFieldId === field.id}
											<textarea
												class="w-full text-sm bg-background border border-border rounded px-2 py-1 max-h-80 resize-y leading-snug"
												style:min-height={fieldHeights[
													field.id
												]
													? `${fieldHeights[field.id]}px`
													: "1.5rem"}
												bind:value={
													fieldDraftValues[field.id]
												}
												oninput={autoResizeTextarea}
												onkeydown={(event) =>
													handleFieldKeydown(
														event,
														field.id,
													)}
												onblur={() =>
													saveEditingField(field.id)}
												bind:this={editingInput}
											></textarea>
										{:else}
											<!-- svelte-ignore a11y_click_events_have_key_events -->
											<!-- svelte-ignore a11y_no_static_element_interactions -->
											<div
												class="w-full whitespace-pre-wrap cursor-text select-text"
												use:captureFieldHeight={field.id}
												ondblclick={() =>
													startEditingField(
														field.id,
														field.value,
													)}
												onclick={(e) =>
													e.altKey &&
													startEditingField(
														field.id,
														field.value,
													)}
												title="Double-click or Alt+click to edit"
											>
												{#if field.value !== null && field.value !== undefined && field.value !== ""}
													{field.value}
												{:else}
													<span
														class="italic text-muted-foreground"
														>Empty</span
													>
												{/if}
											</div>
										{/if}
									</div>
								</div>
							{/each}
						</div>
					</div>
				</Resizable.Pane>
			</Resizable.PaneGroup>
		{:else}
			<div class="h-full overflow-y-auto p-3">
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
								{#if editingFieldId === field.id}
									<textarea
										class="w-full text-sm bg-background border border-border rounded px-2 py-1 max-h-80 resize-y leading-snug"
										style:min-height={fieldHeights[field.id]
											? `${fieldHeights[field.id]}px`
											: "1.5rem"}
										bind:value={fieldDraftValues[field.id]}
										oninput={autoResizeTextarea}
										onkeydown={(event) =>
											handleFieldKeydown(event, field.id)}
										onblur={() =>
											saveEditingField(field.id)}
										bind:this={editingInput}
									></textarea>
								{:else}
									<!-- svelte-ignore a11y_click_events_have_key_events -->
									<!-- svelte-ignore a11y_no_static_element_interactions -->
									<div
										class="w-full whitespace-pre-wrap cursor-text select-text"
										use:captureFieldHeight={field.id}
										ondblclick={() =>
											startEditingField(
												field.id,
												field.value,
											)}
										onclick={(e) =>
											e.altKey &&
											startEditingField(
												field.id,
												field.value,
											)}
										title="Double-click or Alt+click to edit"
									>
										{#if field.value !== null && field.value !== undefined && field.value !== ""}
											{field.value}
										{:else}
											<span
												class="italic text-muted-foreground"
												>Empty</span
											>
										{/if}
									</div>
								{/if}
							</div>
						</div>
					{/each}
				</div>
			</div>
		{/if}
	</div>
</div>
