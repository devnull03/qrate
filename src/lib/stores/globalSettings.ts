import { load } from "@tauri-apps/plugin-store";
import type { Store } from "@tauri-apps/plugin-store";
import {
	type GlobalSettings,
	getGlobalDefaults,
	getGlobalSettingDef,
} from "$lib/settings";

// Re-export type for convenience
export type { GlobalSettings } from "$lib/settings";

// Store instance (lazy loaded)
let storeInstance: Store | null = null;
let storePromise: Promise<Store> | null = null;

// In-memory cache for synchronous access
// Provide a minimal fallback until runtime defaults are fetched
const fallbackDefaults: GlobalSettings = {
	// Appearance
	theme: "system",

	// Default values for new projects
	defaultRowLimit: 100,
	defaultFilePathPattern: "{files_folder}/{file_column}",
	defaultFileColumnName: "file",

	// UI preferences
	sidebarWidth: 280,
	sidebarCollapsed: false,
	showStatusBar: true,

	// Behavior
	autoSaveInterval: 0,
	confirmBeforeDelete: true,
	reopenLastProject: true,

	// Grid/Table
	gridRowHeight: 32,
	gridAlternateRowColors: true,
};

let cachedSettings: GlobalSettings = { ...fallbackDefaults };
let subscribers: Set<(settings: GlobalSettings) => void> = new Set();

/**
 * Get the store instance (creates it if needed)
 */
async function getStore(): Promise<Store> {
	if (storeInstance) return storeInstance;

	if (!storePromise) {
		storePromise = load("global-settings.json");
	}

	storeInstance = await storePromise;
	return storeInstance;
}

/**
 * Notify all subscribers of settings changes
 */
function notifySubscribers() {
	console.log(
		"[globalSettings] Notifying subscribers, count:",
		subscribers.size,
	);
	subscribers.forEach((callback) => callback(cachedSettings));
}

/**
 * Initialize and load global settings
 * Call this early in the app lifecycle
 */
export async function initGlobalSettings(): Promise<GlobalSettings> {
	console.log("[globalSettings] Initializing...");

	try {
		const store = await getStore();
		const storeAny = store as any;

		// Merge runtime defaults (strings from backend) into typed cached settings
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
			} else {
				parsedValue = rawValue;
			}

			(cachedSettings as unknown as Record<string, unknown>)[key] =
				parsedValue;
		}

		// Load each setting from the local store, falling back to runtime defaults
		for (const key of Object.keys(cachedSettings) as Array<
			keyof GlobalSettings
		>) {
			const value = await storeAny.get(key as string);
			if (value !== null && value !== undefined) {
				(cachedSettings as unknown as Record<string, unknown>)[key] =
					value;
			}
		}

		console.log("[globalSettings] Loaded settings:", cachedSettings);
		notifySubscribers();
		return cachedSettings;
	} catch (err) {
		console.error("[globalSettings] Failed to initialize:", err);
		return cachedSettings;
	}
}

/**
 * Get current global settings (synchronous, returns cached value)
 */
export function getGlobalSettings(): GlobalSettings {
	return { ...cachedSettings };
}

/**
 * Get a single global setting
 */
export function getGlobalSetting<K extends keyof GlobalSettings>(
	key: K,
): GlobalSettings[K] {
	return cachedSettings[key];
}

/**
 * Set a single global setting
 */
export async function setGlobalSetting<K extends keyof GlobalSettings>(
	key: K,
	value: GlobalSettings[K],
): Promise<void> {
	console.log("[globalSettings] Setting", key, "to", value);

	try {
		const store = await getStore();
		const storeAny = store as any;
		await storeAny.set(key as string, value);
		await storeAny.save();
		cachedSettings[key] = value;
		notifySubscribers();
	} catch (err) {
		console.error("[globalSettings] Failed to set setting:", err);
		throw err;
	}
}

/**
 * Set multiple global settings at once
 */
export async function setGlobalSettings(
	settings: Partial<GlobalSettings>,
): Promise<void> {
	console.log("[globalSettings] Setting multiple:", settings);

	try {
		const store = await getStore();
		const storeAny = store as any;

		for (const [key, value] of Object.entries(settings)) {
			if (value !== undefined) {
				await storeAny.set(key as string, value);
				(cachedSettings as unknown as Record<string, unknown>)[key] =
					value;
			}
		}

		await storeAny.save();
		notifySubscribers();
	} catch (err) {
		console.error("[globalSettings] Failed to set settings:", err);
		throw err;
	}
}

/**
 * Reset all global settings to defaults
 */
export async function resetGlobalSettings(): Promise<void> {
	console.log("[globalSettings] Resetting to defaults");

	try {
		const store = await getStore();
		const storeAny = store as any;

		await storeAny.clear();

		// Fetch runtime defaults from the backend and store them locally
		const runtimeDefaults = await getGlobalDefaults();
		for (const [key, rawValue] of Object.entries(runtimeDefaults)) {
			const def = await getGlobalSettingDef(key);
			let parsedValue: unknown = rawValue;
			if (def?.setting_type === "number") {
				const num = Number(rawValue);
				parsedValue = Number.isNaN(num) ? 0 : num;
			} else if (def?.setting_type === "boolean") {
				parsedValue = rawValue === "true" || rawValue === "1";
			} else {
				parsedValue = rawValue;
			}

			await storeAny.set(key as string, parsedValue);
			(cachedSettings as unknown as Record<string, unknown>)[key] =
				parsedValue;
		}

		await storeAny.save();
		notifySubscribers();
	} catch (err) {
		console.error("[globalSettings] Failed to reset settings:", err);
		throw err;
	}
}

/**
 * Subscribe to global settings changes
 * Returns an unsubscribe function
 */
export function subscribeToGlobalSettings(
	callback: (settings: GlobalSettings) => void,
): () => void {
	console.log("[globalSettings] New subscriber added");
	subscribers.add(callback);

	// Immediately call with current value
	callback(cachedSettings);

	return () => {
		console.log("[globalSettings] Subscriber removed");
		subscribers.delete(callback);
	};
}
