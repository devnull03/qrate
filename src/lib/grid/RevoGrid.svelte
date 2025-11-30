<script lang="ts">
	import { onMount } from "svelte";
	import { RevoGrid as RevoGridComponent } from "@revolist/svelte-datagrid";
	import type { ColumnRegular, DataType } from "@revolist/revogrid";
	import { Button } from "$lib/components/ui/button/index.js";
	import { qrateStore, type ColumnDef } from "$lib/stores/qrateStore.svelte";

	// Grid reference
	let grid: any = $state();
	let gridContainer: HTMLDivElement | null = $state(null);

	// Convert our ColumnDef format to RevoGrid's ColumnRegular format
	const convertColumns = (columns: ColumnDef[]): ColumnRegular[] => {
		// Add row number column as the first column
		const rowNumberColumn: ColumnRegular = {
			prop: "_rowNum",
			name: "#",
			size: 60,
			readonly: true,
			sortable: false,
			filter: false,
			cellProperties: () => ({
				class: "row-number-cell",
			}),
		};

		const dataColumns = columns
			.filter((col) => !col.hidden)
			.map((col) => ({
				prop: col.id,
				name: col.name,
				size: col.width,
				sortable: true,
				filter: true,
			}));

		return [rowNumberColumn, ...dataColumns];
	};

	// Add row numbers to the data
	const addRowNumbers = (
		rows: Record<string, any>[],
		offset: number,
	): DataType[] => {
		return rows.map((row, index) => ({
			...row,
			_rowNum: offset + index + 1,
		}));
	};

	// RevoGrid columns derived from store
	let revoColumns = $derived(convertColumns(qrateStore.columns));

	// RevoGrid rows derived from store (with row numbers)
	let revoRows = $derived(addRowNumbers(qrateStore.rows, 0));

	// Loading state
	let isLoading = $derived(qrateStore.isLoading);
	let isLoadingMore = $derived(qrateStore.isLoadingMore);
	let hasMoreRows = $derived(qrateStore.hasMoreRows);

	// Check if dark mode is active
	let isDark = $state(false);

	$effect(() => {
		if (typeof document !== "undefined") {
			isDark = document.documentElement.classList.contains("dark");

			// Watch for theme changes
			const observer = new MutationObserver(() => {
				isDark = document.documentElement.classList.contains("dark");
			});

			observer.observe(document.documentElement, {
				attributes: true,
				attributeFilter: ["class"],
			});

			return () => observer.disconnect();
		}
	});

	// Handle cell edit
	const handleCellEdit = async (event: CustomEvent) => {
		const { detail } = event;

		// Ignore edits to the row number column
		if (detail.prop === "_rowNum") {
			return;
		}

		const rowId = detail.model.row_id;
		const columnId = detail.prop;
		const newValue = detail.val;

		try {
			await qrateStore.updateCell(rowId, columnId, newValue);
		} catch (err) {
			console.error("Failed to update cell:", err);
			// Revert the change in the grid
			if (grid) {
				grid.refresh();
			}
		}
	};

	// Handle column resize
	const handleColumnResize = async (event: CustomEvent) => {
		const { detail } = event;
		const columnId = detail.prop;
		const newSize = detail.size;

		// Ignore resize of row number column
		if (columnId === "_rowNum") {
			return;
		}

		const column = qrateStore.columns.find((c) => c.id === columnId);
		if (column) {
			const updatedColumn = { ...column, width: newSize };
			try {
				await qrateStore.updateColumn(updatedColumn);
			} catch (err) {
				console.error("Failed to update column width:", err);
			}
		}
	};

	// Handle row focus/selection
	const handleRowFocus = (event: CustomEvent) => {
		const { detail } = event;
		if (detail && detail.model && detail.model.row_id !== undefined) {
			qrateStore.selectRow(detail.model.row_id);
		}
	};

	// Handle viewport scroll for lazy loading
	const handleViewportScroll = async (event: CustomEvent) => {
		const { detail } = event;

		// Check if we're near the bottom of the data
		if (detail && hasMoreRows && !isLoadingMore) {
			const viewportEndRow = detail.endRow || 0;
			const totalLoadedRows = qrateStore.rows.length;
			const threshold = 20; // Load more when within 20 rows of the end

			if (viewportEndRow >= totalLoadedRows - threshold) {
				await qrateStore.loadMoreRows(100);
			}
		}
	};

	// Set up scroll listener on the grid's viewport
	const setupScrollListener = () => {
		if (!gridContainer) return;

		// Find the viewport element inside RevoGrid
		const viewport = gridContainer.querySelector(
			"revogr-viewport-scroll, .rgViewport, [data-rgviewport]",
		);
		if (viewport) {
			viewport.addEventListener("scroll", handleScroll);
			return () => viewport.removeEventListener("scroll", handleScroll);
		}
	};

	// Handle native scroll event as backup
	const handleScroll = async (event: Event) => {
		const target = event.target as HTMLElement;
		if (!target || !hasMoreRows || isLoadingMore) return;

		const { scrollTop, scrollHeight, clientHeight } = target;
		const scrollPercentage = (scrollTop + clientHeight) / scrollHeight;

		// Load more when scrolled past 80%
		if (scrollPercentage > 0.8) {
			await qrateStore.loadMoreRows(100);
		}
	};

	// Initialize grid on mount
	onMount(() => {
		console.log("RevoGrid mounted");

		// Set up scroll listener after a short delay to ensure grid is rendered
		const timeoutId = setTimeout(() => {
			setupScrollListener();
		}, 500);

		return () => {
			clearTimeout(timeoutId);
		};
	});
