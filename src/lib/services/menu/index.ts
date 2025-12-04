import { listen, type UnlistenFn } from "@tauri-apps/api/event";

// Types
export type ModifierKey = "ctrl" | "alt" | "shift" | "meta";

export interface Shortcut {
	key: string;
	modifiers: ModifierKey[];
}

export interface MenuItem {
	id: string;
	label: string;
	shortcut?: Shortcut;
	action: () => void | Promise<void>;
	enabled?: boolean | (() => boolean);
	visible?: boolean | (() => boolean);
}

export interface MenuGroup {
	id: string;
	label: string;
	items: MenuItem[];
	order?: number;
}

export interface TopLevelMenu {
	id: string;
	label: string;
	groups: MenuGroup[];
	order?: number;
}

// Shortcut helper
export function shortcut(key: string, ...modifiers: ModifierKey[]): Shortcut {
	return { key, modifiers };
}

// Format shortcut for display (e.g., "Ctrl+Alt+B")
export function formatShortcut(s: Shortcut): string {
	const parts: string[] = [];

	if (s.modifiers.includes("ctrl")) parts.push("Ctrl");
	if (s.modifiers.includes("alt")) parts.push("Alt");
	if (s.modifiers.includes("shift")) parts.push("Shift");
	if (s.modifiers.includes("meta")) parts.push("Meta");

	// Format special keys
	let keyDisplay = s.key;
	if (s.key === "`") keyDisplay = "`";
	else if (s.key === " ") keyDisplay = "Space";
	else if (s.key.length === 1) keyDisplay = s.key.toUpperCase();

	parts.push(keyDisplay);
	return parts.join("+");
}

// Check if a keyboard event matches a shortcut
function matchesShortcut(event: KeyboardEvent, shortcut: Shortcut): boolean {
	const key = event.key.toLowerCase();
	const shortcutKey = shortcut.key.toLowerCase();

	// Check key match
	if (key !== shortcutKey) return false;

	// Check modifiers
	const hasCtrl = event.ctrlKey || event.metaKey;
	const hasAlt = event.altKey;
	const hasShift = event.shiftKey;

	const needsCtrl =
		shortcut.modifiers.includes("ctrl") ||
		shortcut.modifiers.includes("meta");
	const needsAlt = shortcut.modifiers.includes("alt");
	const needsShift = shortcut.modifiers.includes("shift");

	return (
		hasCtrl === needsCtrl && hasAlt === needsAlt && hasShift === needsShift
	);
}

class MenuService {
	private menus: Map<string, TopLevelMenu> = new Map();
	private shortcuts: Map<string, MenuItem> = new Map();
	private keydownHandler: ((event: KeyboardEvent) => void) | null = null;
	private eventUnlisteners: UnlistenFn[] = [];
	private initialized = false;

	/**
	 * Initialize the menu service
	 */
	async init(): Promise<void> {
		if (this.initialized) return;

		// Set up global keyboard listener
		this.keydownHandler = (event: KeyboardEvent) => {
			this.handleKeyDown(event);
		};

		window.addEventListener("keydown", this.keydownHandler);
		this.initialized = true;
	}

	/**
	 * Register a top-level menu
	 */
	registerMenu(menu: TopLevelMenu): void {
		this.menus.set(menu.id, menu);

		// Register all shortcuts from this menu
		for (const group of menu.groups) {
			for (const item of group.items) {
				if (item.shortcut) {
					const shortcutKey = this.getShortcutKey(item.shortcut);
					this.shortcuts.set(shortcutKey, item);
				}
			}
		}
	}

	/**
	 * Unregister a top-level menu
	 */
	unregisterMenu(menuId: string): void {
		const menu = this.menus.get(menuId);
		if (!menu) return;

		// Unregister shortcuts
		for (const group of menu.groups) {
			for (const item of group.items) {
				if (item.shortcut) {
					const shortcutKey = this.getShortcutKey(item.shortcut);
					this.shortcuts.delete(shortcutKey);
				}
			}
		}

		this.menus.delete(menuId);
	}

	/**
	 * Register a menu group to an existing menu
	 */
	registerMenuGroup(menuId: string, group: MenuGroup): void {
		const menu = this.menus.get(menuId);
		if (!menu) {
			console.warn(`Menu ${menuId} not found`);
			return;
		}

		// Add or update group
		const existingIndex = menu.groups.findIndex((g) => g.id === group.id);
		if (existingIndex >= 0) {
			menu.groups[existingIndex] = group;
		} else {
			menu.groups.push(group);
			// Sort by order
			menu.groups.sort((a, b) => (a.order ?? 0) - (b.order ?? 0));
		}

		// Register shortcuts
		for (const item of group.items) {
			if (item.shortcut) {
				const shortcutKey = this.getShortcutKey(item.shortcut);
				this.shortcuts.set(shortcutKey, item);
			}
		}
	}

