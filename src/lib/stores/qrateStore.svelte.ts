import { invoke } from "@tauri-apps/api/core";
import {
	workspaceStore,
	saveWorkspace,
	clearWorkspace,
	updateViewport,
	initWorkspace,
	getWorkspace,
} from "./workspaceStore";
import { addRecentFile } from "./recentFiles";
import { getFileName } from "$lib/utils/path";

export interface CurrentStateResponse {
	is_file_open: boolean;
	path: string | null;
	columns: ColumnDef[];
	total_rows: number;
	rows: Record<string, any>[];
	offset: number;
	limit: number;
}

export interface ColumnDef {
	id: string;
	name: string;
	col_type: string;
	width: number;
	hidden: boolean;
}

export interface FileOpenResponse {
	path: string;
	columns: ColumnDef[];
	total_rows: number;
}

export interface DataResponse {
	rows: Record<string, any>[];
	total: number;
}

class QrateStore {
	// Current open file path
	currentFilePath = $state<string | null>(null);

	// Column definitions
	columns = $state<ColumnDef[]>([]);

	// Total number of rows in the database
	totalRows = $state<number>(0);

	// Currently loaded rows (viewport data)
	rows = $state<Record<string, any>[]>([]);

	// Loading states
	isLoading = $state<boolean>(false);
	isFileOpen = $state<boolean>(false);

	// Error state
	error = $state<string | null>(null);

	// Viewport tracking for virtual scrolling
	currentOffset = $state<number>(0);
	currentLimit = $state<number>(100); // Initial chunk size

	// Selected row for file viewing in sidebar
	selectedRowId = $state<number | null>(null);

	// Lazy loading state
	isLoadingMore = $state<boolean>(false);
	hasMoreRows = $state<boolean>(true);

	// Track if we've attempted restoration
	private restorationAttempted = false;

	/**
	 * Sync state from the Rust backend (for cross-window state sharing)
	 * This is the preferred method for the main window to get current state
	 */
	async syncFromBackend(): Promise<boolean> {
		console.log("[qrateStore] syncFromBackend called");
		try {
			this.isLoading = true;
			this.error = null;

			console.log("[qrateStore] Invoking get_current_state...");
			const response =
				await invoke<CurrentStateResponse>("get_current_state");
			console.log("[qrateStore] get_current_state response:", {
				is_file_open: response.is_file_open,
				path: response.path,
				columns_count: response.columns?.length,
				total_rows: response.total_rows,
			});

			if (response.is_file_open && response.path) {
				this.currentFilePath = response.path;
				this.columns = response.columns;
				this.totalRows = response.total_rows;
				this.rows = response.rows;
				this.currentOffset = response.offset;
				this.currentLimit = response.limit;
				this.isFileOpen = true;

				console.log(
					"[qrateStore] Synced state from backend:",
					response.path,
				);
				return true;
			} else {
				// No file open in backend
				console.log(
					"[qrateStore] No file open in backend, resetting store",
				);
				this.reset();
				return false;
			}
		} catch (err) {
			this.error = err instanceof Error ? err.message : String(err);
			console.error("[qrateStore] Failed to sync from backend:", err);
			return false;
		} finally {
			this.isLoading = false;
			console.log(
				"[qrateStore] syncFromBackend finished, isLoading:",
				this.isLoading,
			);
		}
	}

	/**
	 * Restore workspace from persistent storage (call on app init)
	 * @param force - If true, will attempt restoration even if already attempted
	 */
	async restoreWorkspace(force: boolean = false): Promise<boolean> {
		if (this.restorationAttempted && !force) return false;
		if (!force) this.restorationAttempted = true;

		// First try to sync from backend (handles cross-window state)
		const syncedFromBackend = await this.syncFromBackend();
		if (syncedFromBackend) {
			return true;
		}

		// Fall back to workspace store for persistence across app restarts
		try {
			// Initialize the workspace store
			await initWorkspace();

			const workspace = getWorkspace();

			if (workspace.currentFilePath) {
				console.log(
					"[qrateStore] Restoring workspace:",
					workspace.currentFilePath,
				);
				await this.openFile(workspace.currentFilePath);

				// Restore viewport position
				if (
					workspace.currentOffset > 0 ||
					workspace.currentLimit !== 100
				) {
					await this.loadRows(
						workspace.currentOffset,
						workspace.currentLimit,
					);
				}

				return true;
			}
		} catch (err) {
			console.warn("[qrateStore] Failed to restore workspace:", err);
			// Clear invalid workspace state
			await clearWorkspace();
		}

		return false;
	}

