import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { layoutStore } from "./layoutStore.svelte";
import { windowStore } from "./windowStore.svelte";

export type ChatMode = "Docked" | "Panel" | "Detached";

class ChatStore {
	private eventUnlisteners: (() => void)[] = [];

	constructor() {
		// Setup event listeners
		this.setupEventListeners();
	}

	/**
	 * Get current chat mode from layoutStore
	 */
	get mode(): ChatMode {
		return layoutStore.layout?.chat_sidebar?.mode ?? "Docked";
	}

	/**
	 * Get current chat visibility from layoutStore
	 */
	get visible(): boolean {
		return layoutStore.layout?.chat_sidebar?.visible ?? true;
	}

	/**
	 * Get current chat width from layoutStore
	 */
	get width(): number {
		return layoutStore.layout?.chat_sidebar?.width ?? 360;
	}

	/**
	 * Get detached window ID from layoutStore
	 */
	get detachedWindowId(): string | null {
		return layoutStore.layout?.chat_sidebar?.detached_window_id ?? null;
	}

	private async setupEventListeners(): Promise<void> {
		try {
			// Listen to chat:detached events
			const unlistenDetached = await listen<{
				source_window_id: string;
				chat_window_id: string;
			}>("chat:detached", (event) => {
				// If this is the source window, update state via layoutStore
				if (
					event.payload.source_window_id ===
					windowStore.currentWindowId
				) {
					// State will be updated via layoutStore
				}
			});

			// Listen to chat:reattached events
			const unlistenReattached = await listen<{
				source_window_id: string;
			}>("chat:reattached", (event) => {
				// If this is the source window, reattach chat
				if (
					event.payload.source_window_id ===
					windowStore.currentWindowId
				) {
					// State will be updated via layoutStore
				}
			});

			this.eventUnlisteners = [unlistenDetached, unlistenReattached];
		} catch (err) {
			console.error("Failed to setup chat event listeners:", err);
		}
	}

	/**
	 * Toggle chat visibility
	 */
	async toggleVisible(): Promise<void> {
		if (!windowStore.currentWindowId) return;

		try {
			await layoutStore.toggleRegion("chat_sidebar");
		} catch (err) {
			console.error("Failed to toggle chat visibility:", err);
			throw err;
		}
	}

	/**
	 * Set chat mode (Docked, Panel, or Detached)
	 */
	async setMode(mode: ChatMode): Promise<void> {
		if (!windowStore.currentWindowId) return;

		try {
			await layoutStore.setChatMode(mode);
		} catch (err) {
			console.error("Failed to set chat mode:", err);
			throw err;
		}
	}

	/**
	 * Detach chat to a separate window
	 */
	async detach(): Promise<void> {
		if (!windowStore.currentWindowId) return;

		try {
			const chatWindowId = await invoke<string>("create_chat_window", {
				sourceWindowId: windowStore.currentWindowId,
			});

			// Update mode to Detached
			await this.setMode("Detached");
		} catch (err) {
			console.error("Failed to detach chat:", err);
			throw err;
		}
	}

	/**
	 * Reattach chat from detached window
	 */
	async reattach(): Promise<void> {
		if (!this.detachedWindowId) return;

		try {
			// Close the detached window
			await windowStore.closeWindow(this.detachedWindowId);

			// Set mode back to Docked
			await this.setMode("Docked");
		} catch (err) {
			console.error("Failed to reattach chat:", err);
			throw err;
		}
	}

	/**
	 * Cleanup on destroy
	 */
	destroy(): void {
		this.eventUnlisteners.forEach((unlisten) => unlisten());
	}
}

export const chatStore = new ChatStore();
