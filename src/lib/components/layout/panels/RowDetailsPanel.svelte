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
		String(defaultSettings.filePathPattern || "{files_folder}/{file_column}"),
	);
	let fileColumnName = $state(
		String(defaultSettings.fileColumnName || "file"),
	);
	let useThumbnailsOnly = $state(true);

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
			useThumbnailsOnly = settings.useThumbnailsOnly !== "false";
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

/* --- AI suggestions state (mock data for now) --- */

let showAiPanel = $state(true);

interface AiSuggestion {
	value: string;
	reason?: string;
}

type AiFieldState = "pending" | "accepted" | "rejected";

let aiSuggestions = $state<Record<string, AiSuggestion>>({});
let aiFieldStates = $state<Record<string, AiFieldState>>({});

// Regenerate mock suggestions whenever the selected row changes
$effect(() => {
	const _rowId = qrateStore.selectedRowId;

	if (!selectedRow) {
		aiSuggestions = {};
		aiFieldStates = {};
		return;
	}

	const suggestions: Record<string, AiSuggestion> = {};

	for (const field of rowFields) {
		if (field.id === "accessIdentifier") continue; // hard guard: AI must not touch this

		const raw = field.value;
		const base =
			raw !== null && raw !== undefined && String(raw).trim() !== ""
				? String(raw).trim()
				: "";

		const suggested =
			base !== ""
				? base
				: `[AI suggestion for ${field.name}]`;

		suggestions[field.id] = {
			value: suggested,
			reason: "Mock suggestion - replace with Cohere result later",
		};
	}

	aiSuggestions = suggestions;
	aiFieldStates = {};
});


	let altPressed = $state(false);

	function handleKeydown(e: KeyboardEvent) {
		if (e.key === "Alt") altPressed = true;
	}
	function handleKeyup(e: KeyboardEvent) {
		if (e.key === "Alt") altPressed = false;
	}

	$effect(() => {
		window.addEventListener("keydown", handleKeydown);
		window.addEventListener("keyup", handleKeyup);
		return () => {
			window.removeEventListener("keydown", handleKeydown);
			window.removeEventListener("keyup", handleKeyup);
		};
	});

	const PANE_STORAGE_KEY = "qrate:row-details-pane-split";
	let filesPaneSize = $state(60);
	let pendingPaneSize: number | null = null;

	onMount(() => {
		const saved = localStorage.getItem(PANE_STORAGE_KEY);
		if (saved) filesPaneSize = parseFloat(saved);
	});

	function handlePaneLayoutChange(sizes: number[]) {
		pendingPaneSize = sizes[0];
	}

	function handlePaneDragEnd(isDragging: boolean) {
		if (isDragging || pendingPaneSize === null) return;
		localStorage.setItem(PANE_STORAGE_KEY, String(pendingPaneSize));
		pendingPaneSize = null;
	}

	let editingFieldId = $state<string | null>(null);
	let fieldDraftValues = $state<Record<string, string>>({});
	let editingInput = $state<HTMLTextAreaElement | null>(null);
	let fieldHeights = $state<Record<string, number>>({});

	// Separate editing state for AI suggestions
	let aiEditingFieldId = $state<string | null>(null);
	let aiDraftValues = $state<Record<string, string>>({});
	let aiEditingInput = $state<HTMLTextAreaElement | null>(null);

	$effect(() => {
		if (editingInput) {
			editingInput.focus();
			editingInput.style.height = "auto";
			editingInput.style.height = editingInput.scrollHeight + "px";
		}
	});

	$effect(() => {
		if (aiEditingInput) {
			aiEditingInput.focus();
			aiEditingInput.style.height = "auto";
			aiEditingInput.style.height = aiEditingInput.scrollHeight + "px";
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
		fieldDraftValues[fieldId] =
			initialValue !== null && initialValue !== undefined
				? String(initialValue)
				: "";
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
		if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) {
			event.preventDefault();
			saveEditingField(fieldId);
		} else if (event.key === "Escape") {
			event.preventDefault();
			cancelEditingField();
		}
	}
	
	function startAiEditingField(fieldId: string, initialValue: string) {
		aiEditingFieldId = fieldId;
		aiDraftValues[fieldId] = initialValue ?? "";
	}

	function cancelAiEditingField() {
		aiEditingFieldId = null;
	}

	function saveAiEditingField(fieldId: string) {
		const newValue = aiDraftValues[fieldId] ?? "";
		const existing = aiSuggestions[fieldId];

		aiSuggestions = {
			...aiSuggestions,
			[fieldId]: {
				...(existing ?? {}),
				value: newValue,
			},
		};

		aiEditingFieldId = null;
	}

	function handleAiFieldKeydown(event: KeyboardEvent, fieldId: string) {
		// Plain Enter = save; Shift+Enter still inserts a newline
		if (event.key === "Enter" && !event.shiftKey) {
			event.preventDefault();
			saveAiEditingField(fieldId);
		} else if (event.key === "Escape") {
			event.preventDefault();
			cancelAiEditingField();
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

	/* --- AI actions --- */

	function getAiStatusLabel(fieldId: string): string {
		const state = aiFieldStates[fieldId];
		if (state === "accepted") return "Accepted";
		if (state === "rejected") return "Rejected";
		return "Pending";
	}

	async function applyAiSuggestion(fieldId: string) {
		if (!selectedRow) return;
		if (fieldId === "accessIdentifier") return; // safety guard

		const suggestion = aiSuggestions[fieldId];
		if (!suggestion) return;

		await qrateStore.updateCell(
			selectedRow.row_id,
			fieldId,
			suggestion.value,
		);

		aiFieldStates = {
			...aiFieldStates,
			[fieldId]: "accepted",
		};
	}

	function rejectAiSuggestion(fieldId: string) {
		if (fieldId === "accessIdentifier") return; // safety guard
		aiFieldStates = {
			...aiFieldStates,
			[fieldId]: "rejected",
		};
	}

	async function applyAllAiSuggestions() {
		if (!selectedRow) return;

		const newStates: Record<string, AiFieldState> = {
			...aiFieldStates,
		};

		for (const [fieldId, suggestion] of Object.entries(aiSuggestions)) {
			if (fieldId === "accessIdentifier") continue; // safety guard
			if (!suggestion) continue;

			await qrateStore.updateCell(
				selectedRow.row_id,
				fieldId,
				suggestion.value,
			);
			newStates[fieldId] = "accepted";
		}

		aiFieldStates = newStates;
	}

	function rejectAllAiSuggestions() {
		const newStates: Record<string, AiFieldState> = {
			...aiFieldStates,
		};
		for (const fieldId of Object.keys(aiSuggestions)) {
			if (fieldId === "accessIdentifier") continue; // safety guard
			newStates[fieldId] = "rejected";
		}
		aiFieldStates = newStates;
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
			<Resizable.PaneGroup
				direction="vertical"
				class="h-full"
				onLayoutChange={handlePaneLayoutChange}
			>
				<Resizable.Pane defaultSize={filesPaneSize} minSize={20}>
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
											thumbnail={useThumbnailsOnly}
											showOpenButton={true}
											showLoadFullButton={useThumbnailsOnly}
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
					onDraggingChange={handlePaneDragEnd}
				/>

				<Resizable.Pane defaultSize={40} minSize={15}>
					<div class="h-full overflow-y-auto p-3">
						<div class="mb-2 flex items-center justify-between gap-2">
							<h3
								class="text-xs font-medium uppercase text-muted-foreground"
							>
								Row Data
							</h3>
							<div class="flex items-center gap-2">
								<span
									class="hidden text-[12px] text-muted-foreground md:inline"
								>
									AI suggestions (mock)
								</span>
								<Button
									variant="ghost"
									class="h-6 px-2 text-[12px]"
									onclick={() => (showAiPanel = !showAiPanel)}
								>
									{#if showAiPanel}
										Hide AI
									{:else}
										Show AI
									{/if}
								</Button>
								{#if showAiPanel && Object.keys(aiSuggestions).length > 0}
									<Button
										variant="outline"
										class="h-6 px-2 text-[10px]"
										onclick={applyAllAiSuggestions}
									>
										Accept all
									</Button>
									<Button
										variant="ghost"
										class="h-6 px-2 text-[10px]"
										onclick={rejectAllAiSuggestions}
									>
										Reject all
									</Button>
								{/if}
							</div>
						</div>

						<div class="space-y-2">
							{#each rowFields as field}
								<div class="rounded-md bg-muted/50 p-2">
									<div
										class="mb-1 text-xs font-medium text-muted-foreground"
									>
										{field.name}
									</div>

									<div class="flex gap-3">
										<!-- Left: current editable value -->
										<div class="flex-1">
											<div class="wrap-break-word text-sm">
												{#if editingFieldId === field.id}
													<textarea
														class="w-full max-h-80 resize-y rounded border border-border bg-background px-2 py-1 text-sm leading-snug"
														style:min-height={fieldHeights[field.id]
															? `${fieldHeights[field.id]}px`
															: "1.5rem"}
														bind:value={fieldDraftValues[field.id]}
														oninput={autoResizeTextarea}
														onkeydown={(event) =>
															handleFieldKeydown(
																event,
																field.id,
															)}
														onblur={cancelEditingField}
														bind:this={editingInput}
													></textarea>
												{:else}
													<!-- svelte-ignore a11y_click_events_have_key_events -->
													<!-- svelte-ignore a11y_no_static_element_interactions -->
													<div
														class="w-full select-text whitespace-pre-wrap"
														style:cursor={altPressed
															? "pointer"
															: "text"}
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
														title="Double-click or Alt+click to edit (Ctrl+Enter to save)"
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

										<!-- Right: AI suggestion, same row height -->
										{#if showAiPanel}
											<div
												class="flex-1 border-l border-border pl-3 text-xs"
											>
												{#if field.id === "accessIdentifier"}
													<div
														class="text-[12px] italic text-muted-foreground"
													>
														accessIdentifier is not modified by AI.
													</div>
												{:else}
													{@const suggestion =
														aiSuggestions[field.id]}
													{#if suggestion}
														{#if aiEditingFieldId === field.id}
															<textarea
																class="w-full max-h-80 resize-y rounded border border-border bg-background px-2 py-1 text-[12px] leading-snug"
																bind:value={aiDraftValues[field.id]}
																oninput={autoResizeTextarea}
																onkeydown={(event) =>
																	handleAiFieldKeydown(
																		event,
																		field.id,
																	)}
																onblur={() => saveAiEditingField(field.id)}
																bind:this={aiEditingInput}
															></textarea>
														{:else}
															<!-- svelte-ignore a11y_click_events_have_key_events -->
															<!-- svelte-ignore a11y_no_static_element_interactions -->
															<div
																class="whitespace-pre-wrap text-[12px] select-text"
																style:cursor={altPressed ? "pointer" : "text"}
																ondblclick={() =>
																	startAiEditingField(
																		field.id,
																		suggestion.value,
																	)}
																onclick={(e) =>
																	e.altKey &&
																	startAiEditingField(
																		field.id,
																		suggestion.value,
																	)}
																title="Double-click or Alt+click to edit suggestion (Ctrl+Enter to save)"
															>
																{suggestion.value}
															</div>
														{/if}

														<div class="mt-1 flex gap-1">
															<Button
																variant="outline"
																class="h-6 px-2 text-[10px]"
																onclick={() =>
																	applyAiSuggestion(
																		field.id,
																	)}
																disabled={aiFieldStates[field.id] ===
																	"accepted"}
															>
																Accept
															</Button>
															<Button
																variant="ghost"
																class="h-6 px-2 text-[10px]"
																onclick={() =>
																	rejectAiSuggestion(
																		field.id,
																	)}
																disabled={aiFieldStates[field.id] ===
																	"rejected"}
															>
																Reject
															</Button>
														</div>
													{:else}
														<p
															class="text-[12px] italic text-muted-foreground"
														>
															No suggestion.
														</p>
													{/if}
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
				<div class="mb-2 flex items-center justify-between gap-2">
					<h3
						class="text-xs font-medium uppercase text-muted-foreground"
					>
						Row Data
					</h3>
					<div class="flex items-center gap-2">
						<span
							class="hidden text-[12px] text-muted-foreground md:inline"
						>
							AI suggestions (mock)
						</span>
						<Button
							variant="ghost"
							class="h-6 px-2 text-[12px]"
							onclick={() => (showAiPanel = !showAiPanel)}
						>
							{#if showAiPanel}
								Hide AI
							{:else}
								Show AI
							{/if}
						</Button>
						{#if showAiPanel && Object.keys(aiSuggestions).length > 0}
							<Button
								variant="outline"
								class="h-6 px-2 text-[10px]"
								onclick={applyAllAiSuggestions}
							>
								Accept all
							</Button>
							<Button
								variant="ghost"
								class="h-6 px-2 text-[10px]"
								onclick={rejectAllAiSuggestions}
							>
								Reject all
							</Button>
						{/if}
					</div>
				</div>

				<div class="space-y-2">
					{#each rowFields as field}
						<div class="rounded-md bg-muted/50 p-2">
							<div
								class="mb-1 text-xs font-medium text-muted-foreground"
							>
								{field.name}
							</div>

							<div class="flex gap-3">
								<!-- Left: current editable value -->
								<div class="flex-1">
									<div class="wrap-break-word text-sm">
										{#if editingFieldId === field.id}
											<textarea
												class="w-full max-h-80 resize-y rounded border border-border bg-background px-2 py-1 text-sm leading-snug"
												style:min-height={fieldHeights[field.id]
													? `${fieldHeights[field.id]}px`
													: "1.5rem"}
												bind:value={fieldDraftValues[field.id]}
												oninput={autoResizeTextarea}
												onkeydown={(event) =>
													handleFieldKeydown(
														event,
														field.id,
													)}
												onblur={cancelEditingField}
												bind:this={editingInput}
											></textarea>
										{:else}
											<!-- svelte-ignore a11y_click_events_have_key_events -->
											<!-- svelte-ignore a11y_no_static_element_interactions -->
											<div
												class="w-full select-text whitespace-pre-wrap"
												style:cursor={altPressed
													? "pointer"
													: "text"}
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
												title="Double-click or Alt+click to edit (Ctrl+Enter to save)"
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

								<!-- Right: AI suggestion, same row height -->
								{#if showAiPanel}
									<div
										class="flex-1 border-l border-border pl-3 text-xs"
									>
										{#if field.id === "accessIdentifier"}
											<div
												class="text-[12px] italic text-muted-foreground"
											>
												accessIdentifier is not modified by AI.
											</div>
										{:else}
											{@const suggestion =
												aiSuggestions[field.id]}
											{#if suggestion}
												{#if aiEditingFieldId === field.id}
													<textarea
														class="w-full max-h-80 resize-y rounded border border-border bg-background px-2 py-1 text-[12px] leading-snug"
														bind:value={aiDraftValues[field.id]}
														oninput={autoResizeTextarea}
														onkeydown={(event) =>
															handleAiFieldKeydown(
																event,
																field.id,
															)}
														onblur={cancelAiEditingField}
														bind:this={aiEditingInput}
													></textarea>
												{:else}
													<!-- svelte-ignore a11y_click_events_have_key_events -->
													<!-- svelte-ignore a11y_no_static_element_interactions -->
													<div
														class="whitespace-pre-wrap text-[12px] select-text"
														style:cursor={altPressed ? "pointer" : "text"}
														ondblclick={() =>
															startAiEditingField(
																field.id,
																suggestion.value,
															)}
														onclick={(e) =>
															e.altKey &&
															startAiEditingField(
																field.id,
																suggestion.value,
															)}
														title="Double-click or Alt+click to edit suggestion (Ctrl+Enter to save)"
													>
														{suggestion.value}
													</div>
												{/if}

												<div class="mt-1 flex gap-1">
													<Button
														variant="outline"
														class="h-6 px-2 text-[10px]"
														onclick={() =>
															applyAiSuggestion(
																field.id,
															)}
														disabled={aiFieldStates[field.id] ===
															"accepted"}
													>
														Accept
													</Button>
													<Button
														variant="ghost"
														class="h-6 px-2 text-[10px]"
														onclick={() =>
															rejectAiSuggestion(
																field.id,
															)}
														disabled={aiFieldStates[field.id] ===
															"rejected"}
													>
														Reject
													</Button>
												</div>
											{:else}
												<p
													class="text-[12px] italic text-muted-foreground"
												>
													No suggestion.
												</p>
											{/if}
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
<style>
	.wrap-break-word {
		word-break: break-word;
	}
</style>