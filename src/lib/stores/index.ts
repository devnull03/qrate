// =============================================================================
// Settings - Schema and types (from settings module)
// =============================================================================
export {
	// Minimal types for metadata
	type SettingType,
	type SettingMetadata,
	type SettingDefResponse,

	// Minimal, stateless invoke wrappers
	getGlobalDefaults,
	getProjectDefaults,
	getGlobalSettingDef,
	getProjectSettingDef,
	validateSettingServer as validateSettingValue,
} from "$lib/settings";

// =============================================================================
// Global Settings Store - App-wide settings (persisted via Tauri store)
// =============================================================================
export {
	initGlobalSettings,
	getGlobalSettings,
	getGlobalSetting,
	setGlobalSetting,
	setGlobalSettings,
	resetGlobalSettings,
	subscribeToGlobalSettings,
} from "./globalSettings";

// =============================================================================
// Project Settings - Project-specific settings (stored in .qrate file)
// =============================================================================
export {
	loadSettings,
	saveSetting,
	saveSettings,
	getSettings,
	subscribeToSettings,
	getDefaultProjectSettings,
	resolveFilePath,
	// Legacy aliases
	defaultSettings,
	type AppSettings,
} from "./appSettings";

// =============================================================================
// Workspace Store - Persistent open file state (survives refresh)
// =============================================================================
export {
	workspaceStore,
	initWorkspace,
	getWorkspace,
	saveWorkspace,
	clearWorkspace,
	updateViewport,
	subscribeToWorkspace,
	type WorkspaceState,
} from "./workspaceStore";

// =============================================================================
// Recent Files Store - Persistent history of opened files
// =============================================================================
export {
	recentFilesStore,
	initRecentFiles,
	getRecentFiles,
	addRecentFile,
	removeRecentFile,
	clearRecentFiles,
	subscribeToRecentFiles,
	type RecentFile,
} from "./recentFiles";

// =============================================================================
// Qrate Store - Main application state (uses Svelte 5 runes)
// =============================================================================
export {
	qrateStore,
	type ColumnDef,
	type FileOpenResponse,
	type DataResponse,
} from "./qrateStore.svelte";