	/**
	 * Create a new .qrate file
	 */
	async createFile(path: string): Promise<void> {
		try {
			this.isLoading = true;
			this.error = null;

			const response = await invoke<FileOpenResponse>(
				"create_qrate_file",
				{ path },
			);

			this.currentFilePath = response.path;
			this.columns = response.columns;
			this.totalRows = response.total_rows;
			this.rows = [];
			this.isFileOpen = true;

			// Save to persistent workspace
			await saveWorkspace(response.path, 0, this.currentLimit);

			// Add to recent files
			await addRecentFile(path, getFileName(path));
		} catch (err) {
			this.error = err instanceof Error ? err.message : String(err);
			console.error("Failed to create file:", err);
			throw err;
		} finally {
			this.isLoading = false;
		}
	}

	/**
	 * Open an existing .qrate file
	 */
	async openFile(path: string): Promise<void> {
		try {
			this.isLoading = true;
			this.error = null;

			const response = await invoke<FileOpenResponse>("open_qrate_file", {
				path,
			});

			this.currentFilePath = response.path;
			this.columns = response.columns;
			this.totalRows = response.total_rows;
			this.isFileOpen = true;

			// Load initial chunk of data
			if (this.totalRows > 0) {
				await this.loadRows(0, 100);
				this.hasMoreRows = this.totalRows > this.rows.length;
			} else {
				this.rows = [];
				this.hasMoreRows = false;
			}

			// Save to persistent workspace
			await saveWorkspace(response.path, 0, this.currentLimit);

			// Add to recent files
			await addRecentFile(path, getFileName(path));
		} catch (err) {
			this.error = err instanceof Error ? err.message : String(err);
			console.error("Failed to open file:", err);
			throw err;
		} finally {
			this.isLoading = false;
		}
	}

	/**
	 * Close the current file
	 */
	async closeFile(): Promise<void> {
		if (!this.currentFilePath) return;

		try {
			await invoke("close_qrate_file", { path: this.currentFilePath });
			this.reset();

			// Clear persistent workspace
			await clearWorkspace();
		} catch (err) {
			this.error = err instanceof Error ? err.message : String(err);
			console.error("Failed to close file:", err);
			throw err;
		}
	}

	/**
	 * Load a specific range of rows (virtual scrolling)
	 */
	async loadRows(offset: number, limit: number): Promise<void> {
		if (!this.currentFilePath) {
			throw new Error("No file is currently open");
		}

		try {
			const response = await invoke<DataResponse>("get_rows", {
				path: this.currentFilePath,
				limit,
				offset,
			});

			this.rows = response.rows;
			this.currentOffset = offset;
			this.currentLimit = limit;
			this.totalRows = response.total;
			this.hasMoreRows = offset + response.rows.length < response.total;

			// Update persistent viewport state
			await updateViewport(offset, limit);
		} catch (err) {
			this.error = err instanceof Error ? err.message : String(err);
			console.error("Failed to load rows:", err);
			throw err;
		}
	}

	/**
	 * Load more rows (append to existing rows for lazy loading)
	 */
	async loadMoreRows(count: number = 100): Promise<void> {
		if (!this.currentFilePath || this.isLoadingMore || !this.hasMoreRows) {
			return;
		}

		try {
			this.isLoadingMore = true;
			const nextOffset = this.rows.length;

			const response = await invoke<DataResponse>("get_rows", {
				path: this.currentFilePath,
				limit: count,
				offset: nextOffset,
			});

			// Append new rows to existing rows
			this.rows = [...this.rows, ...response.rows];
			this.totalRows = response.total;
			this.hasMoreRows = this.rows.length < response.total;

			// Update persistent viewport state
			await updateViewport(0, this.rows.length);
		} catch (err) {
			this.error = err instanceof Error ? err.message : String(err);
			console.error("Failed to load more rows:", err);
		} finally {
			this.isLoadingMore = false;
		}
	}

	/**
	 * Update a single cell value
	 */
	async updateCell(
		rowId: number,
		columnId: string,
		value: string,
	): Promise<void> {
		if (!this.currentFilePath) {
			throw new Error("No file is currently open");
		}

		try {
			await invoke("update_cell", {
				path: this.currentFilePath,
				rowId,
				columnId,
				value,
			});

			// Update local state
			const rowIndex = this.rows.findIndex((row) => row.row_id === rowId);
			if (rowIndex !== -1) {
				this.rows[rowIndex][columnId] = value;
			}
		} catch (err) {
			this.error = err instanceof Error ? err.message : String(err);
			console.error("Failed to update cell:", err);
			throw err;
		}
	}

