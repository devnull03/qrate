import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

export interface WindowInfo {
	window_id: string;
	window_label: string;
	workspace_path: string | null;
	is_main: boolean;
	is_chat: boolean;
}

export interface WindowLayout {
	version: number;
	window_id: string;
	workspace_path: string | null;
	left_sidebar: any;
	right_sidebar: any;
	bottom_panel: any;
	chat_sidebar: any;
	editor_area: any;
	window_bounds: any;
}

class WindowStore {
	windows = $state<WindowInfo[]>([]);
	currentWindowId = $state<string>("");
	isLoading = $state<boolean>(false);
	error = $state<string | null>(null);

	private eventUnlisteners: (() => void)[] = [];

	constructor() {
		// Initialize current window ID
		this.initCurrentWindow();
		// Setup event listeners
		this.setupEventListeners();
		// Load initial window list
		this.getWindowList();
	}

	private async initCurrentWindow(): Promise<void> {
		try {
			const window = await getCurrentWindow();
			this.currentWindowId = window.label;
		} catch (err) {
			console.error("Failed to get current window:", err);
		}
	}

	private async setupEventListeners(): Promise<void> {
		try {
			// Listen to window:created events
			const unlistenCreated = await listen<{
				window_id: string;
				window_label: string;
			}>("window:created", async () => {
				// Refresh window list
				await this.getWindowList();
			});

			// Listen to window:closed events
			const unlistenClosed = await listen<{
				window_id: string;
			}>("window:closed", async (event) => {
				// Remove from list
				this.windows = this.windows.filter(
					(w) => w.window_id !== event.payload.window_id,
				);
			});

			// Listen to window:focused events
			const unlistenFocused = await listen<{
				window_id: string;
			}>("window:focused", (event) => {
				// Update current window if it's this window
				if (event.payload.window_id === this.currentWindowId) {
					// Window is focused
				}
			});

			this.eventUnlisteners = [
				unlistenCreated,
				unlistenClosed,
				unlistenFocused,
			];
		} catch (err) {
			console.error("Failed to setup window event listeners:", err);
		}
	}

	/**
	 * Get list of all active windows
	 */
	async getWindowList(): Promise<void> {
		try {
			this.isLoading = true;
			this.error = null;

			const windows = await invoke<WindowInfo[]>("get_window_list");
			this.windows = windows;
		} catch (err) {
			this.error = err instanceof Error ? err.message : String(err);
			console.error("Failed to get window list:", err);
		} finally {
			this.isLoading = false;
		}
	}

	/**
	 * Create a new main window
	 */
	async createWindow(
		windowLabel: string,
		workspacePath?: string,
		initialLayout?: Partial<WindowLayout>,
	): Promise<string> {
		try {
			this.isLoading = true;
			this.error = null;

			const windowId = await invoke<string>("create_window", {
				windowLabel,
				workspacePath: workspacePath || null,
				initialLayout: initialLayout || null,
			});

			// Refresh window list
			await this.getWindowList();

			return windowId;
		} catch (err) {
			this.error = err instanceof Error ? err.message : String(err);
			throw err;
		} finally {
			this.isLoading = false;
		}
	}

	/**
	 * Close a window
	 */
	async closeWindow(windowId: string): Promise<void> {
		try {
			this.isLoading = true;
			this.error = null;

			await invoke("close_window", { windowId });

			// Refresh window list
			await this.getWindowList();
		} catch (err) {
			this.error = err instanceof Error ? err.message : String(err);
			throw err;
		} finally {
			this.isLoading = false;
		}
	}

	/**
	 * Focus a window
	 */
	async focusWindow(windowId: string): Promise<void> {
		try {
			this.error = null;

			await invoke("focus_window", { windowId });
		} catch (err) {
			this.error = err instanceof Error ? err.message : String(err);
			throw err;
		}
	}

	/**
	 * Get main windows only
	 */
	get mainWindows(): WindowInfo[] {
		return this.windows.filter((w) => w.is_main);
	}

	/**
	 * Get chat windows only
	 */
	get chatWindows(): WindowInfo[] {
		return this.windows.filter((w) => w.is_chat);
	}

	/**
	 * Cleanup on destroy
	 */
	destroy(): void {
		this.eventUnlisteners.forEach((unlisten) => unlisten());
	}
}

export const windowStore = new WindowStore();