	/**
	 * Register a single menu item
	 */
	registerMenuItem(menuId: string, groupId: string, item: MenuItem): void {
		const menu = this.menus.get(menuId);
		if (!menu) {
			console.warn(`Menu ${menuId} not found`);
			return;
		}

		const group = menu.groups.find((g) => g.id === groupId);
		if (!group) {
			console.warn(`Group ${groupId} not found in menu ${menuId}`);
			return;
		}

		// Add or update item
		const existingIndex = group.items.findIndex((i) => i.id === item.id);
		if (existingIndex >= 0) {
			group.items[existingIndex] = item;
		} else {
			group.items.push(item);
		}

		// Register shortcut
		if (item.shortcut) {
			const shortcutKey = this.getShortcutKey(item.shortcut);
			this.shortcuts.set(shortcutKey, item);
		}
	}

	/**
	 * Register a global shortcut without a menu item
	 */
	registerShortcut(
		id: string,
		shortcutDef: Shortcut,
		action: () => void | Promise<void>,
	): void {
		const item: MenuItem = {
			id,
			label: id,
			shortcut: shortcutDef,
			action,
		};
		const shortcutKey = this.getShortcutKey(shortcutDef);
		this.shortcuts.set(shortcutKey, item);
	}

	/**
	 * Unregister a shortcut
	 */
	unregisterShortcut(shortcutDef: Shortcut): void {
		const shortcutKey = this.getShortcutKey(shortcutDef);
		this.shortcuts.delete(shortcutKey);
	}

	/**
	 * Get all registered menus
	 */
	getMenus(): TopLevelMenu[] {
		return Array.from(this.menus.values()).sort(
			(a, b) => (a.order ?? 0) - (b.order ?? 0),
		);
	}

	/**
	 * Get a specific menu
	 */
	getMenu(menuId: string): TopLevelMenu | undefined {
		return this.menus.get(menuId);
	}

	/**
	 * Execute a menu item action by ID
	 */
	async executeAction(itemId: string): Promise<void> {
		for (const menu of this.menus.values()) {
			for (const group of menu.groups) {
				const item = group.items.find((i) => i.id === itemId);
				if (item) {
					await this.executeItem(item);
					return;
				}
			}
		}
		console.warn(`Menu item ${itemId} not found`);
	}

	/**
	 * Handle keydown events
	 */
	private handleKeyDown(event: KeyboardEvent): void {
		// Don't handle if target is an input element (unless it's a specific shortcut)
		const target = event.target as HTMLElement;
		const isInput =
			target.tagName === "INPUT" ||
			target.tagName === "TEXTAREA" ||
			target.isContentEditable;

		// Check all registered shortcuts
		for (const [, item] of this.shortcuts) {
			if (item.shortcut && matchesShortcut(event, item.shortcut)) {
				// For most shortcuts, skip if in an input
				// But allow certain shortcuts like Escape
				if (isInput && item.shortcut.key !== "Escape") {
					continue;
				}

				event.preventDefault();
				event.stopPropagation();
				this.executeItem(item);
				return;
			}
		}
	}

	/**
	 * Execute a menu item
	 */
	private async executeItem(item: MenuItem): Promise<void> {
		// Check if enabled
		if (item.enabled !== undefined) {
			const isEnabled =
				typeof item.enabled === "function"
					? item.enabled()
					: item.enabled;
			if (!isEnabled) return;
		}

		// Check if visible
		if (item.visible !== undefined) {
			const isVisible =
				typeof item.visible === "function"
					? item.visible()
					: item.visible;
			if (!isVisible) return;
		}

		try {
			await item.action();
		} catch (err) {
			console.error(`Failed to execute menu action ${item.id}:`, err);
		}
	}

	/**
	 * Generate a unique key for a shortcut
	 */
	private getShortcutKey(s: Shortcut): string {
		const mods = [...s.modifiers].sort().join("+");
		return `${mods}+${s.key.toLowerCase()}`;
	}

	/**
	 * Cleanup
	 */
	destroy(): void {
		if (this.keydownHandler) {
			window.removeEventListener("keydown", this.keydownHandler);
			this.keydownHandler = null;
		}

		for (const unlisten of this.eventUnlisteners) {
			unlisten();
		}
		this.eventUnlisteners = [];

		this.menus.clear();
		this.shortcuts.clear();
		this.initialized = false;
	}
}

// Singleton instance
export const menuService = new MenuService();
