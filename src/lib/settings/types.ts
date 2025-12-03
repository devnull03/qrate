/**
 * Settings Types
 *
 * Common types for settings used by the frontend UI and stores.
 *
 * These types are intentionally small and focused on runtime/compile-time
 * compatibility with the backend schema exposed by the Rust/Tauri app.
 *
 * Keep these in sync with the backend schema (src-tauri/src/settings.rs).
 */

/**
 * Low-level type aliases used in metadata
 */
export type SettingType =
	| "string"
	| "number"
	| "boolean"
	| "select"
	| "path"
	| "pattern";

export interface SettingOption {
	value: string;
	label: string;
}

/**
 * Metadata describing a single setting used by the UI
 *
 * This is a light-weight version that matches what the UI needs to
 * render inputs. The server (Rust) exposes a similar structure via
 * `SettingDefResponse` (SettingDef), so keep these fields aligned.
 */
export interface SettingMetadata {
	// Display info
	label: string;
	description: string;

	// Type & category for UI rendering/validation
	type: SettingType;
	category: string;

	// Select options (for `select` fields)
	options?: string[] | SettingOption[];

	// Number bounds for `number` fields
	min?: number;
	max?: number;
	step?: number;

	// Placeholder text for text inputs, patterns
	placeholder?: string;

	// Flag used to indicate a preference that requires an app restart
	requiresRestart?: boolean;
}

/**
 * Global (app-wide) settings interface
 *
 * These settings persist across projects and are stored via the app
 * storage plugin (e.g., tauri-plugin-store).
 */
export interface GlobalSettings {
	[key: string]: string | number | boolean | undefined;
}

/**
 * Project (per-file) settings interface
 *
 * These are stored in the project's .qrate file (in the `_settings`
 * table) and are specific to each project.
 *
 * NOTE: `defaultRowLimit` is intentionally a string because we store the
 * values as strings in the DB, and the app has historically used it as
 * such (parse to number when needed).
 */
export interface ProjectSettings {
	[key: string]: string | number | undefined;
}

/**
 * Runtime setting definition interface returned by the backend.
 *
 * Mirrors the `SettingDefResponse` struct on the Rust side and is useful
 * for runtime form generation/validation when the frontend fetches schema
 * from the backend. This can be imported by UI components that render
 * dynamic forms.
 */
export interface SettingDefResponse {
	key: string;
	scope: "global" | "project";
	setting_type: SettingType;
	default_value: string;
	label: string;
	description: string;
	category: string;
	min?: number | null;
	max?: number | null;
	options?: string[] | null;
}
