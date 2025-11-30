import { load } from "@tauri-apps/plugin-store";
import type { Store } from "@tauri-apps/plugin-store";

export interface RecentFile {
	path: string;
	name: string;
	lastOpened: number; // timestamp
}

const MAX_RECENT_FILES = 10;

// Store instance (lazy loaded)
let storeInstance: Store | null = null;
let storePromise: Promise<Store> | null = null;

// In-memory cache for synchronous access
let cachedFiles: RecentFile[] = [];
let subscribers: Set<(files: RecentFile[]) => void> = new Set();

/**
 * Get the store instance (creates it if needed)
 */
async function getStore(): Promise<Store> {
	if (storeInstance) return storeInstance;

	if (!storePromise) {
		storePromise = load("recent-files.json");
	}

	storeInstance = await storePromise;
	return storeInstance;
}

/**
 * Notify all subscribers of changes
 */
function notifySubscribers() {
	console.log(
		"[recentFiles] Notifying subscribers, count:",
		subscribers.size,
	);
	subscribers.forEach((callback) => callback([...cachedFiles]));
}

/**
 * Initialize and load recent files
 * Call this early in the app lifecycle
 */
export async function initRecentFiles(): Promise<RecentFile[]> {
	console.log("[recentFiles] Initializing...");

	try {
		const store = await getStore();
		const files = await store.get<RecentFile[]>("files");

		if (Array.isArray(files)) {
			cachedFiles = files;
		}

		console.log("[recentFiles] Loaded files:", cachedFiles.length);
		notifySubscribers();
		return cachedFiles;
	} catch (err) {
		console.error("[recentFiles] Failed to initialize:", err);
		return cachedFiles;
	}
}

/**
 * Get recent files (synchronous, returns cached value)
 */
export function getRecentFiles(): RecentFile[] {
	return [...cachedFiles];
}

/**
 * Add a file to the recent files list
 */
export async function addRecentFile(path: string, name: string): Promise<void> {
	console.log("[recentFiles] Adding file:", path);

	try {
		const store = await getStore();

		// Remove if already exists
		const filtered = cachedFiles.filter((f) => f.path !== path);

		// Add to the beginning
		const newFiles = [
			{ path, name, lastOpened: Date.now() },
			...filtered,
		].slice(0, MAX_RECENT_FILES);

		await store.set("files", newFiles);
		cachedFiles = newFiles;

		console.log("[recentFiles] File added, total:", cachedFiles.length);
		notifySubscribers();
	} catch (err) {
		console.error("[recentFiles] Failed to add file:", err);
		throw err;
	}
}

/**
 * Remove a file from the recent files list
 */
export async function removeRecentFile(path: string): Promise<void> {
	console.log("[recentFiles] Removing file:", path);

	try {
		const store = await getStore();
		const newFiles = cachedFiles.filter((f) => f.path !== path);

		await store.set("files", newFiles);
		cachedFiles = newFiles;

		console.log("[recentFiles] File removed, total:", cachedFiles.length);
		notifySubscribers();
	} catch (err) {
		console.error("[recentFiles] Failed to remove file:", err);
		throw err;
	}
}

/**
 * Clear all recent files
 */
export async function clearRecentFiles(): Promise<void> {
	console.log("[recentFiles] Clearing all files");

	try {
		const store = await getStore();
		await store.set("files", []);
		cachedFiles = [];

		notifySubscribers();
	} catch (err) {
		console.error("[recentFiles] Failed to clear files:", err);
		throw err;
	}
}

/**
 * Subscribe to recent files changes
 * Returns an unsubscribe function
 */
export function subscribeToRecentFiles(
	callback: (files: RecentFile[]) => void,
): () => void {
	console.log("[recentFiles] New subscriber added");
	subscribers.add(callback);

	// Immediately call with current value
	callback([...cachedFiles]);

	return () => {
		console.log("[recentFiles] Subscriber removed");
		subscribers.delete(callback);
	};
}

/**
 * Reactive store interface for Svelte compatibility
 */
export const recentFilesStore = {
	subscribe: subscribeToRecentFiles,
	get: getRecentFiles,
};