	/**
	 * Add a new column
	 */
	async addColumn(column: ColumnDef): Promise<void> {
		if (!this.currentFilePath) {
			throw new Error("No file is currently open");
		}

		try {
			await invoke("add_column", {
				path: this.currentFilePath,
				column,
			});

			// Update local state
			this.columns = [...this.columns, column];

			// Reload current viewport to get new column data
			await this.loadRows(this.currentOffset, this.currentLimit);
		} catch (err) {
			this.error = err instanceof Error ? err.message : String(err);
			console.error("Failed to add column:", err);
			throw err;
		}
	}

	/**
	 * Update column metadata (width, hidden, etc.)
	 */
	async updateColumn(column: ColumnDef): Promise<void> {
		if (!this.currentFilePath) {
			throw new Error("No file is currently open");
		}

		try {
			await invoke("update_column", {
				path: this.currentFilePath,
				column,
			});

			// Update local state
			const colIndex = this.columns.findIndex((c) => c.id === column.id);
			if (colIndex !== -1) {
				this.columns[colIndex] = column;
			}
		} catch (err) {
			this.error = err instanceof Error ? err.message : String(err);
			console.error("Failed to update column:", err);
			throw err;
		}
	}

	/**
	 * Insert a new row
	 */
	async insertRow(values: Record<string, any>): Promise<number> {
		if (!this.currentFilePath) {
			throw new Error("No file is currently open");
		}

		try {
			const rowId = await invoke<number>("insert_row", {
				path: this.currentFilePath,
				values,
			});

			this.totalRows += 1;

			// Reload current viewport to show new row if in range
			await this.loadRows(this.currentOffset, this.currentLimit);

			return rowId;
		} catch (err) {
			this.error = err instanceof Error ? err.message : String(err);
			console.error("Failed to insert row:", err);
			throw err;
		}
	}

	/**
	 * Delete a row
	 */
	async deleteRow(rowId: number): Promise<void> {
		if (!this.currentFilePath) {
			throw new Error("No file is currently open");
		}

		try {
			await invoke("delete_row", {
				path: this.currentFilePath,
				rowId,
			});

			this.totalRows -= 1;

			// Remove from local state
			this.rows = this.rows.filter((row) => row.row_id !== rowId);
		} catch (err) {
			this.error = err instanceof Error ? err.message : String(err);
			console.error("Failed to delete row:", err);
			throw err;
		}
	}

	/**
	 * Import CSV data into a .qrate file
	 */
	async importCsv(qratePath: string, csvPath: string): Promise<void> {
		try {
			this.isLoading = true;
			this.error = null;

			const response = await invoke<FileOpenResponse>(
				"import_csv_to_qrate",
				{
					qratePath,
					csvPath,
				},
			);

			this.currentFilePath = response.path;
			this.columns = response.columns;
			this.totalRows = response.total_rows;
			this.isFileOpen = true;

			// Load initial chunk of data
			if (this.totalRows > 0) {
				await this.loadRows(0, 100);
				this.hasMoreRows = this.totalRows > this.rows.length;
			} else {
				this.rows = [];
				this.hasMoreRows = false;
			}

			// Save to persistent workspace
			await saveWorkspace(response.path, 0, this.currentLimit);

			// Add to recent files
			await addRecentFile(qratePath, getFileName(qratePath));
		} catch (err) {
			this.error = err instanceof Error ? err.message : String(err);
			console.error("Failed to import CSV:", err);
			throw err;
		} finally {
			this.isLoading = false;
		}
	}

	/**
	 * Select a row for viewing files
	 */
	selectRow(rowId: number | null): void {
		this.selectedRowId = rowId;
	}

	/**
	 * Reset the store to initial state
	 */
	reset(): void {
		this.currentFilePath = null;
		this.columns = [];
		this.totalRows = 0;
		this.rows = [];
		this.isFileOpen = false;
		this.error = null;
		this.currentOffset = 0;
		this.currentLimit = 100;
		this.selectedRowId = null;
		this.isLoadingMore = false;
		this.hasMoreRows = true;
	}
}

// Export a singleton instance
export const qrateStore = new QrateStore();
