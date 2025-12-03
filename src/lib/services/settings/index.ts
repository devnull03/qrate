import { invoke } from "@tauri-apps/api/core";
import type { SettingDefResponse } from "$lib/models/settings";

export async function getGlobalDefaults(): Promise<Record<string, string>> {
	return invoke<Record<string, string>>("get_global_settings_defaults").catch(() => ({}));
}

export async function getProjectDefaults(): Promise<Record<string, string>> {
	return invoke<Record<string, string>>("get_project_settings_defaults").catch(() => ({}));
}

export async function getGlobalSettingDef(key: string): Promise<SettingDefResponse | undefined> {
	const all = await invoke<SettingDefResponse[]>("get_global_settings_schema").catch(() => []);
	return all.find((s) => s.key === key);
}

export async function getProjectSettingDef(key: string): Promise<SettingDefResponse | undefined> {
	const all = await invoke<SettingDefResponse[]>("get_project_settings_schema").catch(() => []);
	return all.find((s) => s.key === key);
}

export async function validateSettingValue(
	key: string,
	value: string,
	scope: "global" | "project"
): Promise<{ valid: boolean; error?: string }> {
	try {
		await invoke("validate_setting_value", { key, value, scope });
		return { valid: true };
	} catch (err) {
		return { valid: false, error: err instanceof Error ? err.message : String(err) };
	}
}
