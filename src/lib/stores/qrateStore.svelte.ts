import { invoke } from "@tauri-apps/api/core";
import { load } from "@tauri-apps/plugin-store";
import type { Store } from "@tauri-apps/plugin-store";
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

interface PersistedWorkspace {
	currentFilePath: string | null;
}

let workspaceStore: Store | null = null;
let workspaceStorePromise: Promise<Store> | null = null;

async function getWorkspaceStore(): Promise<Store> {
	if (workspaceStore) return workspaceStore;
	workspaceStorePromise ??= load("workspace.json");
	workspaceStore = await workspaceStorePromise;
	return workspaceStore;
}

class QrateStore {
	currentFilePath = $state<string | null>(null);
	columns = $state<ColumnDef[]>([]);
	totalRows = $state<number>(0);
	rows = $state<Record<string, any>[]>([]);
	isLoading = $state<boolean>(false);
	isFileOpen = $state<boolean>(false);
	error = $state<string | null>(null);
	currentOffset = $state<number>(0);
	currentLimit = $state<number>(100);
	scrollPosition = $state<number>(0);
	selectedRowId = $state<number | null>(null);
	selectedColumnId = $state<string | null>(null);
	selectedRange = $state<{
		startRow: number;
		endRow: number;
		startCol: number;
		endCol: number;
	} | null>(null);
	isLoadingMore = $state<boolean>(false);
	hasMoreRows = $state<boolean>(true);
	activeView = $state<"spreadsheet" | "files">("spreadsheet");
	filesGridFilteredCount = $state<number>(0);
	filesGridTotalCount = $state<number>(0);
	filesGridSearchQuery = $state<string>("");

	private restorationAttempted = false;

	private async persistWorkspace(): Promise<void> {
		const store = await getWorkspaceStore();
		await store.set("currentFilePath", this.currentFilePath);
	}

	private async loadPersistedWorkspace(): Promise<PersistedWorkspace | null> {
		const store = await getWorkspaceStore();
		const filePath = await store.get<string>("currentFilePath");
		if (!filePath) return null;

		return {
			currentFilePath: filePath,
		};
	}

	private async clearPersistedWorkspace(): Promise<void> {
		const store = await getWorkspaceStore();
		await store.set("currentFilePath", null);
	}

	async syncFromBackend(): Promise<boolean> {
		try {
			this.isLoading = true;
			this.error = null;

			const response =
				await invoke<CurrentStateResponse>("get_current_state");

			if (response.is_file_open && response.path) {
				this.currentFilePath = response.path;
				this.columns = response.columns;
				this.totalRows = response.total_rows;
				this.isFileOpen = true;

				// Always reload from row 0 to avoid stale offset
				if (response.offset > 0) {
					await this.loadRows(0, 100);
				} else {
					this.rows = response.rows;
					this.currentOffset = response.offset;
					this.currentLimit = response.limit;
				}
				return true;
			}

			this.reset();
			return false;
		} catch (err) {
			this.error = err instanceof Error ? err.message : String(err);
			return false;
		} finally {
			this.isLoading = false;
		}
	}

	async restoreWorkspace(force: boolean = false): Promise<boolean> {
		if (this.restorationAttempted && !force) return false;
		if (!force) this.restorationAttempted = true;

		const syncedFromBackend = await this.syncFromBackend();
		if (syncedFromBackend) return true;

		try {
			const workspace = await this.loadPersistedWorkspace();

			if (workspace?.currentFilePath) {
				await this.openFile(workspace.currentFilePath);
				// Always start from row 0 - don't restore offset
				return true;
			}
		} catch {
			await this.clearPersistedWorkspace();
		}

		return false;
	}

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

