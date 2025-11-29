import { Store } from "@tauri-store/svelte";

export interface AppSettings {
	theme: "light" | "dark" | "system";
	defaultRowLimit: number;
	autoSaveInterval: number; // in milliseconds
	showHiddenColumns: boolean;
	sidebarCollapsed: boolean;
	[key: string]: string | number | boolean;
}

const defaultSettings: AppSettings = {
	theme: "system",
	defaultRowLimit: 100,
	autoSaveInterval: 30000,
	showHiddenColumns: false,
	sidebarCollapsed: false,
};

export const appSettingsStore = new Store<AppSettings>(
	"app-settings",
	defaultSettings,
	{
		autoStart: true,
	},
);
