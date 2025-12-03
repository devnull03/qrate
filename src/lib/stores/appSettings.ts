import { invoke } from "@tauri-apps/api/core";
import { qrateStore } from "./qrateStore.svelte";
import { type ProjectSettings } from "$lib/settings";
import { getProjectDefaults } from "$lib/settings";
import { getGlobalSettings } from "./globalSettings";

// Re-export types for convenience
export type { ProjectSettings } from "$lib/settings";
export const defaultProjectSettings: ProjectSettings = {
	filesFolder: "",
	filePathPattern: "{files_folder}/{file_column}",
	fileColumnName: "file",
	defaultRowLimit: "100",
};

// Legacy alias for backward compatibility
export type AppSettings = ProjectSettings;
export const defaultSettings = defaultProjectSettings;

// Current settings state (reactive)
let currentSettings: ProjectSettings = { ...defaultProjectSettings };
let subscribers: Set<(settings: ProjectSettings) => void> = new Set();

function notifySubscribers() {
	console.log(
		"[appSettings] Notifying subscribers, count:",
		subscribers.size,
	);
	subscribers.forEach((callback) => callback(currentSettings));
}

/**
 * Get default project settings based on current global settings.
 * This uses global defaults for pattern and column name when creating new projects.
 */
export async function getDefaultProjectSettings(): Promise<ProjectSettings> {
	const global = getGlobalSettings();
	const pDefaults = await getProjectDefaults();
	return {
		filesFolder: (pDefaults.filesFolder as unknown as string) ?? "",
		filePathPattern:
			(pDefaults.filePathPattern as unknown as string) ??
			(global.defaultFilePathPattern as unknown as string),
		fileColumnName:
			(pDefaults.fileColumnName as unknown as string) ??
			(global.defaultFileColumnName as unknown as string),
		defaultRowLimit:
			(pDefaults.defaultRowLimit as unknown as string) ??
			String(global.defaultRowLimit),
	};
}

/**
 * Load project settings from the current .qrate file
 */
export async function loadSettings(): Promise<ProjectSettings> {
	const path = qrateStore.currentFilePath;
	console.log("[appSettings] loadSettings called, path:", path);

	// Get fresh defaults based on current global settings
	const defaults = await getDefaultProjectSettings();

	if (!path) {
		console.log("[appSettings] No path, returning defaults");
		currentSettings = { ...defaults };
		notifySubscribers();
		return currentSettings;
	}

	try {
		console.log(
			"[appSettings] Invoking get_project_settings for path:",
			path,
		);
		const settings = await invoke<Record<string, string>>(
			"get_project_settings",
			{
				path,
			},
		);
		console.log("[appSettings] Received settings from backend:", settings);
		currentSettings = {
			...defaults,
			...settings,
		};
		console.log("[appSettings] Merged settings:", currentSettings);
		notifySubscribers();
		return currentSettings;
	} catch (err) {
		console.error("[appSettings] Failed to load settings:", err);
		currentSettings = { ...defaults };
		notifySubscribers();
		return currentSettings;
	}
}

/**
 * Save a single project setting to the current .qrate file
 */
export async function saveSetting(key: string, value: string): Promise<void> {
	const path = qrateStore.currentFilePath;
	console.log(
		"[appSettings] saveSetting called, key:",
		key,
		"value:",
		value,
		"path:",
		path,
	);

	if (!path) {
		console.warn("[appSettings] No file open, cannot save setting");
		return;
	}

	try {
		console.log("[appSettings] Invoking set_project_setting");
		await invoke("set_project_setting", { path, key, value });
		currentSettings[key] = value;
		console.log("[appSettings] Setting saved successfully");
		notifySubscribers();
	} catch (err) {
		console.error("[appSettings] Failed to save setting:", err);
		throw err;
	}
}

/**
 * Save multiple project settings to the current .qrate file
 */
export async function saveSettings(
	settings: Partial<ProjectSettings>,
): Promise<void> {
	const path = qrateStore.currentFilePath;
	console.log(
		"[appSettings] saveSettings called, settings:",
		settings,
		"path:",
		path,
	);

	if (!path) {
		console.warn("[appSettings] No file open, cannot save settings");
		return;
	}

	try {
		const settingsMap: Record<string, string> = {};
		for (const [key, value] of Object.entries(settings)) {
			if (value !== undefined) {
				settingsMap[key] = String(value);
			}
		}
		console.log(
			"[appSettings] Invoking set_project_settings with map:",
			settingsMap,
		);

		await invoke("set_project_settings", { path, settings: settingsMap });
		currentSettings = { ...currentSettings, ...settingsMap };
		console.log("[appSettings] Settings saved successfully");
		notifySubscribers();
	} catch (err) {
		console.error("[appSettings] Failed to save settings:", err);
		throw err;
	}
}

/**
 * Get current project settings (synchronous, returns cached value)
 */
export function getSettings(): ProjectSettings {
	console.log(
		"[appSettings] getSettings called, returning:",
		currentSettings,
	);
	return { ...currentSettings };
}

/**
 * Subscribe to project settings changes
 */
export function subscribeToSettings(
	callback: (settings: ProjectSettings) => void,
): () => void {
	console.log("[appSettings] New subscriber added");
	subscribers.add(callback);
	// Immediately call with current value
	callback(currentSettings);

	// Return unsubscribe function
	return () => {
		console.log("[appSettings] Subscriber removed");
		subscribers.delete(callback);
	};
}

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
	rowData: Record<string, unknown>,
	fileColumnName: string,
): string {
	let resolvedPath = pattern;

	// Replace {files_folder} with the actual folder path
	resolvedPath = resolvedPath.replace(/{files_folder}/g, filesFolder);

	// Replace {file_column} with the value from the file column
	const fileValue = rowData[fileColumnName] || "";
	resolvedPath = resolvedPath.replace(/{file_column}/g, String(fileValue));

	// Replace any other {column_name} patterns with their values from row data
	const columnPattern = /\{([^}]+)\}/g;
	resolvedPath = resolvedPath.replace(columnPattern, (match, columnName) => {
		// Skip if already handled
		if (columnName === "files_folder" || columnName === "file_column") {
			return match;
		}
		// Look up the column value in row data
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
