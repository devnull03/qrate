import { menuService, shortcut, type TopLevelMenu } from "./index";
import { layoutStore } from "$lib/stores/layoutStore.svelte";
import { chatStore } from "$lib/stores/chatStore.svelte";

// View menu command IDs
export const VIEW_COMMANDS = {
	TOGGLE_LEFT_SIDEBAR: "view.toggleLeftSidebar",
	TOGGLE_RIGHT_SIDEBAR: "view.toggleRightSidebar",
	TOGGLE_BOTTOM_PANEL: "view.toggleBottomPanel",
	TOGGLE_CHAT: "view.toggleChat",
} as const;

/**
 * Toggle the left sidebar visibility
 */
export async function toggleLeftSidebar(): Promise<void> {
	await layoutStore.toggleRegion("left_sidebar");
}

/**
 * Toggle the right sidebar (chat) visibility
 */
export async function toggleRightSidebar(): Promise<void> {
	await layoutStore.toggleRegion("right_sidebar");
}

/**
 * Toggle the bottom panel visibility
 */
export async function toggleBottomPanel(): Promise<void> {
	await layoutStore.toggleRegion("bottom_panel");
}

/**
 * Toggle chat visibility
 */
export async function toggleChat(): Promise<void> {
	await chatStore.toggleVisible();
}

/**
 * Register the View menu with all toggle options
 */
export function registerViewMenu(): void {
	const viewMenu: TopLevelMenu = {
		id: "view",
		label: "View",
		order: 2,
		groups: [
			{
				id: "view.appearance",
				label: "Appearance",
				order: 1,
				items: [
					{
						id: VIEW_COMMANDS.TOGGLE_LEFT_SIDEBAR,
						label: "Toggle Left Sidebar",
						shortcut: shortcut("b", "ctrl"),
						action: toggleLeftSidebar,
					},
					{
						id: VIEW_COMMANDS.TOGGLE_RIGHT_SIDEBAR,
						label: "Toggle Right Sidebar",
						shortcut: shortcut("b", "ctrl", "alt"),
						action: toggleRightSidebar,
					},
					{
						id: VIEW_COMMANDS.TOGGLE_BOTTOM_PANEL,
						label: "Toggle Bottom Panel",
						shortcut: shortcut("`", "ctrl"),
						action: toggleBottomPanel,
					},
					{
						id: VIEW_COMMANDS.TOGGLE_CHAT,
						label: "Toggle Chat",
						shortcut: shortcut("j", "ctrl"),
						action: toggleChat,
					},
				],
			},
		],
	};

	menuService.registerMenu(viewMenu);
}
