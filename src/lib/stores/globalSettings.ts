import { load } from "@tauri-apps/plugin-store";
import type { Store } from "@tauri-apps/plugin-store";
import type { GlobalSettings } from "$lib/models/settings";
import { getGlobalDefaults, getGlobalSettingDef } from "$lib/services/settings";

export type { GlobalSettings } from "$lib/models/settings";

let storeInstance: Store | null = null;
let storePromise: Promise<Store> | null = null;

const fallbackDefaults: GlobalSettings = {
	theme: "system",
	defaultRowLimit: 100,
	defaultFilePathPattern: "{files_folder}/{file_column}",
	defaultFileColumnName: "file",
	sidebarWidth: 280,
	sidebarCollapsed: false,
	showStatusBar: true,
	autoSaveInterval: 0,
	confirmBeforeDelete: true,
	reopenLastProject: true,
	gridRowHeight: 32,
	gridAlternateRowColors: true,
	splitDirection: "right",
	splitSize: 65,
};

let cachedSettings: GlobalSettings = { ...fallbackDefaults };
let subscribers = new Set<(settings: GlobalSettings) => void>();

async function getStore(): Promise<Store> {
	if (storeInstance) return storeInstance;
	storePromise ??= load("global-settings.json");
	storeInstance = await storePromise;
	return storeInstance;
}

function notifySubscribers() {
	subscribers.forEach((callback) => callback(cachedSettings));
}

export async function initGlobalSettings(): Promise<GlobalSettings> {
	const store = await getStore();
	const storeAny = store as any;

	const runtimeDefaults = await getGlobalDefaults();
	for (const [key, rawValue] of Object.entries(runtimeDefaults)) {
		const def = await getGlobalSettingDef(key);
		if (!def) continue;

		let parsedValue: unknown = rawValue;
		if (def.setting_type === "number") {
			const num = Number(rawValue);
			parsedValue = Number.isNaN(num) ? 0 : num;
		} else if (def.setting_type === "boolean") {
			parsedValue = rawValue === "true" || rawValue === "1";
		}

		(cachedSettings as Record<string, unknown>)[key] = parsedValue;
	}

	for (const key of Object.keys(cachedSettings)) {
		const value = await storeAny.get(key);
		if (value !== null && value !== undefined) {
			(cachedSettings as Record<string, unknown>)[key] = value;
		}
	}

	notifySubscribers();
	return cachedSettings;
}

export function getGlobalSettings(): GlobalSettings {
	return { ...cachedSettings };
}

export function getGlobalSetting<K extends keyof GlobalSettings>(
	key: K,
): GlobalSettings[K] {
	return cachedSettings[key];
}

export async function setGlobalSetting<K extends keyof GlobalSettings>(
	key: K,
	value: GlobalSettings[K],
): Promise<void> {
	const store = await getStore();
	const storeAny = store as any;
	await storeAny.set(key as string, value);
	await storeAny.save();
	cachedSettings[key] = value;
	notifySubscribers();

	// Broadcast settings change to all windows
	if (typeof window !== "undefined") {
		const { emit } = await import("@tauri-apps/api/event");
		await emit("settings:updated", cachedSettings);

		// Special handling for theme changes
		if (key === "theme") {
			await emit("theme:changed", { theme: value });
		}
	}
}

export async function setGlobalSettings(
	settings: Partial<GlobalSettings>,
): Promise<void> {
	const store = await getStore();
	const storeAny = store as any;

	for (const [key, value] of Object.entries(settings)) {
		if (value !== undefined) {
			await storeAny.set(key, value);
			(cachedSettings as Record<string, unknown>)[key] = value;
		}
	}

	await storeAny.save();
	notifySubscribers();

	// Broadcast settings change to all windows
	if (typeof window !== "undefined") {
		const { emit } = await import("@tauri-apps/api/event");
		await emit("settings:updated", cachedSettings);

		// Special handling for theme changes
		if (settings.theme !== undefined) {
			await emit("theme:changed", { theme: settings.theme });
		}
	}
}

export async function resetGlobalSettings(): Promise<void> {
	const store = await getStore();
	const storeAny = store as any;
	await storeAny.clear();

	const runtimeDefaults = await getGlobalDefaults();
	for (const [key, rawValue] of Object.entries(runtimeDefaults)) {
		const def = await getGlobalSettingDef(key);
		let parsedValue: unknown = rawValue;
		if (def?.setting_type === "number") {
			const num = Number(rawValue);
			parsedValue = Number.isNaN(num) ? 0 : num;
		} else if (def?.setting_type === "boolean") {
			parsedValue = rawValue === "true" || rawValue === "1";
		}

		await storeAny.set(key, parsedValue);
		(cachedSettings as Record<string, unknown>)[key] = parsedValue;
	}

	await storeAny.save();
	notifySubscribers();
}

export function subscribeToGlobalSettings(
	callback: (settings: GlobalSettings) => void,
): () => void {
	subscribers.add(callback);
	callback(cachedSettings);
	return () => subscribers.delete(callback);
}
