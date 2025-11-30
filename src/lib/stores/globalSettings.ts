import { load } from "@tauri-apps/plugin-store";
import type { Store } from "@tauri-apps/plugin-store";
import {
	type GlobalSettings,
	defaultGlobalSettings,
} from "$lib/settings/schema";

// Re-export types for convenience
export type { GlobalSettings } from "$lib/settings/schema";
export { defaultGlobalSettings } from "$lib/settings/schema";

// Store instance (lazy loaded)
let storeInstance: Store | null = null;
let storePromise: Promise<Store> | null = null;

// In-memory cache for synchronous access
let cachedSettings: GlobalSettings = { ...defaultGlobalSettings };
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

		// Load each setting, falling back to defaults
		for (const key of Object.keys(defaultGlobalSettings) as Array<
			keyof GlobalSettings
		>) {
			const value = await store.get<GlobalSettings[typeof key]>(key);
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
		await store.set(key, value);
		await store.save();
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

		for (const [key, value] of Object.entries(settings)) {
			if (value !== undefined) {
				await store.set(key, value);
				(cachedSettings as unknown as Record<string, unknown>)[key] =
					value;
			}
		}

		await store.save();
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
		await store.clear();

		// Set all defaults
		for (const [key, value] of Object.entries(defaultGlobalSettings)) {
			await store.set(key, value);
		}

		await store.save();
		cachedSettings = { ...defaultGlobalSettings };
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
