import { Store } from "@tauri-store/svelte";

export interface RecentFile {
	path: string;
	name: string;
	lastOpened: number; // timestamp
}

export interface RecentFilesState {
	files: RecentFile[];
	maxFiles: number;
	lastOpenedPath: string | null;
	[key: string]: RecentFile[] | number | string | null;
}

const defaultState: RecentFilesState = {
	files: [],
	maxFiles: 10,
	lastOpenedPath: null,
};

export const recentFilesStore = new Store<RecentFilesState>(
	"recent-files",
	defaultState,
	{
		autoStart: true,
	},
);

/**
 * Add a file to the recent files list
 */
export function addRecentFile(path: string, name: string): void {
	recentFilesStore.update((state) => {
		// Remove if already exists
		const filtered = state.files.filter((f) => f.path !== path);

		// Add to the beginning
		const newFiles = [{ path, name, lastOpened: Date.now() }, ...filtered];

		// Trim to max files
		return {
			...state,
			files: newFiles.slice(0, state.maxFiles),
			lastOpenedPath: path,
		};
	});
}

/**
 * Remove a file from the recent files list
 */
export function removeRecentFile(path: string): void {
	recentFilesStore.update((state) => ({
		...state,
		files: state.files.filter((f) => f.path !== path),
		lastOpenedPath:
			state.lastOpenedPath === path ? null : state.lastOpenedPath,
	}));
}

/**
 * Clear all recent files
 */
export function clearRecentFiles(): void {
	recentFilesStore.update((state) => ({
		...state,
		files: [],
		lastOpenedPath: null,
	}));
}
