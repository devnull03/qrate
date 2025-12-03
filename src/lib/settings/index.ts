import { invoke } from "@tauri-apps/api/core";
import type {
	SettingDefResponse,
	SettingMetadata,
	SettingType,
	GlobalSettings,
	ProjectSettings,
} from "./types";

// Minimal, stateless wrappers that call the backend on demand.
// These do not cache or maintain any frontend state.

export type {
	SettingDefResponse,
	SettingMetadata,
	SettingType,
	GlobalSettings,
	ProjectSettings,
};

/**
 * Get all global settings default values from the backend.
 */
export async function getGlobalDefaults(): Promise<Record<string, string>> {
	try {
		return (await invoke("get_global_settings_defaults")) as Record<
			string,
			string
		>;
	} catch (err) {
		console.error("[settings] getGlobalDefaults failed:", err);
		return {};
	}
}

/**
 * Get all project settings default values from the backend.
 */
export async function getProjectDefaults(): Promise<Record<string, string>> {
	try {
		return (await invoke("get_project_settings_defaults")) as Record<
			string,
			string
		>;
	} catch (err) {
		console.error("[settings] getProjectDefaults failed:", err);
		return {};
	}
}

/**
 * Get a single global setting definition (metadata) by key.
 */
export async function getGlobalSettingDef(
	key: string,
): Promise<SettingDefResponse | undefined> {
	try {
		const all = (await invoke(
			"get_global_settings_schema",
		)) as SettingDefResponse[];
		return all.find((s) => s.key === key);
	} catch (err) {
		console.error("[settings] getGlobalSettingDef failed:", err);
		return undefined;
	}
}

/**
 * Get a single project setting definition (metadata) by key.
 */
export async function getProjectSettingDef(
	key: string,
): Promise<SettingDefResponse | undefined> {
	try {
		const all = (await invoke(
			"get_project_settings_schema",
		)) as SettingDefResponse[];
		return all.find((s) => s.key === key);
	} catch (err) {
		console.error("[settings] getProjectSettingDef failed:", err);
		return undefined;
	}
}

/**
 * Validate a setting on the server (backend) to ensure rules are enforced.
 * Returns { valid: true } on success, or { valid: false, error } on failure.
 */
export async function validateSettingServer(
	key: string,
	value: string,
	scope: "global" | "project",
): Promise<{ valid: boolean; error?: string }> {
	try {
		await invoke("validate_setting_value", { key, value, scope });
		return { valid: true };
	} catch (err) {
		const message = err instanceof Error ? err.message : String(err);
		return { valid: false, error: message };
	}
}
