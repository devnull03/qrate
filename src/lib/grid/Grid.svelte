<script lang="ts">
	import type {
		CellValueChangedEvent,
		GridApi,
		GridOptions,
		SelectionChangedEvent,
	} from "ag-grid-community";
	import { createGrid, themeQuartz } from "ag-grid-community";
	import { csvStore } from "$lib/stores/csvStore.svelte";

	// Grid API: Access to Grid API methods
	let gridApi: GridApi | undefined = $state();

	// Generate column definitions from CSV data
	function generateColumnDefs(data: Record<string, string>[]) {
		if (data.length === 0) return [];

		const firstRow = data[0];
		const columns = Object.keys(firstRow).map((key) => ({
			field: key,
			filter: true,
			editable: true,
			sortable: true,
			resizable: true,
		}));

		return columns;
	}

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

	const gridOptions: GridOptions = {
		// Data to be displayed
		rowData: [],
		// Columns to be displayed (Should match rowData properties)
		columnDefs: [],
		// Use AG Grid v34 Theming API
		theme: themeQuartz,
		// Configurations applied to all columns
		defaultColDef: {
			filter: true,
			editable: true,
			sortable: true,
			resizable: true,
		},
		// Grid Options & Callbacks
		pagination: true,
		rowSelection: { mode: "multiRow", headerCheckbox: false },
		onSelectionChanged: (event: SelectionChangedEvent) => {
			console.log("Row Selection Event!");
		},
		onCellValueChanged: (event: CellValueChangedEvent) => {
			console.log(`New Cell Value: ${event.value}`);
		},
	};

	// Create Grid: Create new grid within the #myGrid div, using the Grid Options object
	let mygrid: HTMLDivElement | null = $state(null);

	// Initialize grid when div is available
	$effect(() => {
		if (mygrid && !gridApi) {
			gridApi = createGrid(mygrid, gridOptions);
		}
	});

	// Watch for changes in CSV data and update grid
	$effect(() => {
		if (gridApi && csvStore.data.length > 0) {
			const columnDefs = generateColumnDefs(csvStore.data);
			gridApi.setGridOption("columnDefs", columnDefs);
			gridApi.setGridOption("rowData", csvStore.data);
		} else if (gridApi && csvStore.data.length === 0) {
			// Clear grid when no data
			gridApi.setGridOption("columnDefs", []);
			gridApi.setGridOption("rowData", []);
		}
	});

	// Update grid theme when dark mode changes
	$effect(() => {
		if (gridApi) {
			gridApi.setGridOption(
				"theme",
				themeQuartz.withParams({
					browserColorScheme: isDark
						? "dark"
						: "light",
				}),
			);
		}
	});
</script>

<div class="size-full relative">
	{#if csvStore.data.length === 0}
		<div
			class="absolute inset-0 flex items-center justify-center p-8 bg-background z-10"
		>
			<div class="text-center text-muted-foreground">
				<p class="text-lg mb-2">No data to display</p>
				<p class="text-sm">
					Load a CSV file from the sidebar to get
					started
				</p>
			</div>
		</div>
	{/if}
	<div bind:this={mygrid} class="size-full"></div>
</div>
