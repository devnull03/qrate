/**
 * Settings Schema
 *
 * This file defines all settings for the application.
 * To add a new setting:
 * 1. Add it to the appropriate interface (GlobalSettings or ProjectSettings)
 * 2. Add the default value to the corresponding defaults object
 * 3. Add metadata to the settingsMetadata object for UI generation
 *
 * The settings will automatically be available in the settings UI and persisted.
 */

// =============================================================================
// GLOBAL SETTINGS - Persist across all projects and app restarts
// =============================================================================

export interface GlobalSettings {
	// Appearance
	theme: "light" | "dark" | "system";

	// Default values for new projects
	defaultRowLimit: number;
	defaultFilePathPattern: string;
	defaultFileColumnName: string;

	// UI preferences
	sidebarWidth: number;
	sidebarCollapsed: boolean;
	showStatusBar: boolean;

	// Behavior
	autoSaveInterval: number; // in milliseconds, 0 = disabled
	confirmBeforeDelete: boolean;
	reopenLastProject: boolean;

	// Grid/Table
	gridRowHeight: number;
	gridAlternateRowColors: boolean;
}

export const defaultGlobalSettings: GlobalSettings = {
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

// =============================================================================
// PROJECT SETTINGS - Stored in .qrate file, specific to each project
// =============================================================================

export interface ProjectSettings {
	// File handling
	filesFolder: string;
	filePathPattern: string;
	fileColumnName: string;

	// Data
	defaultRowLimit: string; // stored as string in the database

	// View state (optional, per-project)
	lastSelectedRowId?: number;
	lastScrollPosition?: number;

	// Index signature for dynamic access
	[key: string]: string | number | undefined;
}

export const defaultProjectSettings: ProjectSettings = {
	filesFolder: "",
	filePathPattern: "{files_folder}/{file_column}",
	fileColumnName: "file",
	defaultRowLimit: "100",
};

// =============================================================================
// SETTINGS METADATA - For UI generation and validation
// =============================================================================

export type SettingType =
	| "string"
	| "number"
	| "boolean"
	| "select"
	| "path"
	| "pattern";

export interface SettingMetadata {
	label: string;
	description: string;
	type: SettingType;
	category: string;
	options?: { value: string; label: string }[]; // for select type
	min?: number; // for number type
	max?: number; // for number type
	step?: number; // for number type
	placeholder?: string;
	requiresRestart?: boolean;
}

export const globalSettingsMetadata: Record<
	keyof GlobalSettings,
	SettingMetadata
> = {
	// Appearance
	theme: {
		label: "Theme",
		description: "Choose the application color theme",
		type: "select",
		category: "Appearance",
		options: [
			{ value: "light", label: "Light" },
			{ value: "dark", label: "Dark" },
			{ value: "system", label: "System" },
		],
	},

	// Default values for new projects
	defaultRowLimit: {
		label: "Default Row Limit",
		description:
			"Default number of rows to load at a time for new projects",
		type: "number",
		category: "Defaults",
		min: 50,
		max: 1000,
		step: 50,
	},
	defaultFilePathPattern: {
		label: "Default File Path Pattern",
		description: "Default pattern for locating files in new projects",
		type: "pattern",
		category: "Defaults",
		placeholder: "{files_folder}/{file_column}",
	},
	defaultFileColumnName: {
		label: "Default File Column Name",
		description: "Default column name for file references in new projects",
		type: "string",
		category: "Defaults",
		placeholder: "file",
	},

	// UI preferences
	sidebarWidth: {
		label: "Sidebar Width",
		description: "Width of the sidebar in pixels",
		type: "number",
		category: "UI",
		min: 200,
		max: 500,
		step: 10,
	},
	sidebarCollapsed: {
		label: "Sidebar Collapsed",
		description: "Whether the sidebar starts collapsed",
		type: "boolean",
		category: "UI",
	},
	showStatusBar: {
		label: "Show Status Bar",
		description: "Show the status bar at the bottom of the window",
		type: "boolean",
		category: "UI",
	},

	// Behavior
	autoSaveInterval: {
		label: "Auto-Save Interval",
		description: "Interval for auto-saving changes (0 to disable)",
		type: "number",
		category: "Behavior",
		min: 0,
		max: 60000,
		step: 1000,
	},
	confirmBeforeDelete: {
		label: "Confirm Before Delete",
		description: "Show confirmation dialog before deleting rows",
		type: "boolean",
		category: "Behavior",
	},
	reopenLastProject: {
		label: "Reopen Last Project",
		description: "Automatically reopen the last project on startup",
		type: "boolean",
		category: "Behavior",
	},

	// Grid/Table
	gridRowHeight: {
		label: "Grid Row Height",
		description: "Height of each row in the data grid",
		type: "number",
		category: "Grid",
		min: 24,
		max: 64,
		step: 4,
	},
	gridAlternateRowColors: {
		label: "Alternate Row Colors",
		description: "Use alternating colors for grid rows",
		type: "boolean",
		category: "Grid",
	},
};

export const projectSettingsMetadata: Record<
	keyof ProjectSettings,
	SettingMetadata
> = {
	filesFolder: {
		label: "Files Folder",
		description:
			"Base folder containing all files referenced in this project",
		type: "path",
		category: "Files",
	},
	filePathPattern: {
		label: "File Path Pattern",
		description:
			"Pattern for locating files. Use {files_folder}, {file_column}, or any column name.",
		type: "pattern",
		category: "Files",
		placeholder: "{files_folder}/{file_column}",
	},
	fileColumnName: {
		label: "File Column Name",
		description: "The column name containing file names in your CSV",
		type: "string",
		category: "Files",
		placeholder: "file",
	},
	defaultRowLimit: {
		label: "Row Limit",
		description: "Number of rows to load at a time for this project",
		type: "number",
		category: "Data",
		min: 50,
		max: 1000,
		step: 50,
	},
	lastSelectedRowId: {
		label: "Last Selected Row",
		description: "Internal: tracks the last selected row",
		type: "number",
		category: "Internal",
	},
	lastScrollPosition: {
		label: "Last Scroll Position",
		description: "Internal: tracks the last scroll position",
		type: "number",
		category: "Internal",
	},
};

// =============================================================================
// HELPER FUNCTIONS
// =============================================================================

/**
 * Get all setting categories for global settings
 */
export function getGlobalSettingCategories(): string[] {
	const categories = new Set<string>();
	for (const meta of Object.values(globalSettingsMetadata)) {
		categories.add(meta.category);
	}
	return Array.from(categories);
}

/**
 * Get all setting categories for project settings
 */
export function getProjectSettingCategories(): string[] {
	const categories = new Set<string>();
	for (const meta of Object.values(projectSettingsMetadata)) {
		if (meta.category !== "Internal") {
			categories.add(meta.category);
		}
	}
	return Array.from(categories);
}

/**
 * Get global settings by category
 */
export function getGlobalSettingsByCategory(
	category: string,
): Array<{ key: keyof GlobalSettings; metadata: SettingMetadata }> {
	return Object.entries(globalSettingsMetadata)
		.filter(([_, meta]) => meta.category === category)
		.map(([key, metadata]) => ({
			key: key as keyof GlobalSettings,
			metadata,
		}));
}

/**
 * Get project settings by category (excludes Internal)
 */
export function getProjectSettingsByCategory(
	category: string,
): Array<{ key: keyof ProjectSettings; metadata: SettingMetadata }> {
	return Object.entries(projectSettingsMetadata)
		.filter(
			([_, meta]) =>
				meta.category === category && meta.category !== "Internal",
		)
		.map(([key, metadata]) => ({
			key: key as keyof ProjectSettings,
			metadata,
		}));
}

/**
 * Validate a setting value against its metadata
 */
export function validateSetting(
	value: unknown,
	metadata: SettingMetadata,
): { valid: boolean; error?: string } {
	switch (metadata.type) {
		case "number":
			if (typeof value !== "number") {
				return { valid: false, error: "Value must be a number" };
			}
			if (metadata.min !== undefined && value < metadata.min) {
				return {
					valid: false,
					error: `Value must be at least ${metadata.min}`,
				};
			}
			if (metadata.max !== undefined && value > metadata.max) {
				return {
					valid: false,
					error: `Value must be at most ${metadata.max}`,
				};
			}
			break;

		case "boolean":
			if (typeof value !== "boolean") {
				return { valid: false, error: "Value must be true or false" };
			}
			break;

		case "string":
		case "path":
		case "pattern":
			if (typeof value !== "string") {
				return { valid: false, error: "Value must be a string" };
			}
			break;

		case "select":
			if (
				metadata.options &&
				!metadata.options.some((opt) => opt.value === value)
			) {
				return { valid: false, error: "Invalid option selected" };
			}
			break;
	}

	return { valid: true };
}