			await this.persistWorkspace();
			await addRecentFile(path, getFileName(path));
		} catch (err) {
			this.error = err instanceof Error ? err.message : String(err);
			throw err;
		} finally {
			this.isLoading = false;
		}
	}

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

			if (this.totalRows > 0) {
				await this.loadRows(0, 100);
				this.hasMoreRows = this.totalRows > this.rows.length;
			} else {
				this.rows = [];
				this.hasMoreRows = false;
			}

			await this.persistWorkspace();
			await addRecentFile(path, getFileName(path));
		} catch (err) {
			this.error = err instanceof Error ? err.message : String(err);
			throw err;
		} finally {
			this.isLoading = false;
		}
	}

	async closeFile(): Promise<void> {
		if (!this.currentFilePath) return;

		try {
			await invoke("close_qrate_file", { path: this.currentFilePath });
			this.reset();
			await this.clearPersistedWorkspace();
		} catch (err) {
			this.error = err instanceof Error ? err.message : String(err);
			throw err;
		}
	}

	async loadRows(offset: number, limit: number): Promise<void> {
		if (!this.currentFilePath) throw new Error("No file is currently open");

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

			await this.persistWorkspace();
		} catch (err) {
			this.error = err instanceof Error ? err.message : String(err);
			throw err;
		}
	}

	async loadMoreRows(count: number = 100): Promise<void> {
		if (!this.currentFilePath || this.isLoadingMore || !this.hasMoreRows)
			return;

		try {
			this.isLoadingMore = true;
			const nextOffset = this.rows.length;

			const response = await invoke<DataResponse>("get_rows", {
				path: this.currentFilePath,
				limit: count,
				offset: nextOffset,
			});

			this.rows = [...this.rows, ...response.rows];
			this.totalRows = response.total;
			this.hasMoreRows = this.rows.length < response.total;

			await this.persistWorkspace();
		} catch (err) {
			this.error = err instanceof Error ? err.message : String(err);
		} finally {
			this.isLoadingMore = false;
		}
	}

	async updateCell(
		rowId: number,
		columnId: string,
		value: string,
	): Promise<void> {
		if (!this.currentFilePath) throw new Error("No file is currently open");

		try {
			await invoke("update_cell", {
				path: this.currentFilePath,
				rowId,
				columnId,
				value,
			});

			const rowIndex = this.rows.findIndex((row) => row.row_id === rowId);
			if (rowIndex !== -1) {
				this.rows[rowIndex][columnId] = value;
			}
		} catch (err) {
			this.error = err instanceof Error ? err.message : String(err);
			throw err;
		}
	}

	async addColumn(column: ColumnDef): Promise<void> {
		if (!this.currentFilePath) throw new Error("No file is currently open");

		try {
			await invoke("add_column", { path: this.currentFilePath, column });
			this.columns = [...this.columns, column];
			await this.loadRows(this.currentOffset, this.currentLimit);
		} catch (err) {
			this.error = err instanceof Error ? err.message : String(err);
			throw err;
		}
	}

	async updateColumn(column: ColumnDef): Promise<void> {
		if (!this.currentFilePath) throw new Error("No file is currently open");

		try {
			await invoke("update_column", {
				path: this.currentFilePath,
				column,
			});
			const colIndex = this.columns.findIndex((c) => c.id === column.id);
			if (colIndex !== -1) {
				this.columns[colIndex] = column;
			}
		} catch (err) {
			this.error = err instanceof Error ? err.message : String(err);
			throw err;
		}
	}

	async insertRow(values: Record<string, any>): Promise<number> {
		if (!this.currentFilePath) throw new Error("No file is currently open");

		try {
			const rowId = await invoke<number>("insert_row", {
				path: this.currentFilePath,
				values,
			});

			this.totalRows += 1;
			await this.loadRows(this.currentOffset, this.currentLimit);

			return rowId;
		} catch (err) {
			this.error = err instanceof Error ? err.message : String(err);
			throw err;
		}
	}

	async deleteRow(rowId: number): Promise<void> {
		if (!this.currentFilePath) throw new Error("No file is currently open");

		try {
			await invoke("delete_row", { path: this.currentFilePath, rowId });
			this.totalRows -= 1;
			this.rows = this.rows.filter((row) => row.row_id !== rowId);
		} catch (err) {
			this.error = err instanceof Error ? err.message : String(err);
			throw err;
		}
	}

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

			if (this.totalRows > 0) {
				await this.loadRows(0, 100);
				this.hasMoreRows = this.totalRows > this.rows.length;
			} else {
				this.rows = [];
				this.hasMoreRows = false;
			}

			await this.persistWorkspace();
			await addRecentFile(qratePath, getFileName(qratePath));
		} catch (err) {
			this.error = err instanceof Error ? err.message : String(err);
			throw err;
		} finally {
			this.isLoading = false;
		}
	}

	selectRow(rowId: number | null): void {
		this.selectedRowId = rowId;
	}

	selectColumn(columnId: string | null): void {
		this.selectedColumnId = columnId;
	}

	selectRange(
		range: {
			startRow: number;
			endRow: number;
			startCol: number;
			endCol: number;
		} | null,
	): void {
		this.selectedRange = range;
	}

	clearSelection(): void {
		this.selectedRowId = null;
		this.selectedColumnId = null;
		this.selectedRange = null;
	}

	reset(): void {
		this.currentFilePath = null;
		this.columns = [];
		this.totalRows = 0;
		this.rows = [];
		this.isFileOpen = false;
		this.error = null;
		this.currentOffset = 0;
		this.currentLimit = 100;
		this.scrollPosition = 0;
		this.selectedRowId = null;
		this.selectedColumnId = null;
		this.selectedRange = null;
		this.isLoadingMore = false;
		this.hasMoreRows = true;
		this.filesGridFilteredCount = 0;
		this.filesGridTotalCount = 0;
		this.filesGridSearchQuery = "";
	}
}

export const qrateStore = new QrateStore();
