<script lang="ts">
	import { onMount } from "svelte";
	import { RevoGrid as RevoGridComponent } from "@revolist/svelte-datagrid";
	import type { ColumnRegular, DataType } from "@revolist/revogrid";
	import {
		qrateStore,
		type ColumnDef,
	} from "$lib/stores/qrateStore.svelte";

	// Grid reference
	let grid: any = $state();

	// Convert our ColumnDef format to RevoGrid's ColumnRegular format
	const convertColumns = (columns: ColumnDef[]): ColumnRegular[] => {
		return columns
			.filter((col) => !col.hidden)
			.map((col) => ({
				prop: col.id,
				name: col.name,
				size: col.width,
				sortable: true,
				filter: true,
			}));
	};

	// RevoGrid columns derived from store
	let revoColumns = $derived(convertColumns(qrateStore.columns));

	// RevoGrid rows derived from store
	let revoRows = $derived(qrateStore.rows as DataType[]);

	// Loading state
	let isLoading = $derived(qrateStore.isLoading);

	// Check if dark mode is active
	let isDark = $state(false);

	$effect(() => {
		if (typeof document !== "undefined") {
			isDark =
				document.documentElement.classList.contains(
					"dark",
				);

			// Watch for theme changes
			const observer = new MutationObserver(() => {
				isDark =
					document.documentElement.classList.contains(
						"dark",
					);
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

		const column = qrateStore.columns.find(
			(c) => c.id === columnId,
		);
		if (column) {
			const updatedColumn = { ...column, width: newSize };
			try {
				await qrateStore.updateColumn(updatedColumn);
			} catch (err) {
				console.error(
					"Failed to update column width:",
					err,
				);
			}
		}
	};

	// Handle virtual scroll - load more data when needed
	const handleScrolling = async (event: CustomEvent) => {
		// RevoGrid provides viewport information in scroll events
		// We can use this to determine when to load more data
		const { detail } = event;

		// Calculate which rows should be visible based on scroll position
		if (detail && detail.virtualSize) {
			const start = detail.virtualSize.realCount || 0;
			const end = start + 100; // Load 100 rows at a time

			// Only load if we're scrolling to new data
			if (start !== qrateStore.currentOffset) {
				try {
					await qrateStore.loadRows(start, 100);
				} catch (err) {
					console.error(
						"Failed to load rows during scroll:",
						err,
					);
				}
			}
		}
	};

	// Initialize grid on mount
	onMount(() => {
		// Grid initialization happens automatically via RevoGrid component
		console.log("RevoGrid mounted");
	});
</script>

<div class="grid-container">
	{#if !qrateStore.isFileOpen}
		<div class="empty-state">
			<div class="text-center text-muted-foreground">
				<p class="text-lg mb-2">No file open</p>
				<p class="text-sm">
					Open a .qrate file or import a CSV to
					get started
				</p>
			</div>
		</div>
	{:else if isLoading}
		<div class="empty-state">
			<div class="text-center text-muted-foreground">
				<p class="text-lg mb-2">Loading...</p>
			</div>
		</div>
	{:else}
		<div class="grid-wrapper">
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
				on:afterviewportscroll={handleScrolling}
			/>
		</div>
	{/if}

	{#if qrateStore.error}
		<div class="error-toast">
			<p class="font-semibold">Error</p>
			<p class="text-sm">{qrateStore.error}</p>
			<button
				onclick={() => (qrateStore.error = null)}
				class="mt-2 text-xs underline"
			>
				Dismiss
			</button>
		</div>
	{/if}
</div>


