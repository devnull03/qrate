// App Settings - persistent user preferences
export { appSettingsStore, type AppSettings } from "./appSettings";

// Workspace - persistent open file state (survives refresh)
export {
	workspaceStore,
	saveWorkspace,
	clearWorkspace,
	updateViewport,
	type WorkspaceState,
} from "./workspaceStore";

// Recent Files - persistent history of opened files
export {
	recentFilesStore,
	addRecentFile,
	removeRecentFile,
	clearRecentFiles,
	type RecentFile,
	type RecentFilesState,
} from "./recentFiles";

// Qrate Store - main application state (uses runes)
export { qrateStore, type ColumnDef, type FileOpenResponse, type DataResponse } from "./qrateStore.svelte";

// CSV Store - legacy CSV loading (uses runes)
export { csvStore } from "./csvStore.svelte";
