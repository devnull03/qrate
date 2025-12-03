<script lang="ts">
	import { RevoGrid as RevoGridComponent } from "@revolist/svelte-datagrid";
	import type { ColumnRegular, DataType } from "@revolist/revogrid";
	import { Button } from "$lib/components/ui/button/index.js";
	import { qrateStore, type ColumnDef } from "$lib/stores/qrateStore.svelte";

	let grid: any = $state();
	let gridContainer: HTMLDivElement | null = $state(null);

	const convertColumns = (columns: ColumnDef[]): ColumnRegular[] => {
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

	const addRowNumbers = (
		rows: Record<string, any>[],
		offset: number,
	): DataType[] => {
		return rows.map((row, index) => ({
			...row,
			_rowNum: offset + index + 1,
		}));
	};

	let revoColumns = $derived(convertColumns(qrateStore.columns));
	let revoRows = $derived(addRowNumbers(qrateStore.rows, 0));

	let isDark = $state(false);

	$effect(() => {
		if (typeof document === "undefined") return;

		isDark = document.documentElement.classList.contains("dark");

		const observer = new MutationObserver(() => {
			isDark = document.documentElement.classList.contains("dark");
		});

		observer.observe(document.documentElement, {
			attributes: true,
			attributeFilter: ["class"],
		});

		return () => observer.disconnect();
	});

	const handleAfterEdit = async (event: CustomEvent) => {
		const { detail } = event;
		if (!detail) return;

		if (detail.prop && detail.prop !== "_rowNum") {
			const rowId = detail.model?.row_id;
			const columnId = detail.prop;
			const newValue = detail.val;

			if (rowId !== undefined) {
				try {
					await qrateStore.updateCell(rowId, columnId, newValue);
				} catch {
					grid?.refresh();
				}
			}
		}
	};

	const handleAfterColumnResize = async (event: CustomEvent) => {
		const { detail } = event;
		if (!detail) return;

		for (const [, colData] of Object.entries(detail) as [
			string,
			ColumnRegular,
		][]) {
			const columnId = colData.prop as string;
			if (columnId === "_rowNum") continue;

			const column = qrateStore.columns.find((c) => c.id === columnId);
			if (column && colData.size) {
				await qrateStore
					.updateColumn({ ...column, width: colData.size })
					.catch(() => {});
			}
		}
	};

	const handleAfterFocus = (event: CustomEvent) => {
		const { detail } = event;
		if (!detail) return;

		const rowId = detail.model?.row_id ?? null;
		const colProp = detail.column?.prop ?? null;

		qrateStore.selectRow(rowId);
		qrateStore.selectColumn(colProp === "_rowNum" ? null : colProp);
		qrateStore.selectRange(null);
	};

	const handleBeforeCellFocus = (event: CustomEvent) => {
		const { detail } = event;
		console.log("beforecellfocus event:", detail);

		// Try to get selection from grid
		if (grid) {
			grid.getSelectedRange?.()
				.then((range: any) => {
					console.log("grid.getSelectedRange:", range);
				})
				.catch(() => {});
		}
	};
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
	{:else if qrateStore.isLoading}
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
				on:afteredit={handleAfterEdit}
				on:aftercolumnresize={handleAfterColumnResize}
				on:afterfocus={handleAfterFocus}
				on:beforecellfocus={handleBeforeCellFocus}
			/>
		</div>
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
