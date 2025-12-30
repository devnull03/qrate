import { invoke } from "@tauri-apps/api/core";
import { qrateStore } from "./qrateStore.svelte";
import type { ProjectSettings } from "$lib/models/settings";
import { getProjectDefaults } from "$lib/services/settings";
import { getGlobalSettings } from "./globalSettings";

export type { ProjectSettings } from "$lib/models/settings";

export const defaultProjectSettings: ProjectSettings = {
	filesFolder: "",
	filePathPattern: "{files_folder}/{file_column}",
	fileColumnName: "file",
	defaultRowLimit: "10000",
};

export type AppSettings = ProjectSettings;
export const defaultSettings = defaultProjectSettings;

let currentSettings: ProjectSettings = { ...defaultProjectSettings };
let subscribers = new Set<(settings: ProjectSettings) => void>();

function notifySubscribers() {
	subscribers.forEach((callback) => callback(currentSettings));
}

export async function getDefaultProjectSettings(): Promise<ProjectSettings> {
	const global = getGlobalSettings();
	const pDefaults = await getProjectDefaults();
	return {
		filesFolder: (pDefaults.filesFolder as string) ?? "",
		filePathPattern:
			(pDefaults.filePathPattern as string) ??
			(global.defaultFilePathPattern as string),
		fileColumnName:
			(pDefaults.fileColumnName as string) ??
			(global.defaultFileColumnName as string),
		defaultRowLimit:
			(pDefaults.defaultRowLimit as string) ??
			String(global.defaultRowLimit),
	};
}

export async function loadSettings(): Promise<ProjectSettings> {
	const path = qrateStore.currentFilePath;
	const defaults = await getDefaultProjectSettings();

	if (!path) {
		currentSettings = { ...defaults };
		notifySubscribers();
		return currentSettings;
	}

	const settings = await invoke<Record<string, string>>(
		"get_project_settings",
		{ path },
	).catch(() => ({}));
	currentSettings = { ...defaults, ...settings };
	notifySubscribers();
	return currentSettings;
}

export async function saveSetting(key: string, value: string): Promise<void> {
	const path = qrateStore.currentFilePath;
	if (!path) return;

	await invoke("set_project_setting", { path, key, value });
	currentSettings[key] = value;
	notifySubscribers();
}

export async function saveSettings(
	settings: Partial<ProjectSettings>,
): Promise<void> {
	const path = qrateStore.currentFilePath;
	if (!path) return;

	const settingsMap: Record<string, string> = {};
	for (const [key, value] of Object.entries(settings)) {
		if (value !== undefined) {
			settingsMap[key] = String(value);
		}
	}

	await invoke("set_project_settings", { path, settings: settingsMap });
	currentSettings = { ...currentSettings, ...settingsMap };
	notifySubscribers();
}

export function getSettings(): ProjectSettings {
	return { ...currentSettings };
}

export function subscribeToSettings(
	callback: (settings: ProjectSettings) => void,
): () => void {
	subscribers.add(callback);
	callback(currentSettings);
	return () => subscribers.delete(callback);
}

export function resolveFilePath(
	pattern: string,
	filesFolder: string,
	rowData: Record<string, unknown>,
	fileColumnName: string,
): string {
	let resolvedPath = pattern
		.replace(/{files_folder}/g, filesFolder)
		.replace(/{file_column}/g, String(rowData[fileColumnName] || ""));

	resolvedPath = resolvedPath.replace(/\{([^}]+)\}/g, (match, columnName) => {
		if (columnName === "files_folder" || columnName === "file_column")
			return match;

		const entry = Object.entries(rowData).find(
			([key]) =>
				key === columnName ||
				key.toLowerCase() === columnName.toLowerCase(),
		);
		return entry ? String(entry[1] || "") : match;
	});

	return resolvedPath.replace(/\\/g, "/");
}
