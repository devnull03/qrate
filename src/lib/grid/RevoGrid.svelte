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
				theme={isDark ? "darkMaterial" : "compact"}
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

<style>
	.grid-container {
		width: 100%;
		height: 100%;
		position: relative;
		overflow: hidden;
		display: flex;
		flex-direction: column;
	}

	.grid-wrapper {
		width: 100%;
		height: 100%;
		overflow: auto;
		position: relative;
	}

	.empty-state {
		position: absolute;
		inset: 0;
		display: flex;
		align-items: center;
		justify-content: center;
		padding: 2rem;
		background-color: hsl(var(--background));
		z-index: 10;
	}

	.error-toast {
		position: absolute;
		bottom: 1rem;
		right: 1rem;
		background-color: rgb(254 226 226);
		color: rgb(127 29 29);
		padding: 1rem;
		border-radius: 0.375rem;
		box-shadow: 0 10px 15px -3px rgb(0 0 0 / 0.1);
		z-index: 20;
		max-width: 28rem;
	}

	:global(.dark) .error-toast {
		background-color: rgb(127 29 29);
		color: rgb(254 226 226);
	}

	/* RevoGrid base styling */
	:global(revo-grid) {
		width: 100%;
		height: 100%;
		display: block;
	}

	/* Light mode theme - map to app.css variables */
	:global(:not(.dark) revo-grid) {
		/* Core colors */
		--rgrid-color-border: hsl(var(--border));
		--rgrid-color-bg-primary: hsl(var(--background));
		--rgrid-color-bg-secondary: hsl(var(--muted));
		--rgrid-color-text-primary: hsl(var(--foreground));
		--rgrid-color-text-secondary: hsl(var(--muted-foreground));

		/* Header */
		--rgrid-header-bg-color: hsl(var(--muted));
		--rgrid-header-color: hsl(var(--foreground));

		/* Focus and selection */
		--rgrid-focus-border-color: hsl(var(--ring));
		--rgrid-cell-border-color: hsl(var(--border));

		/* Editor - critical for visible editing */
		--rgrid-editor-bg-color: hsl(var(--background));
		--rgrid-editor-border-color: hsl(var(--ring));
		--rgrid-editor-color: hsl(var(--foreground));
	}

	/* Dark mode theme - map to app.css dark variables */
	:global(.dark revo-grid) {
		/* Core colors */
		--rgrid-color-border: hsl(var(--border));
		--rgrid-color-bg-primary: hsl(var(--card));
		--rgrid-color-bg-secondary: hsl(var(--background));
		--rgrid-color-text-primary: hsl(var(--foreground));
		--rgrid-color-text-secondary: hsl(var(--muted-foreground));

		/* Header */
		--rgrid-header-bg-color: hsl(var(--muted));
		--rgrid-header-color: hsl(var(--foreground));

		/* Focus and selection */
		--rgrid-focus-border-color: hsl(var(--ring));
		--rgrid-cell-border-color: hsl(var(--border));

		/* Editor - critical for visible editing in dark mode */
		--rgrid-editor-bg-color: hsl(var(--card));
		--rgrid-editor-border-color: hsl(var(--ring));
		--rgrid-editor-color: hsl(var(--foreground));

		/* Input field inside editor */
		--rgrid-input-bg-color: hsl(var(--input));
		--rgrid-input-color: hsl(var(--foreground));
		--rgrid-input-border-color: hsl(var(--ring));
	}

	/* Override any default styles that might interfere */
	:global(revo-grid .edit-input-wrapper),
	:global(revo-grid .edit-input),
	:global(revo-grid input[type="text"]) {
		background-color: var(--rgrid-editor-bg-color) !important;
		color: var(--rgrid-editor-color) !important;
		border-color: var(--rgrid-editor-border-color) !important;
	}

	/* Ensure editor is visible */
	:global(revo-grid .edit-input-wrapper) {
		background-color: var(--rgrid-editor-bg-color) !important;
		border: 2px solid var(--rgrid-editor-border-color) !important;
		z-index: 100 !important;
	}

	/* Fix scrollbar styling for dark mode */
	:global(.dark .grid-wrapper::-webkit-scrollbar) {
		width: 12px;
		height: 12px;
	}

	:global(.dark .grid-wrapper::-webkit-scrollbar-track) {
		background: hsl(var(--background));
	}

	:global(.dark .grid-wrapper::-webkit-scrollbar-thumb) {
		background: hsl(var(--muted));
		border-radius: 6px;
	}

	:global(.dark .grid-wrapper::-webkit-scrollbar-thumb:hover) {
		background: hsl(var(--muted-foreground));
	}

	/* Light mode scrollbar */
	:global(.grid-wrapper::-webkit-scrollbar) {
		width: 12px;
		height: 12px;
	}

	:global(.grid-wrapper::-webkit-scrollbar-track) {
		background: hsl(var(--muted));
	}

	:global(.grid-wrapper::-webkit-scrollbar-thumb) {
		background: hsl(var(--border));
		border-radius: 6px;
	}

	:global(.grid-wrapper::-webkit-scrollbar-thumb:hover) {
		background: hsl(var(--muted-foreground));
	}
</style>
