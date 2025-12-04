import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { emit, listen } from "@tauri-apps/api/event";

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

/**
 * Window manager utility for Tauri window operations
 */
export const windowManager = {
	/**
	 * Create a new main window with optional workspace path
	 */
	async createMainWindow(
		workspacePath?: string,
	): Promise<string> {
		try {
			const windowId = await invoke<string>("create_window", {
				windowLabel: `main_${Date.now()}`,
				workspacePath: workspacePath || null,
				initialLayout: null,
			});

			return windowId;
		} catch (err) {
			console.error("Failed to create main window:", err);
			throw err;
		}
	},

	/**
	 * Create a detached chat window
	 */
	async createChatWindow(sourceWindowId: string): Promise<string> {
		try {
			const windowId = await invoke<string>("create_chat_window", {
				sourceWindowId,
			});

			return windowId;
		} catch (err) {
			console.error("Failed to create chat window:", err);
			throw err;
		}
	},

	/**
	 * Get the current window ID
	 */
	async getCurrentWindowId(): Promise<string> {
		try {
		const window = await getCurrentWindow();
		return window.label;
		} catch (err) {
			console.error("Failed to get current window ID:", err);
			throw err;
		}
	},

	/**
	 * Send an event to a specific window
	 */
	async sendToWindow(
		windowId: string,
		event: string,
		payload: any,
	): Promise<void> {
		try {
			// Use emit with window label
			await emit(event, payload);
			// Note: Tauri v2's emit doesn't support targeting specific windows directly
			// The backend should handle routing based on window_id in payload
		} catch (err) {
			console.error(`Failed to send event to window ${windowId}:`, err);
			throw err;
		}
	},

	/**
	 * Broadcast an event to all windows
	 */
	async broadcast(event: string, payload: any): Promise<void> {
		try {
			await emit(event, payload);
		} catch (err) {
			console.error(`Failed to broadcast event ${event}:`, err);
			throw err;
		}
	},

	/**
	 * Listen to an event
	 */
	async listen<T = any>(
		event: string,
		handler: (payload: T) => void,
	): Promise<() => void> {
		try {
			const unlisten = await listen<T>(event, (event) => {
				handler(event.payload);
			});
			return unlisten;
		} catch (err) {
			console.error(`Failed to listen to event ${event}:`, err);
			throw err;
		}
	},
};

