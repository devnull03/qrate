<script lang="ts">
	import { onMount } from "svelte";
	import { browser } from "$app/environment";
	import type { ColumnRegular, DataType } from "@revolist/revogrid";
	import { Button } from "$lib/components/ui/button/index.js";
	import { qrateStore, type ColumnDef } from "$lib/stores/qrateStore.svelte";
	import { annotationsService } from "$lib/services/annotations";
	import AddCommentDialog from "$lib/components/layout/panels/AddCommentDialog.svelte";
	import * as ContextMenu from "$lib/components/ui/context-menu/index.js";
	import MessageSquarePlusIcon from "@lucide/svelte/icons/message-square-plus";

	let RevoGridComponent: any = $state(null);
	let grid: any = $state();
	let gridContainer: HTMLDivElement | null = $state(null);
	let isLoadingMore = $state(false);
	let isMounted = $state(false);

	onMount(async () => {
		if (browser) {
			const module = await import("@revolist/svelte-datagrid");
			RevoGridComponent = module.RevoGrid;
			isMounted = true;
		}
	});
	let scrollDebounceTimer: number | null = null;
	let defaultRowHeight = $state(35);

	let contextRowId = $state<number | null>(null);
	let contextColumnId = $state<string | null>(null);
	let lastContextMenuPosition = $state<{ x: number; y: number }>({
		x: 0,
		y: 0,
	});

	let addCommentDialogOpen = $state(false);
	let commentRowId = $state<number | null>(null);
	let commentColumnId = $state<string | null>(null);
	let popoverAnchor = $state<{ x: number; y: number } | null>(null);

	const annotatedCells = $derived.by(() => {
		const cells = new Set<string>();
		for (const annotation of annotationsService.annotations) {
			if (annotation.rowId !== null && annotation.columnId !== null) {
				cells.add(`${annotation.rowId}-${annotation.columnId}`);
			}
		}
		return cells;
	});

	const convertColumns = (columns: ColumnDef[]): ColumnRegular[] => {
		const rowNumberColumn: ColumnRegular = {
			prop: "_rowNum",
			name: "#",
			size: 60,
			readonly: true,
			sortable: false,
			filter: false,
			pin: "colPinStart",
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
				cellProperties: (data: { model?: { row_id?: number } }) => {
					const rowId = data.model?.row_id;
					if (
						rowId !== undefined &&
						annotatedCells.has(`${rowId}-${col.id}`)
					) {
						return { class: "has-annotation" };
					}
					return {};
				},
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

	let previousRowCount = $state<number>(0);
	let cachedRevoRows = $state<DataType[]>([]);

	$effect(() => {
		const currentRows = qrateStore.rows;
		const currentCount = currentRows.length;

		console.log(
			`[RevoGrid] rows effect triggered: prev=${previousRowCount}, current=${currentCount}`,
		);

		if (currentCount !== previousRowCount) {
			if (currentCount > previousRowCount && previousRowCount > 0) {
				// Incremental update: only add new rows
				const newRows = currentRows.slice(previousRowCount);
				console.log(
					`[RevoGrid] Adding ${newRows.length} new rows (${previousRowCount} -> ${currentCount})`,
				);
				const newRowsWithNumbers = addRowNumbers(
					newRows,
					previousRowCount,
				);
				cachedRevoRows = [...cachedRevoRows, ...newRowsWithNumbers];
			} else {
				// Full reset or initial load
				console.log(`[RevoGrid] Full reset: ${currentCount} rows`);
				cachedRevoRows = addRowNumbers(currentRows, 0);
			}
			previousRowCount = currentCount;
		}
	});

	let revoRows = $derived(cachedRevoRows);

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

	const pollRangeSelection = async () => {
		if (!grid) return;

		await new Promise((r) => setTimeout(r, 10));

		try {
			const range = await grid.getSelectedRange?.();
			if (range) {
				const startRow = range.y ?? range.startRow ?? 0;
				const endRow = range.y1 ?? range.endRow ?? startRow;
				const startCol = range.x ?? range.startCol ?? 0;
				const endCol = range.x1 ?? range.endCol ?? startCol;

				const rowSpan = Math.abs(endRow - startRow) + 1;
				const colSpan = Math.abs(endCol - startCol) + 1;

				if (rowSpan > 1 || colSpan > 1) {
					qrateStore.selectRange({
						startRow: Math.min(startRow, endRow),
						endRow: Math.max(startRow, endRow),
						startCol: Math.min(startCol, endCol),
						endCol: Math.max(startCol, endCol),
					});
				} else {
					qrateStore.selectRange(null);
				}
			}
		} catch {
			// ignore
		}
	};

	$effect(() => {
		if (!gridContainer) return;

		const handleMouseUp = () => pollRangeSelection();

		const handleKeyUp = (e: KeyboardEvent) => {
			if (
				e.shiftKey &&
				["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight"].includes(
					e.key,
				)
			) {
				pollRangeSelection();
			}
			if ((e.ctrlKey || e.metaKey) && e.key === "a") {
				pollRangeSelection();
			}
		};

		gridContainer.addEventListener("mouseup", handleMouseUp);
		gridContainer.addEventListener("keyup", handleKeyUp);

		return () => {
			gridContainer?.removeEventListener("mouseup", handleMouseUp);
			gridContainer?.removeEventListener("keyup", handleKeyUp);
		};
	});

	$effect(() => {
		return () => {
			if (scrollDebounceTimer !== null) {
				clearTimeout(scrollDebounceTimer);
			}
		};
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

	const handleScroll = async (event: CustomEvent) => {
		const { detail } = event;
		if (!detail || !qrateStore.isFileOpen) return;

		if (scrollDebounceTimer !== null) {
			clearTimeout(scrollDebounceTimer);
		}

		scrollDebounceTimer = window.setTimeout(async () => {
			if (!grid) return;

			const currentRowCount = qrateStore.rows.length;
			const totalRows = qrateStore.totalRows;
			const scrollTop = detail?.scrollTop ?? detail?.y ?? 0;
			const viewportHeight =
				detail?.viewportHeight ?? detail?.height ?? 0;
			const contentHeight =
				detail?.contentHeight ?? detail?.virtualSize ?? 0;
			const lastVisibleRow =
				detail?.lastVisibleRow ?? detail?.endRow ?? 0;

			const scrollPercentage =
				contentHeight > 0
					? (scrollTop + viewportHeight) / contentHeight
					: 0;
			const shouldLoad =
				(scrollPercentage > 0.8 && contentHeight > 0) ||
				(lastVisibleRow > 0 &&
					lastVisibleRow >= currentRowCount - 20) ||
				(scrollTop > 0 &&
					currentRowCount < totalRows &&
					currentRowCount > 0 &&
					Math.abs(scrollTop + viewportHeight - contentHeight) < 100);

			if (
				shouldLoad &&
				!isLoadingMore &&
				qrateStore.hasMoreRows &&
				!qrateStore.isLoadingMore
			) {
				isLoadingMore = true;
				await qrateStore.loadMoreRows(100).finally(() => {
					isLoadingMore = false;
				});
			}
		}, 150);
	};

	const handleAfterFocus = async (event: CustomEvent) => {
		const { detail } = event;
		if (!detail) return;

		const rowId = detail.model?.row_id ?? null;
		const colProp = detail.column?.prop ?? null;

		qrateStore.selectRow(rowId);
		qrateStore.selectColumn(colProp === "_rowNum" ? null : colProp);
		await pollRangeSelection();
	};

	const handleContextMenuOpen = () => {
		contextRowId = qrateStore.selectedRowId;
		contextColumnId = qrateStore.selectedColumnId;
	};

	const handleNativeContextMenu = (e: MouseEvent) => {
		lastContextMenuPosition = { x: e.clientX, y: e.clientY };
	};

	const openAddCommentDialog = () => {
		commentRowId = contextRowId;
		commentColumnId = contextColumnId;
		popoverAnchor = lastContextMenuPosition;
		addCommentDialogOpen = true;
	};

	const closeAddCommentDialog = () => {
		addCommentDialogOpen = false;
		commentRowId = null;
		commentColumnId = null;
	};
</script>

<ContextMenu.Root onOpenChange={(open) => open && handleContextMenuOpen()}>
	<!-- svelte-ignore a11y_no_static_element_interactions -->
	<ContextMenu.Trigger
		class="flex h-full w-full flex-col overflow-hidden"
		bind:ref={gridContainer}
		oncontextmenu={handleNativeContextMenu}
	>
		{#if !qrateStore.isFileOpen}
			<div class="flex h-full w-full items-center justify-center">
				<div class="text-center text-muted-foreground">
					<p class="mb-2 text-lg">No project open</p>
					<p class="text-sm">
						Open a project folder or import a CSV to get started
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
			<!-- Grid Controls -->
			<!-- <div class="mb-2 flex items-center justify-between gap-4">
			<div class="flex items-center gap-2">
				<Label for="row-height" class="text-xs text-muted-foreground">
					Row Height:
				</Label>
				<div class="flex items-center gap-1">
					<Button
						variant="ghost"
						size="icon-sm"
						class="size-7"
						onclick={() => {
							if (defaultRowHeight > 20) {
								defaultRowHeight -= 5;
								grid?.refresh?.();
							}
						}}
						disabled={defaultRowHeight <= 20}
						title="Decrease row height"
					>
						<ArrowDownIcon class="size-3" />
					</Button>
					<Input
						id="row-height"
						type="number"
						min="20"
						max="100"
						step="5"
						bind:value={defaultRowHeight}
						class="h-7 w-16 text-center text-xs"
						onchange={() => grid?.refresh?.()}
					/>
					<Button
						variant="ghost"
						size="icon-sm"
						class="size-7"
						onclick={() => {
							if (defaultRowHeight < 100) {
								defaultRowHeight += 5;
								grid?.refresh?.();
							}
						}}
						disabled={defaultRowHeight >= 100}
						title="Increase row height"
					>
						<ArrowUpIcon class="size-3" />
					</Button>
					<span class="ml-1 text-xs text-muted-foreground">px</span>
				</div>
			</div>
		</div> -->

			<div class="min-h-0 flex-1 overflow-hidden">
				{#if isMounted && RevoGridComponent}
					<RevoGridComponent
						bind:this={grid}
						source={revoRows}
						columns={revoColumns}
						theme={isDark ? "darkMaterial" : "default"}
						resize={true}
						range={true}
						readonly={false}
						autoSizeColumn={false}
						rowSize={defaultRowHeight}
						on:afteredit={handleAfterEdit}
						on:aftercolumnresize={handleAfterColumnResize}
						on:afterfocus={handleAfterFocus}
						on:viewportscroll={handleScroll}
					/>
				{:else}
					<div class="flex h-full items-center justify-center">
						<div
							class="flex items-center gap-2 text-sm text-muted-foreground"
						>
							<div
								class="size-4 animate-spin rounded-full border-2 border-current border-t-transparent"
							></div>
							<span>Loading grid...</span>
						</div>
					</div>
				{/if}
			</div>

			{#if isLoadingMore || qrateStore.isLoadingMore}
				<div
					class="fixed bottom-20 right-4 z-50 rounded-lg border border-border bg-background/95 px-4 py-2 shadow-lg backdrop-blur-sm"
				>
					<div
						class="flex items-center gap-2 text-sm text-muted-foreground"
					>
						<div
							class="size-4 animate-spin rounded-full border-2 border-current border-t-transparent"
						></div>
						<span>Loading more rows...</span>
					</div>
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
	</ContextMenu.Trigger>

	<ContextMenu.Content class="w-48">
		<ContextMenu.Item
			onclick={openAddCommentDialog}
			disabled={contextRowId === null}
		>
			<MessageSquarePlusIcon class="mr-2 size-4" />
			Add Comment
		</ContextMenu.Item>
	</ContextMenu.Content>
</ContextMenu.Root>

<!-- Add Comment Popover -->
{#if popoverAnchor}
	<AddCommentDialog
		bind:open={addCommentDialogOpen}
		rowId={commentRowId}
		columnId={commentColumnId}
		onClose={closeAddCommentDialog}
		anchorPosition={popoverAnchor}
	/>
{/if}

<style>
	:global(.row-number-cell) {
		background-color: var(--muted) !important;
		color: var(--muted-foreground) !important;
		font-size: 0.75rem !important;
		text-align: center !important;
		user-select: none !important;
	}

	:global(.has-annotation) {
		position: relative;
	}

	:global(.has-annotation::after) {
		content: "";
		position: absolute;
		top: 2px;
		right: 2px;
		width: 0;
		height: 0;
		border-left: 6px solid transparent;
		border-top: 6px solid hsl(var(--primary));
	}
</style>
