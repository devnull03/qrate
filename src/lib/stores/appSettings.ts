import { Store } from "@tauri-store/svelte";

export interface AppSettings {
	theme: "light" | "dark" | "system";
	defaultRowLimit: number;
	autoSaveInterval: number; // in milliseconds
	showHiddenColumns: boolean;
	sidebarCollapsed: boolean;
	// File path pattern settings
	filePathPattern: string; // e.g., "{files_folder}/{some_column}/{file_column}"
	filesFolder: string; // Base folder containing all files
	fileColumnName: string; // Name of the column containing file names
	[key: string]: string | number | boolean;
}

const defaultSettings: AppSettings = {
	theme: "system",
	defaultRowLimit: 100,
	autoSaveInterval: 30000,
	showHiddenColumns: false,
	sidebarCollapsed: false,
	// Default file path pattern - just file in the chosen folder
	filePathPattern: "{files_folder}/{file_column}",
	filesFolder: "",
	fileColumnName: "file",
};

export const appSettingsStore = new Store<AppSettings>(
	"app-settings",
	defaultSettings,
	{
		autoStart: true,
	},
);

/**
 * Resolve a file path based on the pattern and row data
 * @param pattern The file path pattern (e.g., "{files_folder}/{some_column}/{file_column}")
 * @param filesFolder The base folder containing files
 * @param rowData The row data to extract column values from
 * @param fileColumnName The name of the column containing the file name
 * @returns The resolved file path
 */
export function resolveFilePath(
	pattern: string,
	filesFolder: string,
	rowData: Record<string, any>,
	fileColumnName: string,
): string {
	let resolvedPath = pattern;

	// Replace {files_folder} with the actual folder path
	resolvedPath = resolvedPath.replace("{files_folder}", filesFolder);

	// Replace {file_column} with the value from the file column
	const fileValue = rowData[fileColumnName] || "";
	resolvedPath = resolvedPath.replace("{file_column}", fileValue);

	// Replace any other {column_name} patterns with their values from row data
	const columnPattern = /\{([^}]+)\}/g;
	resolvedPath = resolvedPath.replace(columnPattern, (match, columnName) => {
		// Skip if already handled
		if (columnName === "files_folder" || columnName === "file_column") {
			return match;
		}
		// Look up the column value in row data
		// We need to find the column by name, not id
		for (const [key, value] of Object.entries(rowData)) {
			if (
				key === columnName ||
				key.toLowerCase() === columnName.toLowerCase()
			) {
				return String(value || "");
			}
		}
		return match; // Keep original if not found
	});

	// Normalize path separators
	resolvedPath = resolvedPath.replace(/\\/g, "/");

	return resolvedPath;
}
