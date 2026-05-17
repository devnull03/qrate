<script lang="ts">
	import * as Collapsible from "$lib/components/ui/collapsible";
	import DataView from "$lib/components/data-view/data-view.svelte";
	import { ChevronDown, ChevronRight, GripHorizontal } from "@lucide/svelte";
	import { type Sheet } from "$lib/interfaces/sheet.interface";
	import { loadProject, saveProject } from "$lib/utils/project";
	import { onMount } from "svelte";
	import type { PageData } from "./$types";
	import ItemViewModal from "$lib/components/item-view-modal/item-view-modal.svelte";
	import { createSpreadsheetEditingAPI } from "$lib/utils/files";

	let data: PageData = $props();

	let todoDisplayMode = $state<"spreadsheet" | "grid">(data.data.todoDisplayMode ?? "grid");
	let doneDisplayMode = $state<"spreadsheet" | "grid">(data.data.doneDisplayMode ?? "spreadsheet");

	// Load real project data instead of mock data
	let sheet_data: Sheet | null = $state(null);
	let loading = $state(true);
	let error = $state<string | null>(null);

	// Initialize with empty structure for type safety
	let defaultSheet: Sheet = {
		fields: [],
		rows: [],
		images: []
	};

	let actualSheet = $derived(sheet_data || defaultSheet);

	// Split data into todo and done items (initially all items are todo)
	let todoRows: number[] = $state([]);
	let doneRows: number[] = $state([]);

	let todoData: Sheet = $derived({
		fields: actualSheet.fields,
		rows: actualSheet.rows?.filter((_, i) => todoRows.includes(i)) || [],
		images: actualSheet.images?.filter((_, i) => todoRows.includes(i)) || []
	});

	let doneData: Sheet = $derived({
		fields: actualSheet.fields,
		rows: actualSheet.rows?.filter((_, i) => doneRows.includes(i)) || [],
		images: actualSheet.images?.filter((_, i) => doneRows.includes(i)) || []
	});

	// Get the original indices for the filtered arrays
	let todoOriginalIndices = $derived(todoRows);
	let doneOriginalIndices = $derived(doneRows);

	let todoOpen = $state(true);
	let doneOpen = $state(true);

	// Load project data on mount
	onMount(async () => {
		try {
			loading = true;
			const projectData = await loadProject();

			if (projectData) {
				sheet_data = projectData;
				// Initialize all rows as todo items
				todoRows = Array.from({ length: projectData.rows.length }, (_, i) => i);
				doneRows = [];
				error = null;
			} else {
				error = "No project data found. Please create a new project.";
			}
		} catch (err) {
			console.error("Failed to load project:", err);
			error = "Failed to load project data.";
		} finally {
			loading = false;
		}
	});

	// Move item from todo to done
	function moveToCompleted(index: number) {
		const todoIndex = todoRows.indexOf(index);
		if (todoIndex > -1) {
			todoRows = todoRows.filter((i) => i !== index);
			doneRows = [...doneRows, index];
		}
	}

	// Move item from done to todo
	function moveToTodo(index: number) {
		const doneIndex = doneRows.indexOf(index);
		if (doneIndex > -1) {
			doneRows = doneRows.filter((i) => i !== index);
			todoRows = [...todoRows, index];
		}
	}

	let todoHeight = $state(400);
	let doneHeight = $state(400);

	function createResizeHandler(
		heightSetter: (height: number) => void,
		currentHeight: () => number
	) {
		return function (event: MouseEvent) {
			event.preventDefault();
			const startY = event.clientY;
			const startHeight = currentHeight();

			function onMouseMove(e: MouseEvent) {
				const delta = e.clientY - startY;
				const newHeight = Math.max(200, Math.min(800, startHeight + delta));
				heightSetter(newHeight);
			}

			function onMouseUp() {
				document.removeEventListener("mousemove", onMouseMove);
				document.removeEventListener("mouseup", onMouseUp);
			}

			document.addEventListener("mousemove", onMouseMove);
			document.addEventListener("mouseup", onMouseUp);
		};
	}

	let onItemClick = (img: string, itemFields: Record<string, any> | null, filename: string) => {
		itemViewModalData = {
			img,
			itemFields,
			filename
		};
		isItemViewModalOpen = true;
	};

	let isItemViewModalOpen = $state(false);
	let itemViewModalData = $state<{
		img: string;
		itemFields: Record<string, any> | null;
		filename: string;
	} | null>(null);

	let editingApi = $derived(createSpreadsheetEditingAPI(actualSheet));
</script>

