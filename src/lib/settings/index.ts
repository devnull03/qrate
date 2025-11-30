/**
 * Settings Module
 *
 * Centralized settings management for the application.
 * Import from here to access settings types, defaults, and metadata.
 */

// Schema exports - types, defaults, and metadata
export {
	// Global settings
	type GlobalSettings,
	defaultGlobalSettings,
	globalSettingsMetadata,

	// Project settings
	type ProjectSettings,
	defaultProjectSettings,
	projectSettingsMetadata,

	// Metadata types
	type SettingType,
	type SettingMetadata,

	// Helper functions
	getGlobalSettingCategories,
	getProjectSettingCategories,
	getGlobalSettingsByCategory,
	getProjectSettingsByCategory,
	validateSetting,
} from "./schema";