</script>

<div
	class="flex h-full w-full flex-col overflow-hidden p-4"
	bind:this={gridContainer}
>
	{#if !qrateStore.isFileOpen}
		<div class="flex h-full w-full items-center justify-center">
			<div class="text-center text-muted-foreground">
				<p class="mb-2 text-lg">No file open</p>
				<p class="text-sm">
					Open a .qrate file or import a CSV to get started
				</p>
			</div>
		</div>
	{:else if isLoading}
		<div class="flex h-full w-full items-center justify-center">
			<div class="text-center text-muted-foreground">
				<p class="mb-2 text-lg">Loading...</p>
			</div>
		</div>
	{:else}
		<div
			class="min-h-0 flex-1 overflow-hidden rounded-lg border border-border"
		>
			<RevoGridComponent
				bind:this={grid}
				source={revoRows}
				columns={revoColumns}
				theme={isDark ? "darkMaterial" : "default"}
				resize={true}
				range={true}
				readonly={false}
				autoSizeColumn={false}
				on:afteredit={handleCellEdit}
				on:aftercolumnresize={handleColumnResize}
				on:afterfocus={handleRowFocus}
				on:viewportscroll={handleViewportScroll}
			/>
		</div>

		{#if isLoadingMore}
			<div class="flex items-center justify-center py-2">
				<p class="text-xs text-muted-foreground">
					Loading more rows...
				</p>
			</div>
		{/if}

		{#if !hasMoreRows && qrateStore.rows.length > 0}
			<div class="flex items-center justify-center py-1">
				<p class="text-xs text-muted-foreground">
					Showing all {qrateStore.totalRows.toLocaleString()} rows
				</p>
			</div>
		{:else if hasMoreRows}
			<div class="flex items-center justify-center py-1">
				<p class="text-xs text-muted-foreground">
					Loaded {qrateStore.rows.length.toLocaleString()} of {qrateStore.totalRows.toLocaleString()}
					rows
				</p>
			</div>
		{/if}
	{/if}

	{#if qrateStore.error}
		<div
			class="fixed bottom-4 right-4 z-50 max-w-sm rounded-lg border border-destructive/50 bg-destructive/10 p-4 shadow-lg backdrop-blur-sm"
		>
			<p class="font-semibold text-destructive">Error</p>
			<p class="text-sm text-destructive/90">{qrateStore.error}</p>
			<Button
				variant="ghost"
				size="sm"
				class="mt-2 h-auto p-0 text-xs text-destructive underline hover:bg-transparent"
				onclick={() => (qrateStore.error = null)}
			>
				Dismiss
			</Button>
		</div>
	{/if}
</div>

<style>
	:global(.row-number-cell) {
		background-color: var(--muted) !important;
		color: var(--muted-foreground) !important;
		font-size: 0.75rem !important;
		text-align: center !important;
		user-select: none !important;
	}
</style>
