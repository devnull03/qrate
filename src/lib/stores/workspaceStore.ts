import { Store } from "@tauri-store/svelte";

export interface WorkspaceState {
	currentFilePath: string | null;
	currentOffset: number;
	currentLimit: number;
	scrollPosition: number;
	[key: string]: string | number | null;
}

const defaultState: WorkspaceState = {
	currentFilePath: null,
	currentOffset: 0,
	currentLimit: 100,
	scrollPosition: 0,
};

export const workspaceStore = new Store<WorkspaceState>(
	"workspace",
	defaultState,
	{
		autoStart: true,
	},
);

/**
 * Save the current workspace state
 */
export function saveWorkspace(
	filePath: string | null,
	offset: number = 0,
	limit: number = 100,
	scrollPosition: number = 0,
): void {
	workspaceStore.set({
		currentFilePath: filePath,
		currentOffset: offset,
		currentLimit: limit,
		scrollPosition: scrollPosition,
	});
}

/**
 * Clear the workspace (e.g., when closing a file)
 */
export function clearWorkspace(): void {
	workspaceStore.set(defaultState);
}

/**
 * Update just the scroll/viewport position
 */
export function updateViewport(
	offset: number,
	limit: number,
	scrollPosition?: number,
): void {
	workspaceStore.update((state) => ({
		...state,
		currentOffset: offset,
		currentLimit: limit,
		scrollPosition: scrollPosition ?? state.scrollPosition,
	}));
}