{#if itemViewModalData}
	<svelte:boundary>
		{#snippet pending()}
			<p>Loading...</p>
		{/snippet}{#key itemViewModalData.filename}
			<ItemViewModal bind:isOpen={isItemViewModalOpen} {...itemViewModalData} />
		{/key}
	</svelte:boundary>
{/if}

<div class="p-4 max-w-full">
	{#if loading}
		<div class="flex items-center justify-center h-64">
			<div class="text-center">
				<div
					class="animate-spin h-8 w-8 border-4 border-primary border-t-transparent rounded-full mx-auto mb-4"
				></div>
				<p class="text-muted-foreground">Loading project data...</p>
			</div>
		</div>
	{:else if error}
		<div class="flex items-center justify-center h-64">
			<div class="text-center">
				<p class="text-destructive font-medium mb-2">Error Loading Project</p>
				<p class="text-muted-foreground">{error}</p>
				<a
					href="/new"
					class="inline-block mt-4 px-4 py-2 bg-primary text-primary-foreground rounded-md hover:bg-primary/90 transition-colors"
				>
					Create New Project
				</a>
			</div>
		</div>
	{:else}
		<div class="flex flex-col gap-6 w-full">
			<!-- Todo Section -->
			<div class="bg-card border border-border rounded-lg overflow-hidden w-full">
				<Collapsible.Root bind:open={todoOpen}>
					<Collapsible.Trigger class="w-full p-4 bg-muted border-l-4 border-l-destructive">
						<div class="flex items-center gap-2">
							{#if todoOpen}
								<ChevronDown class="h-4 w-4" />
							{:else}
								<ChevronRight class="h-4 w-4" />
							{/if}
							<h2 class="text-lg font-semibold">To Do</h2>
							<span
								class="bg-primary text-primary-foreground text-xs px-2 py-1 rounded-full font-medium"
								>{todoData.rows.length}</span
							>
						</div>
					</Collapsible.Trigger>
					<Collapsible.Content>
						<div class="relative">
							<div class="py-4 overflow-y-auto" style={`height: ${todoHeight}px`}>
								<DataView
									bind:mode={todoDisplayMode}
									bind:items={todoData}
									imagesLoading={false}
									{onItemClick}
									{editingApi}
									{moveToCompleted}
									{moveToTodo}
									isDoneSection={false}
									originalIndices={todoOriginalIndices}
								/>
							</div>
							<!-- svelte-ignore a11y_no_static_element_interactions -->
							<div
								class="flex items-center justify-center h-3 bg-muted/50 cursor-row-resize hover:bg-muted transition-colors"
								onmousedown={createResizeHandler(
									(h) => (todoHeight = h),
									() => todoHeight
								)}
							>
								<GripHorizontal class="h-3 w-3 text-muted-foreground" />
							</div>
						</div>
					</Collapsible.Content>
				</Collapsible.Root>
			</div>

			<!-- Done Section -->
			<div class="bg-card border border-border rounded-lg overflow-hidden w-full">
				<Collapsible.Root bind:open={doneOpen}>
					<Collapsible.Trigger class="w-full p-4 bg-muted border-l-4 border-l-green-500">
						<div class="flex items-center gap-2">
							{#if doneOpen}
								<ChevronDown class="h-4 w-4" />
							{:else}
								<ChevronRight class="h-4 w-4" />
							{/if}
							<h2 class="text-lg font-semibold">Done</h2>
							<span
								class="bg-primary text-primary-foreground text-xs px-2 py-1 rounded-full font-medium"
								>{doneData.rows.length}</span
							>
						</div>
					</Collapsible.Trigger>
					<Collapsible.Content>
						<div class="relative">
							<div class="p-4 overflow-y-auto" style={`height: ${doneHeight}px`}>
								<DataView
									bind:mode={doneDisplayMode}
									bind:items={doneData}
									imagesLoading={false}
									{onItemClick}
									{editingApi}
									{moveToCompleted}
									{moveToTodo}
									isDoneSection={true}
									originalIndices={doneOriginalIndices}
								/>
							</div>
							<!-- svelte-ignore a11y_no_static_element_interactions -->
							<div
								class="flex items-center justify-center h-3 bg-muted/50 cursor-row-resize hover:bg-muted transition-colors"
								onmousedown={createResizeHandler(
									(h) => (doneHeight = h),
									() => doneHeight
								)}
							>
								<GripHorizontal class="h-3 w-3 text-muted-foreground" />
							</div>
						</div>
					</Collapsible.Content>
				</Collapsible.Root>
			</div>
		</div>
	{/if}
</div>
