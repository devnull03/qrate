export type SettingType = "string" | "number" | "boolean" | "select" | "path" | "pattern";

export interface SettingOption {
	value: string;
	label: string;
}

export interface SettingMetadata {
	label: string;
	description: string;
	type: SettingType;
	category: string;
	options?: string[] | SettingOption[];
	min?: number;
	max?: number;
	step?: number;
	placeholder?: string;
	requiresRestart?: boolean;
}

export interface GlobalSettings {
	[key: string]: string | number | boolean | undefined;
}

export interface ProjectSettings {
	[key: string]: string | number | undefined;
}

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
