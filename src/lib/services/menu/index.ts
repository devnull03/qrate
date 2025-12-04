import type { UnlistenFn } from "@tauri-apps/api/event";

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

export function shortcut(key: string, ...modifiers: ModifierKey[]): Shortcut {
	return { key, modifiers };
}

export function formatShortcut(s: Shortcut): string {
	const parts: string[] = [];
	if (s.modifiers.includes("ctrl")) parts.push("Ctrl");
	if (s.modifiers.includes("alt")) parts.push("Alt");
	if (s.modifiers.includes("shift")) parts.push("Shift");
	if (s.modifiers.includes("meta")) parts.push("Meta");

	const keyDisplay =
		s.key === "`"
			? "`"
			: s.key === " "
				? "Space"
				: s.key.length === 1
					? s.key.toUpperCase()
					: s.key;
	parts.push(keyDisplay);
	return parts.join("+");
}

function matchesShortcut(event: KeyboardEvent, shortcut: Shortcut): boolean {
	if (event.key.toLowerCase() !== shortcut.key.toLowerCase()) return false;

	const hasCtrl = event.ctrlKey || event.metaKey;
	const needsCtrl =
		shortcut.modifiers.includes("ctrl") ||
		shortcut.modifiers.includes("meta");

	return (
		hasCtrl === needsCtrl &&
		event.altKey === shortcut.modifiers.includes("alt") &&
		event.shiftKey === shortcut.modifiers.includes("shift")
	);
}

class MenuService {
	private menus = new Map<string, TopLevelMenu>();
	private shortcuts = new Map<string, MenuItem>();
	private keydownHandler: ((event: KeyboardEvent) => void) | null = null;
	private eventUnlisteners: UnlistenFn[] = [];
	private initialized = false;

	async init(): Promise<void> {
		if (this.initialized) return;
		this.keydownHandler = (event: KeyboardEvent) =>
			this.handleKeyDown(event);
		window.addEventListener("keydown", this.keydownHandler);
		this.initialized = true;
	}

	registerMenu(menu: TopLevelMenu): void {
		this.menus.set(menu.id, menu);
		menu.groups
			.flatMap((g) => g.items)
			.forEach((item) => {
				if (item.shortcut)
					this.shortcuts.set(
						this.getShortcutKey(item.shortcut),
						item,
					);
			});
	}

	unregisterMenu(menuId: string): void {
		const menu = this.menus.get(menuId);
		if (!menu) return;

		menu.groups
			.flatMap((g) => g.items)
			.forEach((item) => {
				if (item.shortcut)
					this.shortcuts.delete(this.getShortcutKey(item.shortcut));
			});
		this.menus.delete(menuId);
	}

	registerMenuGroup(menuId: string, group: MenuGroup): void {
		const menu = this.menus.get(menuId);
		if (!menu) return;

		const existingIndex = menu.groups.findIndex((g) => g.id === group.id);
		if (existingIndex >= 0) {
			menu.groups[existingIndex] = group;
		} else {
			menu.groups.push(group);
			menu.groups.sort((a, b) => (a.order ?? 0) - (b.order ?? 0));
		}

		group.items.forEach((item) => {
			if (item.shortcut)
				this.shortcuts.set(this.getShortcutKey(item.shortcut), item);
		});
	}

	registerMenuItem(menuId: string, groupId: string, item: MenuItem): void {
		const menu = this.menus.get(menuId);
		const group = menu?.groups.find((g) => g.id === groupId);
		if (!group) return;

		const existingIndex = group.items.findIndex((i) => i.id === item.id);
		if (existingIndex >= 0) {
			group.items[existingIndex] = item;
		} else {
			group.items.push(item);
		}

		if (item.shortcut)
			this.shortcuts.set(this.getShortcutKey(item.shortcut), item);
	}

	registerShortcut(
		id: string,
		shortcutDef: Shortcut,
		action: () => void | Promise<void>,
	): void {
		const item: MenuItem = { id, label: id, shortcut: shortcutDef, action };
		this.shortcuts.set(this.getShortcutKey(shortcutDef), item);
	}

	unregisterShortcut(shortcutDef: Shortcut): void {
		this.shortcuts.delete(this.getShortcutKey(shortcutDef));
	}

	getMenus(): TopLevelMenu[] {
		return Array.from(this.menus.values()).sort(
			(a, b) => (a.order ?? 0) - (b.order ?? 0),
		);
	}

	getMenu(menuId: string): TopLevelMenu | undefined {
		return this.menus.get(menuId);
	}

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
	}

	private handleKeyDown(event: KeyboardEvent): void {
		const target = event.target as HTMLElement;
		const isInput =
			target.tagName === "INPUT" ||
			target.tagName === "TEXTAREA" ||
			target.isContentEditable;

		for (const [, item] of this.shortcuts) {
			if (item.shortcut && matchesShortcut(event, item.shortcut)) {
				if (isInput && item.shortcut.key !== "Escape") continue;

				event.preventDefault();
				event.stopPropagation();
				this.executeItem(item);
				return;
			}
		}
	}

	private async executeItem(item: MenuItem): Promise<void> {
		const isEnabled =
			typeof item.enabled === "function"
				? item.enabled()
				: (item.enabled ?? true);
		const isVisible =
			typeof item.visible === "function"
				? item.visible()
				: (item.visible ?? true);
		if (!isEnabled || !isVisible) return;

		try {
			await item.action();
		} catch (err) {
			console.error(`Menu action ${item.id} failed:`, err);
		}
	}

	private getShortcutKey(s: Shortcut): string {
		return `${[...s.modifiers].sort().join("+")}+${s.key.toLowerCase()}`;
	}

	destroy(): void {
		if (this.keydownHandler) {
			window.removeEventListener("keydown", this.keydownHandler);
			this.keydownHandler = null;
		}
		this.eventUnlisteners.forEach((unlisten) => unlisten());
		this.eventUnlisteners = [];
		this.menus.clear();
		this.shortcuts.clear();
		this.initialized = false;
	}
}

export const menuService = new MenuService();
