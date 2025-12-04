import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

// Type definitions matching Rust types
export type ChatMode = "Docked" | "Panel" | "Detached";

export interface SidebarState {
	visible: boolean;
	width: number;
	active_view_id: string;
}

export interface PanelState {
	visible: boolean;
	height: number;
	active_tab: string;
}

export interface ChatSidebarState {
	visible: boolean;
	width: number;
	mode: ChatMode;
	detached_window_id: string | null;
}

export interface EditorAreaState {
	active_view: string;
}

export interface WindowBounds {
	x: number;
	y: number;
	width: number;
	height: number;
	maximized: boolean;
}

export interface WindowLayout {
	version: number;
	window_id: string;
	workspace_path: string | null;
	left_sidebar: SidebarState;
	right_sidebar: SidebarState;
	bottom_panel: PanelState;
	chat_sidebar: ChatSidebarState;
	editor_area: EditorAreaState;
	window_bounds: WindowBounds;
}

class LayoutStore {
	windowId = $state<string>("");
	layout = $state<WindowLayout | null>(null);
	isLoading = $state<boolean>(false);
	error = $state<string | null>(null);

	private saveDebounceTimer: number | null = null;
	private savePending = $state<boolean>(false);
	private eventUnlisteners: (() => void)[] = [];

	constructor() {
		// Listen to layout:changed events from Rust
		this.setupEventListeners();
	}

	private async setupEventListeners(): Promise<void> {
		try {
			// Listen for layout changes
			const unlistenLayout = await listen<{
				window_id: string;
				layout: WindowLayout;
			}>("layout:changed", (event) => {
				// Only update if it's for this window
				if (event.payload.window_id === this.windowId) {
					this.layout = event.payload.layout;
				}
			});

			// Listen for settings updates from other windows
			const unlistenSettings = await listen<Record<string, unknown>>(
				"settings:updated",
				async () => {
					// Reload settings to sync with other windows
					const { initGlobalSettings } =
						await import("./globalSettings");
					await initGlobalSettings();
				},
			);

			// Listen for theme changes from other windows
			const unlistenTheme = await listen<{ theme: string }>(
				"theme:changed",
				async (event) => {
					// Apply theme change
					try {
						const { setMode, resetMode } =
							await import("mode-watcher");
						if (event.payload.theme === "system") {
							resetMode();
						} else {
							setMode(event.payload.theme as "light" | "dark");
						}
					} catch (err) {
						console.error("Failed to apply theme change:", err);
					}
				},
			);

			this.eventUnlisteners.push(
				unlistenLayout,
				unlistenSettings,
				unlistenTheme,
			);
		} catch (err) {
			console.error("Failed to setup layout event listeners:", err);
		}
	}

	/**
	 * Load layout from Rust backend for a specific window
	 */
	async loadLayout(windowId: string): Promise<void> {
		if (this.isLoading) return;

		try {
			this.isLoading = true;
			this.error = null;
			this.windowId = windowId;

			const layout = await invoke<WindowLayout | null>("get_layout", {
				windowId,
			});

			if (layout) {
				this.layout = layout;
			} else {
				// No layout found, create default
				this.layout = this.createDefaultLayout(windowId);
				// Save default layout
				try {
					await this.saveLayout();
				} catch (saveErr) {
					console.warn("Failed to save default layout:", saveErr);
					// Non-critical, continue
				}
			}
		} catch (err) {
			this.error = err instanceof Error ? err.message : String(err);
			console.error("Failed to load layout:", err);
			// Fallback to default layout
			this.layout = this.createDefaultLayout(windowId);
		} finally {
			this.isLoading = false;
		}
	}

	/**
	 * Save layout to Rust backend (with debouncing)
	 */
	async saveLayout(): Promise<void> {
		if (!this.layout || !this.windowId) return;

		// Clear existing debounce timer
		if (this.saveDebounceTimer !== null) {
			clearTimeout(this.saveDebounceTimer);
		}

		this.savePending = true;

		// Debounce saves (300ms as per plan)
		this.saveDebounceTimer = window.setTimeout(async () => {
			try {
				await invoke("save_layout_cmd", {
					windowId: this.windowId,
					layout: this.layout,
				});
				this.savePending = false;
			} catch (err) {
				this.error = err instanceof Error ? err.message : String(err);
				console.error("Failed to save layout:", err);
				this.savePending = false;
			} finally {
				this.saveDebounceTimer = null;
			}
		}, 300);
	}

