import { load } from "@tauri-apps/plugin-store";
import type { Store } from "@tauri-apps/plugin-store";

export interface RecentFile {
	path: string;
	name: string;
	lastOpened: number;
}

const MAX_RECENT_FILES = 10;

let storeInstance: Store | null = null;
let storePromise: Promise<Store> | null = null;
let cachedFiles: RecentFile[] = [];
let subscribers = new Set<(files: RecentFile[]) => void>();

async function getStore(): Promise<Store> {
	if (storeInstance) return storeInstance;
	storePromise ??= load("recent-files.json");
	storeInstance = await storePromise;
	return storeInstance;
}

function notifySubscribers() {
	subscribers.forEach((callback) => callback([...cachedFiles]));
}

export async function initRecentFiles(): Promise<RecentFile[]> {
	const store = await getStore();
	const files = await store.get<RecentFile[]>("files");
	if (Array.isArray(files)) {
		cachedFiles = files;
	}
	notifySubscribers();
	return cachedFiles;
}

export function getRecentFiles(): RecentFile[] {
	return [...cachedFiles];
}

export async function addRecentFile(path: string, name: string): Promise<void> {
	const store = await getStore();
	const newFiles = [
		{ path, name, lastOpened: Date.now() },
		...cachedFiles.filter((f) => f.path !== path),
	].slice(0, MAX_RECENT_FILES);

	await store.set("files", newFiles);
	cachedFiles = newFiles;
	notifySubscribers();
}

export async function removeRecentFile(path: string): Promise<void> {
	const store = await getStore();
	const newFiles = cachedFiles.filter((f) => f.path !== path);
	await store.set("files", newFiles);
	cachedFiles = newFiles;
	notifySubscribers();
}

export async function clearRecentFiles(): Promise<void> {
	const store = await getStore();
	await store.set("files", []);
	cachedFiles = [];
	notifySubscribers();
}

export function subscribeToRecentFiles(
	callback: (files: RecentFile[]) => void,
): () => void {
	subscribers.add(callback);
	callback([...cachedFiles]);
	return () => subscribers.delete(callback);
}

export const recentFilesStore = {
	subscribe: subscribeToRecentFiles,
	get: getRecentFiles,
};
