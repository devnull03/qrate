<script lang="ts">
	import type { Sheet, SpreadsheetData } from "$lib/interfaces/sheet.interface";
	import {
		encodeCell,
		decodeCell,
		encodeCol,
		getBorder,
		clearSelection,
		pasteSelection,
		computeStyles,
		getColumnWidth,
		getRowHeight,
		handleKeyboardNavigation,
		normalizeSpreadsheetData,
		defaultConfig,
		type SpreadsheetConfig
	} from "$lib/utils/spreadsheet";
	import { cn } from "$lib/utils";
	import { Expand } from "@lucide/svelte";

	interface Props {
		items: Sheet;
		config?: Partial<SpreadsheetConfig>;
		class?: string;
		onViewItemClick?: (
			img: string,
			itemFields: Record<string, any> | null,
			filename: string
		) => void;
		editingApi?: any;
		moveToCompleted?: (index: number) => void;
		moveToTodo?: (index: number) => void;
		isDoneSection?: boolean;
		originalIndices?: number[];
	}

	let {
		items = $bindable(),
		config = {},
		class: className,
		onViewItemClick: onItemClick = $bindable(),
		editingApi = $bindable(null),
		moveToCompleted = $bindable(() => {}),
		moveToTodo = $bindable(() => {}),
		isDoneSection = false,
		originalIndices = []
	}: Props = $props();

	// Focus action for better accessibility
	function focus(node: HTMLElement) {
		node.focus();
		return {
			destroy() {
				// cleanup if needed
			}
		};
	}

	// Convert Sheet to SpreadsheetData format
	let spreadsheetData = $derived.by(() => {
		if (!items) return null;

		// Create columns from fields
		const columns = items.fields.map((field) => ({
			title: field.title,
			width: "150px",
			type: "text" as const,
			readOnly: false,
			align: "left" as const
		}));

		// Convert rows to 2D array
		const data = items.rows.map((row) => items.fields.map((field) => row[field.title] || ""));

		return {
			data: normalizeSpreadsheetData(data, 15, Math.max(columns.length, 6)),
			columns,
			rows: data.map(() => ({ height: "28px" })),
			mergeCells: {},
			style: {},
			selected: null as [string, string] | null,
			currentValue: ""
		};
	});

	// Spreadsheet state
	let selected = $state<[string, string] | null>(null);
	let clipboard = $state<[string, string] | null>(null);
	let currentValue = $state<string>("");
	let editing = $state<[number, number] | null>(null);
	let shiftPressed = $state(false);
	let ctrlPressed = $state(false);
	let validationErrors = $state<Record<string, string>>({});

	// Column resizing state
	let columnWidths = $state<Record<number, number>>({});
	let resizing = $state<{ colIndex: number; startX: number; startWidth: number } | null>(null);

	// Configuration
	const finalConfig = $derived({ ...defaultConfig, ...config });

	// Get current cell value when selection changes
	$effect(() => {
		if (selected && spreadsheetData) {
			const [start] = selected;
			const { c, r } = decodeCell(start);
			currentValue = spreadsheetData.data[r]?.[c] || "";
		}
	});

	// Sync changes back to items
	$effect(() => {
		if (spreadsheetData && items) {
			// Convert back to Sheet format
			const updatedRows = spreadsheetData.data
				.map((row) => {
					const rowObject: Record<string, any> = {};
					row.forEach((value, index) => {
						const fieldTitle = items.fields[index]?.title;
						if (fieldTitle && value !== "") {
							rowObject[fieldTitle] = value;
						}
					});
					return rowObject;
				})
				.filter((row) => Object.keys(row).length > 0);

			if (JSON.stringify(updatedRows) !== JSON.stringify(items.rows)) {
				items.rows = updatedRows;
			}
		}
	});

	// Event handlers
	function handleCellClick(colIndex: number, rowIndex: number, event: MouseEvent) {
		const cellAddress = encodeCell({ c: colIndex, r: rowIndex });

		if (shiftPressed && selected) {
			selected = [selected[0], cellAddress];
		} else {
			selected = [cellAddress, cellAddress];
		}

		editing = null; // Stop editing when clicking
	}

	function handleCellDoubleClick(colIndex: number, rowIndex: number) {
		editing = [colIndex, rowIndex];
		const cellAddress = encodeCell({ c: colIndex, r: rowIndex });
		selected = [cellAddress, cellAddress];
	}

	function handleInputChange(value: string, colIndex: number, rowIndex: number) {
		if (!spreadsheetData || !editingApi) return;

		// Get the field name for this column
		const fieldName = items.fields[colIndex]?.title;
		if (!fieldName) return;

		// Clear previous validation error for this cell
		const cellKey = `${rowIndex}-${colIndex}`;
		delete validationErrors[cellKey];
		validationErrors = { ...validationErrors };

		// Use the editing API to update the cell with validation
		editingApi.editCell(rowIndex, fieldName, value).then((result: any) => {
			if (!result.success) {
				console.warn("Cell edit failed:", result.error);
				// Store validation error for this cell
				validationErrors[cellKey] = result.error;
				validationErrors = { ...validationErrors };
			}
		});

		// Also update local spreadsheet data for immediate UI feedback
		if (!spreadsheetData.data[rowIndex]) {
			spreadsheetData.data[rowIndex] = [];
		}
		spreadsheetData.data[rowIndex][colIndex] = value;

		// Trigger reactivity
		spreadsheetData.data = [...spreadsheetData.data];
	}

	function handleInputBlur() {
		editing = null;
	}

	// Validate a cell value before committing
	function validateCellInput(value: string, colIndex: number, rowIndex: number): boolean {
		if (!editingApi) return true;

		const fieldName = items.fields[colIndex]?.title;
		if (!fieldName) return true;

		const validation = editingApi.validateValue(fieldName, value);
		const cellKey = `${rowIndex}-${colIndex}`;

		if (!validation.isValid) {
			validationErrors[cellKey] = validation.error;
			validationErrors = { ...validationErrors };
			return false;
		} else {
			delete validationErrors[cellKey];
			validationErrors = { ...validationErrors };
			return true;
		}
	}

	function handleKeyDown(event: KeyboardEvent) {
		// Track modifier keys
		shiftPressed = event.shiftKey;
		ctrlPressed = event.ctrlKey || event.metaKey;

		// Handle keyboard navigation
		if (["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "Enter", "Tab"].includes(event.key)) {
			event.preventDefault();
			const newSelection = handleKeyboardNavigation(
				event,
				selected,
				spreadsheetData?.data || [],
				shiftPressed
			);
			if (newSelection) {
				selected = newSelection;
			}
			editing = null;
		}

		// Handle copy/paste
		if (ctrlPressed) {
			switch (event.key.toLowerCase()) {
				case "c":
					event.preventDefault();
					clipboard = selected;
					break;
				case "v":
					event.preventDefault();
					if (clipboard && selected && spreadsheetData && editingApi) {
						// Use editing API for paste operations with validation
						const clipboardBounds = getBorder(clipboard);
						const selectedBounds = getBorder(selected);

						const edits = [];
						for (let r = selectedBounds.tl.r; r <= selectedBounds.br.r; r++) {
							for (let c = selectedBounds.tl.c; c <= selectedBounds.br.c; c++) {
								const sourceR =
									clipboardBounds.tl.r +
									((r - selectedBounds.tl.r) % (clipboardBounds.br.r - clipboardBounds.tl.r + 1));
								const sourceC =
									clipboardBounds.tl.c +
									((c - selectedBounds.tl.c) % (clipboardBounds.br.c - clipboardBounds.tl.c + 1));
								const sourceValue = spreadsheetData.data[sourceR]?.[sourceC] || "";
								const fieldName = items.fields[c]?.title;

								if (fieldName) {
									edits.push({ rowIndex: r, fieldName, value: sourceValue });
								}
							}
						}

						// Use batch edit for better performance
						editingApi.batchEdit(edits);

						// Also update local data for immediate UI feedback
						spreadsheetData.data = pasteSelection(spreadsheetData.data, clipboard, selected);
					}
					break;
				case "x":
					event.preventDefault();
					clipboard = selected;
					if (selected && spreadsheetData && editingApi) {
						// Use editing API for cut operations
						const bounds = getBorder(selected);
						const edits = [];

						for (let r = bounds.tl.r; r <= bounds.br.r; r++) {
							for (let c = bounds.tl.c; c <= bounds.br.c; c++) {
								const fieldName = items.fields[c]?.title;
								if (fieldName) {
									edits.push({ rowIndex: r, fieldName, value: "" });
								}
							}
						}

						editingApi.batchEdit(edits);
						spreadsheetData.data = clearSelection(spreadsheetData.data, selected);
					}
					break;
			}
		}

		// Handle delete
		if (event.key === "Delete" && selected && spreadsheetData && editingApi) {
			event.preventDefault();

			// Use editing API for delete operations
			const bounds = getBorder(selected);
			const edits = [];

			for (let r = bounds.tl.r; r <= bounds.br.r; r++) {
				for (let c = bounds.tl.c; c <= bounds.br.c; c++) {
					const fieldName = items.fields[c]?.title;
					if (fieldName) {
						edits.push({ rowIndex: r, fieldName, value: "" });
					}
				}
			}

			editingApi.batchEdit(edits);
			spreadsheetData.data = clearSelection(spreadsheetData.data, selected);
		}

		// Handle escape
		if (event.key === "Escape") {
			editing = null;
			selected = null;
		}
	}

	function handleKeyUp(event: KeyboardEvent) {
		shiftPressed = event.shiftKey;
		ctrlPressed = event.ctrlKey || event.metaKey;
	}

	function handleColumnHeaderClick(colIndex: number) {
		const start = encodeCell({ c: colIndex, r: 0 });
		const end = encodeCell({ c: colIndex, r: (spreadsheetData?.data.length || 10) - 1 });
		selected = [start, end];
		editing = null;
	}

	function handleRowHeaderClick(rowIndex: number) {
		const start = encodeCell({ c: 0, r: rowIndex });
		const end = encodeCell({ c: (spreadsheetData?.columns.length || 6) - 1, r: rowIndex });
		selected = [start, end];
		editing = null;
	}

	// Get selection boundaries for styling
	const selectionBounds = $derived.by(() => {
		if (!selected) return null;
		return getBorder(selected);
	});

	function isCellSelected(colIndex: number, rowIndex: number): boolean {
		if (!selectionBounds) return false;
		return (
			colIndex >= selectionBounds.tl.c &&
			colIndex <= selectionBounds.br.c &&
			rowIndex >= selectionBounds.tl.r &&
			rowIndex <= selectionBounds.br.r
		);
	}

	function isColumnSelected(colIndex: number): boolean {
		if (!selectionBounds) return false;
		return colIndex >= selectionBounds.tl.c && colIndex <= selectionBounds.br.c;
	}

	function isRowSelected(rowIndex: number): boolean {
		if (!selectionBounds) return false;
		return rowIndex >= selectionBounds.tl.r && rowIndex <= selectionBounds.br.r;
	}

	// Column resizing functions
	function handleResizeStart(event: MouseEvent, colIndex: number) {
		event.preventDefault();
		event.stopPropagation();

		const currentWidth = columnWidths[colIndex] || 150; // Default width
		resizing = {
			colIndex,
			startX: event.clientX,
			startWidth: currentWidth
		};

		document.addEventListener("mousemove", handleResizeMove);
		document.addEventListener("mouseup", handleResizeEnd);
		document.body.style.cursor = "col-resize";
		document.body.style.userSelect = "none";
	}

	function handleResizeMove(event: MouseEvent) {
		if (!resizing) return;

		const deltaX = event.clientX - resizing.startX;
		const newWidth = Math.max(50, resizing.startWidth + deltaX); // Minimum width of 50px

		columnWidths[resizing.colIndex] = newWidth;
	}

	function handleResizeEnd() {
		resizing = null;
		document.removeEventListener("mousemove", handleResizeMove);
		document.removeEventListener("mouseup", handleResizeEnd);
		document.body.style.cursor = "";
		document.body.style.userSelect = "";
	}

	function getCurrentColumnWidth(colIndex: number): string {
		return `${columnWidths[colIndex] || 150}px`;
	}

	// Cleanup event listeners on component destroy
	$effect(() => {
		return () => {
			if (resizing) {
				handleResizeEnd();
			}
		};
	});
</script>

<svelte:window onkeydown={handleKeyDown} onkeyup={handleKeyUp} />

{#if spreadsheetData}
	<div
		class={cn(
			"spreadsheet-container border rounded-lg overflow-hidden bg-background flex flex-col w-full max-w-[calc(100vw-20rem)]",
			className
		)}
		tabindex="0"
		role="grid"
		aria-label="Spreadsheet"
	>
		<!-- Simplified toolbar (no formula bar) -->
		<div class="toolbar flex items-center justify-between p-2 border-b bg-muted/30">
			<div class="flex items-center gap-4">
				<span class="text-sm text-muted-foreground">
					{spreadsheetData.data.length} rows × {spreadsheetData.columns.length} columns
				</span>
				{#if selected}
					<span class="text-sm text-muted-foreground">
						Selected: {selected[0]}{selected[1] && selected[1] !== selected[0]
							? `:${selected[1]}`
							: ""}
					</span>
				{/if}
			</div>
			<div class="flex items-center gap-2 text-xs text-muted-foreground">
				<span>Double-click: Edit</span>
				<span>Ctrl+C/V: Copy/Paste</span>
				<span>Del: Clear</span>
			</div>
		</div>

		<!-- Spreadsheet table -->
		<div
			class="spreadsheet-table overflow-x-auto overflow-y-auto flex-1 w-full max-w-[calc(100vw-18rem)]"
			style={`height: ${finalConfig.tableHeight || "100%"}`}
		>
			<table class="border-collapse w-full max-w-[calc(100vw-18rem)]">
				<!-- Column headers -->
				<thead class="sticky top-0 bg-muted/50 border-b">
					<tr>
						<!-- Top-left corner cell -->
						<th class="w-12 h-8 border-r border-b bg-muted text-center"></th>

						<!-- Expand button header -->
						<th class="w-16 h-8 border-r border-b bg-muted text-center text-xs">Action</th>

						<!-- Column headers -->
						{#each spreadsheetData.columns as column, colIndex}
							<th
								class={cn(
									"h-8 px-2 border-r border-b bg-muted text-center text-sm font-medium cursor-pointer hover:bg-muted/80 relative",
									isColumnSelected(colIndex) && "bg-primary/20"
								)}
								style={`width: ${getCurrentColumnWidth(colIndex)}; max-width: ${getCurrentColumnWidth(colIndex)};`}
								onclick={() => handleColumnHeaderClick(colIndex)}
								role="columnheader"
							>
								{column.title || encodeCol(colIndex)}

								<!-- Resize handle -->
								<!-- svelte-ignore a11y_no_static_element_interactions -->
								<div
									class="absolute right-0 top-0 w-2 h-full cursor-col-resize hover:bg-primary/30 transition-colors"
									onmousedown={(e) => handleResizeStart(e, colIndex)}
									title="Drag to resize column"
								></div>
							</th>
						{/each}
					</tr>
				</thead>

				<!-- Table body -->
				<tbody>
					{#each spreadsheetData.data as row, rowIndex}
						<tr>
							<!-- Row header -->
							<td
								class={cn(
									"w-12 h-8 border-r border-b bg-muted text-center text-sm font-medium cursor-pointer hover:bg-muted/80 sticky left-0",
									isRowSelected(rowIndex) && "bg-primary/20"
								)}
								onclick={() => handleRowHeaderClick(rowIndex)}
								role="rowheader"
							>
								{rowIndex + 1}
							</td>

							<!-- Expand button and checkbox -->
							<td class="w-16 h-8 border-r border-b bg-background text-center p-1">
								<div class="flex items-center justify-center gap-1">
									<!-- Checkbox for todo/done -->
									<input
										type="checkbox"
										checked={isDoneSection}
										onchange={(e) => {
											const target = e.target as HTMLInputElement;
											const originalIndex = originalIndices?.[rowIndex] ?? rowIndex;
											if (target.checked && moveToCompleted) {
												moveToCompleted(originalIndex);
											} else if (!target.checked && moveToTodo) {
												moveToTodo(originalIndex);
											}
										}}
										class="w-3 h-3"
										title={isDoneSection ? "Mark as Todo" : "Mark as Done"}
									/>

									<!-- Expand button -->
									<button
										type="button"
										class="w-6 h-6 flex items-center justify-center rounded hover:bg-muted transition-colors"
										onclick={async (e) => {
											if (onItemClick && items?.rows[rowIndex]) {
												console.log("Expand clicked for row:", rowIndex, items.rows[rowIndex]);

												let image = items.images.find(
													(img) => img[0] === items.rows[rowIndex].file
												);

												console.log(image);

												if (image) {
													const file = await image[1].getFile();
													if (file) {
														const img = URL.createObjectURL(file);
														const itemFields = items.rows.find(
															(row) => row.file === items.rows[rowIndex].file
														) as Record<string, any> | null;
														onItemClick(img, itemFields, image[0]);
													}
												}
											}
										}}
										aria-label={`Expand row ${rowIndex + 1}`}
									>
										<Expand class="size-3" />
									</button>
								</div>
							</td>

							<!-- Data cells -->
							{#each spreadsheetData.columns as column, colIndex}
								{@const cellKey = `${rowIndex}-${colIndex}`}
								{@const hasValidationError = validationErrors[cellKey]}
								<td
									class={cn(
										"h-8 border-r border-b p-0 relative",
										isCellSelected(colIndex, rowIndex) && "bg-primary/10",
										editing?.[0] === colIndex && editing?.[1] === rowIndex && "bg-primary/20",
										hasValidationError && "bg-destructive/10 border-destructive/50"
									)}
									style={`width: ${getCurrentColumnWidth(colIndex)}; max-width: ${getCurrentColumnWidth(colIndex)}; ${computeStyles(
										colIndex,
										rowIndex,
										row,
										spreadsheetData.style,
										finalConfig,
										row[colIndex],
										row[colIndex + 1]
									)}`}
									onclick={(e) => handleCellClick(colIndex, rowIndex, e)}
									ondblclick={() => handleCellDoubleClick(colIndex, rowIndex)}
									role="gridcell"
									tabindex="-1"
									title={hasValidationError ? `Validation Error: ${hasValidationError}` : undefined}
								>
									{#if editing?.[0] === colIndex && editing?.[1] === rowIndex}
										<input
											type="text"
											value={row[colIndex] || ""}
											class={cn(
												"w-full h-full px-2 text-sm border-0 bg-transparent focus:outline-none focus:ring-2 focus:ring-primary/50",
												hasValidationError && "text-destructive focus:ring-destructive/50"
											)}
											oninput={(e) => handleInputChange(e.currentTarget.value, colIndex, rowIndex)}
											onblur={handleInputBlur}
											use:focus
										/>
									{:else}
										<div class="px-2 py-1 text-sm w-full h-full flex items-center overflow-hidden">
											<span class={cn("truncate", hasValidationError && "text-destructive")}>
												{row[colIndex] || ""}
											</span>
										</div>
									{/if}

									<!-- Validation error indicator -->
									{#if hasValidationError}
										<div
											class="absolute top-0 right-0 w-2 h-2 bg-destructive rounded-full transform translate-x-1 -translate-y-1"
										></div>
									{/if}
								</td>
							{/each}
						</tr>
					{/each}
				</tbody>
			</table>
		</div>
	</div>
{/if}

<style>
	.spreadsheet-container {
		--cell-height: 32px;
		--header-height: 40px;
		min-height: 400px;
		/* Ensure container respects parent width */
		box-sizing: border-box;
	}

	.spreadsheet-table {
		/* Enable smooth scrolling */
		scroll-behavior: smooth;
		/* Ensure table container is constrained */
		max-width: 100%;
	}

	/* Improve table rendering performance and enforce column widths */
	.spreadsheet-table table {
		table-layout: fixed;
	}

	/* Selection styling */
	.spreadsheet-container:focus-within {
		outline: 2px solid hsl(var(--primary));
		outline-offset: -2px;
	}

	/* Cell border styling for better visual separation */
	td,
	th {
		border-color: hsl(var(--border));
	}

	/* Ensure text truncates properly in fixed width columns */
	td div {
		overflow: hidden;
		max-width: 100%;
	}

	td div span {
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
		display: block;
	}
</style>