	/**
	 * Update a region size and save
	 */
	async updateRegionSize(region: string, size: number): Promise<void> {
		if (!this.layout || !this.windowId) return;

		// Save previous layout for rollback
		const previousLayout = $state.snapshot(this.layout);

		try {
			// Update local state
			switch (region) {
				case "left_sidebar":
					this.layout.left_sidebar.width = size;
					break;
				case "right_sidebar":
					this.layout.right_sidebar.width = size;
					break;
				case "bottom_panel":
					this.layout.bottom_panel.height = size;
					break;
				case "chat_sidebar":
					this.layout.chat_sidebar.width = size;
					break;
				default:
					throw new Error(`Unknown region: ${region}`);
			}

			// Save to backend
			await invoke("update_region_size", {
				windowId: this.windowId,
				region,
				size,
			});

			// Trigger debounced save
			await this.saveLayout();
		} catch (err) {
			// Rollback on error
			if (this.layout) {
				this.layout = previousLayout;
			}
			this.error = err instanceof Error ? err.message : String(err);
			throw err;
		}
	}

	/**
	 * Toggle a region's visibility
	 */
	async toggleRegion(region: string): Promise<void> {
		if (!this.layout || !this.windowId) return;

		// Save previous layout for rollback
		const previousLayout = $state.snapshot(this.layout);

		try {
			// Update local state
			switch (region) {
				case "left_sidebar":
					this.layout.left_sidebar.visible =
						!this.layout.left_sidebar.visible;
					break;
				case "right_sidebar":
					this.layout.right_sidebar.visible =
						!this.layout.right_sidebar.visible;
					break;
				case "bottom_panel":
					this.layout.bottom_panel.visible =
						!this.layout.bottom_panel.visible;
					break;
				case "chat_sidebar":
					this.layout.chat_sidebar.visible =
						!this.layout.chat_sidebar.visible;
					break;
				default:
					throw new Error(`Unknown region: ${region}`);
			}

			// Save to backend
			await invoke("toggle_region", {
				windowId: this.windowId,
				region,
			});

			// Trigger debounced save
			await this.saveLayout();
		} catch (err) {
			// Rollback on error
			if (this.layout) {
				this.layout = previousLayout;
			}
			this.error = err instanceof Error ? err.message : String(err);
			throw err;
		}
	}

	/**
	 * Set chat mode
	 */
	async setChatMode(mode: ChatMode): Promise<void> {
		if (!this.layout || !this.windowId) return;

		// Save previous layout for rollback
		const previousLayout = $state.snapshot(this.layout);

		try {
			// Update local state
			this.layout.chat_sidebar.mode = mode;

			// Save to backend
			await invoke("set_chat_mode", {
				windowId: this.windowId,
				mode,
			});

			// Trigger debounced save
			await this.saveLayout();
		} catch (err) {
			// Rollback on error
			if (this.layout) {
				this.layout = previousLayout;
			}
			this.error = err instanceof Error ? err.message : String(err);
			throw err;
		}
	}

	/**
	 * Create a default layout for a window
	 */
	private createDefaultLayout(windowId: string): WindowLayout {
		return {
			version: 1,
			window_id: windowId,
			workspace_path: null,
			left_sidebar: {
				visible: false,
				width: 250,
				active_view_id: "",
			},
			right_sidebar: {
				visible: false,
				width: 360,
				active_view_id: "",
			},
			bottom_panel: {
				visible: false,
				height: 200,
				active_tab: "problems",
			},
			chat_sidebar: {
				visible: true,
				width: 360,
				mode: "Docked",
				detached_window_id: null,
			},
			editor_area: {
				active_view: "spreadsheet",
			},
			window_bounds: {
				x: 100,
				y: 100,
				width: 1200,
				height: 800,
				maximized: false,
			},
		};
	}

	/**
	 * Cleanup on destroy
	 */
	destroy(): void {
		if (this.saveDebounceTimer !== null) {
			clearTimeout(this.saveDebounceTimer);
		}
		this.eventUnlisteners.forEach((unlisten) => unlisten());
		this.eventUnlisteners = [];
	}
}

export const layoutStore = new LayoutStore();
