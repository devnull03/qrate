import { invoke } from '@tauri-apps/api/core';

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
	currentLimit = $state<number>(100);

	/**
	 * Create a new .qrate file
	 */
	async createFile(path: string): Promise<void> {
		try {
			this.isLoading = true;
			this.error = null;

			const response = await invoke<FileOpenResponse>('create_qrate_file', { path });

			this.currentFilePath = response.path;
			this.columns = response.columns;
			this.totalRows = response.total_rows;
			this.rows = [];
			this.isFileOpen = true;
		} catch (err) {
			this.error = err instanceof Error ? err.message : String(err);
			console.error('Failed to create file:', err);
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

			const response = await invoke<FileOpenResponse>('open_qrate_file', { path });

			this.currentFilePath = response.path;
			this.columns = response.columns;
			this.totalRows = response.total_rows;
			this.isFileOpen = true;

			// Load initial viewport data
			if (this.totalRows > 0) {
				await this.loadRows(0, this.currentLimit);
			} else {
				this.rows = [];
			}
		} catch (err) {
			this.error = err instanceof Error ? err.message : String(err);
			console.error('Failed to open file:', err);
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
			await invoke('close_qrate_file', { path: this.currentFilePath });
			this.reset();
		} catch (err) {
			this.error = err instanceof Error ? err.message : String(err);
			console.error('Failed to close file:', err);
			throw err;
		}
	}

	/**
	 * Load a specific range of rows (virtual scrolling)
	 */
	async loadRows(offset: number, limit: number): Promise<void> {
		if (!this.currentFilePath) {
			throw new Error('No file is currently open');
		}

		try {
			const response = await invoke<DataResponse>('get_rows', {
				path: this.currentFilePath,
				limit,
				offset,
			});

			this.rows = response.rows;
			this.currentOffset = offset;
			this.currentLimit = limit;
			this.totalRows = response.total;
		} catch (err) {
			this.error = err instanceof Error ? err.message : String(err);
			console.error('Failed to load rows:', err);
			throw err;
		}
	}

	/**
	 * Update a single cell value
	 */
	async updateCell(rowId: number, columnId: string, value: string): Promise<void> {
		if (!this.currentFilePath) {
			throw new Error('No file is currently open');
		}

		try {
			await invoke('update_cell', {
				path: this.currentFilePath,
				rowId,
				columnId,
				value,
			});

			// Update local state
			const rowIndex = this.rows.findIndex(row => row.row_id === rowId);
			if (rowIndex !== -1) {
				this.rows[rowIndex][columnId] = value;
			}
		} catch (err) {
			this.error = err instanceof Error ? err.message : String(err);
			console.error('Failed to update cell:', err);
			throw err;
		}
	}

	/**
	 * Add a new column
	 */
	async addColumn(column: ColumnDef): Promise<void> {
		if (!this.currentFilePath) {
			throw new Error('No file is currently open');
		}

		try {
			await invoke('add_column', {
				path: this.currentFilePath,
				column,
			});

			// Update local state
			this.columns = [...this.columns, column];

			// Reload current viewport to get new column data
			await this.loadRows(this.currentOffset, this.currentLimit);
		} catch (err) {
			this.error = err instanceof Error ? err.message : String(err);
			console.error('Failed to add column:', err);
			throw err;
		}
	}

	/**
	 * Update column metadata (width, hidden, etc.)
	 */
	async updateColumn(column: ColumnDef): Promise<void> {
		if (!this.currentFilePath) {
			throw new Error('No file is currently open');
		}

		try {
			await invoke('update_column', {
				path: this.currentFilePath,
				column,
			});

			// Update local state
			const colIndex = this.columns.findIndex(c => c.id === column.id);
			if (colIndex !== -1) {
				this.columns[colIndex] = column;
			}
		} catch (err) {
			this.error = err instanceof Error ? err.message : String(err);
			console.error('Failed to update column:', err);
			throw err;
		}
	}

	/**
	 * Insert a new row
	 */
	async insertRow(values: Record<string, any>): Promise<number> {
		if (!this.currentFilePath) {
			throw new Error('No file is currently open');
		}

		try {
			const rowId = await invoke<number>('insert_row', {
				path: this.currentFilePath,
				values,
			});

			this.totalRows += 1;

			// Reload current viewport to show new row if in range
			await this.loadRows(this.currentOffset, this.currentLimit);

			return rowId;
		} catch (err) {
			this.error = err instanceof Error ? err.message : String(err);
			console.error('Failed to insert row:', err);
			throw err;
		}
	}

	/**
	 * Delete a row
	 */
	async deleteRow(rowId: number): Promise<void> {
		if (!this.currentFilePath) {
			throw new Error('No file is currently open');
		}

		try {
			await invoke('delete_row', {
				path: this.currentFilePath,
				rowId,
			});

			this.totalRows -= 1;

			// Remove from local state
			this.rows = this.rows.filter(row => row.row_id !== rowId);
		} catch (err) {
			this.error = err instanceof Error ? err.message : String(err);
			console.error('Failed to delete row:', err);
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

			const response = await invoke<FileOpenResponse>('import_csv_to_qrate', {
				qratePath,
				csvPath,
			});

			this.currentFilePath = response.path;
			this.columns = response.columns;
			this.totalRows = response.total_rows;
			this.isFileOpen = true;

			// Load initial viewport data
			if (this.totalRows > 0) {
				await this.loadRows(0, this.currentLimit);
			} else {
				this.rows = [];
			}
		} catch (err) {
			this.error = err instanceof Error ? err.message : String(err);
			console.error('Failed to import CSV:', err);
			throw err;
		} finally {
			this.isLoading = false;
		}
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
	}
}

// Export a singleton instance
export const qrateStore = new QrateStore();
