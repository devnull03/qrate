import { load } from "@tauri-apps/plugin-store";
import type { Store } from "@tauri-apps/plugin-store";

/**
 * Workspace state that persists across app restarts.
 * Tracks the current open file and viewport position.
 */
export interface WorkspaceState {
	currentFilePath: string | null;
	currentOffset: number;
	currentLimit: number;
	scrollPosition: number;
}

export const defaultWorkspaceState: WorkspaceState = {
	currentFilePath: null,
	currentOffset: 0,
	currentLimit: 100,
	scrollPosition: 0,
};

// Store instance (lazy loaded)
let storeInstance: Store | null = null;
let storePromise: Promise<Store> | null = null;

// In-memory cache for synchronous access
let cachedState: WorkspaceState = { ...defaultWorkspaceState };
let subscribers: Set<(state: WorkspaceState) => void> = new Set();

/**
 * Get the store instance (creates it if needed)
 */
async function getStore(): Promise<Store> {
	if (storeInstance) return storeInstance;

	if (!storePromise) {
		storePromise = load("workspace.json");
	}

	storeInstance = await storePromise;
	return storeInstance;
}

/**
 * Notify all subscribers of state changes
 */
function notifySubscribers() {
	console.log(
		"[workspaceStore] Notifying subscribers, count:",
		subscribers.size,
	);
	subscribers.forEach((callback) => callback({ ...cachedState }));
}

/**
 * Initialize and load workspace state
 * Call this early in the app lifecycle
 */
export async function initWorkspace(): Promise<WorkspaceState> {
	console.log("[workspaceStore] Initializing...");

	try {
		const store = await getStore();

		// Load each field, falling back to defaults
		for (const key of Object.keys(defaultWorkspaceState) as Array<
			keyof WorkspaceState
		>) {
			const value = await store.get<WorkspaceState[typeof key]>(key);
			if (value !== null && value !== undefined) {
				(cachedState as unknown as Record<string, unknown>)[key] =
					value;
			}
		}

		console.log("[workspaceStore] Loaded state:", cachedState);
		notifySubscribers();
		return cachedState;
	} catch (err) {
		console.error("[workspaceStore] Failed to initialize:", err);
		return cachedState;
	}
}

/**
 * Get current workspace state (synchronous, returns cached value)
 */
export function getWorkspace(): WorkspaceState {
	return { ...cachedState };
}

/**
 * Save the current workspace state
 */
export async function saveWorkspace(
	filePath: string | null,
	offset: number = 0,
	limit: number = 100,
	scrollPosition: number = 0,
): Promise<void> {
	console.log("[workspaceStore] Saving workspace:", filePath);

	try {
		const store = await getStore();

		const newState: WorkspaceState = {
			currentFilePath: filePath,
			currentOffset: offset,
			currentLimit: limit,
			scrollPosition: scrollPosition,
		};

		for (const [key, value] of Object.entries(newState)) {
			await store.set(key, value);
		}

		cachedState = newState;
		console.log("[workspaceStore] Workspace saved");
		notifySubscribers();
	} catch (err) {
		console.error("[workspaceStore] Failed to save workspace:", err);
		throw err;
	}
}

/**
 * Clear the workspace (e.g., when closing a file)
 */
export async function clearWorkspace(): Promise<void> {
	console.log("[workspaceStore] Clearing workspace");

	try {
		const store = await getStore();

		for (const [key, value] of Object.entries(defaultWorkspaceState)) {
			await store.set(key, value);
		}

		cachedState = { ...defaultWorkspaceState };
		notifySubscribers();
	} catch (err) {
		console.error("[workspaceStore] Failed to clear workspace:", err);
		throw err;
	}
}

/**
 * Update just the scroll/viewport position
 */
export async function updateViewport(
	offset: number,
	limit: number,
	scrollPosition?: number,
): Promise<void> {
	console.log("[workspaceStore] Updating viewport:", { offset, limit });

	try {
		const store = await getStore();

		await store.set("currentOffset", offset);
		await store.set("currentLimit", limit);

		cachedState.currentOffset = offset;
		cachedState.currentLimit = limit;

		if (scrollPosition !== undefined) {
			await store.set("scrollPosition", scrollPosition);
			cachedState.scrollPosition = scrollPosition;
		}

		notifySubscribers();
	} catch (err) {
		console.error("[workspaceStore] Failed to update viewport:", err);
		throw err;
	}
}

/**
 * Subscribe to workspace state changes
 * Returns an unsubscribe function
 */
export function subscribeToWorkspace(
	callback: (state: WorkspaceState) => void,
): () => void {
	console.log("[workspaceStore] New subscriber added");
	subscribers.add(callback);

	// Immediately call with current value
	callback({ ...cachedState });

	return () => {
		console.log("[workspaceStore] Subscriber removed");
		subscribers.delete(callback);
	};
}

/**
 * Reactive store interface for Svelte compatibility
 */
export const workspaceStore = {
	subscribe: subscribeToWorkspace,
	get: getWorkspace,
	/**
	 * Start method for compatibility - initializes the store
	 */
	start: initWorkspace,
};
