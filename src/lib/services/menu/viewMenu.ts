import { menuService, shortcut, type TopLevelMenu } from "./index";
import { layoutStore } from "$lib/stores/layoutStore.svelte";
import { chatStore } from "$lib/stores/chatStore.svelte";
import { qrateStore } from "$lib/stores/qrateStore.svelte";

export const VIEW_COMMANDS = {
	TOGGLE_LEFT_SIDEBAR: "view.toggleLeftSidebar",
	TOGGLE_RIGHT_SIDEBAR: "view.toggleRightSidebar",
	TOGGLE_BOTTOM_PANEL: "view.toggleBottomPanel",
	TOGGLE_CHAT: "view.toggleChat",
	TOGGLE_DETAILS_PANEL: "view.toggleDetailsPanel",
} as const;

export const toggleLeftSidebar = () => layoutStore.toggleRegion("left_sidebar");
export const toggleRightSidebar = () =>
	layoutStore.toggleRegion("right_sidebar");
export const toggleBottomPanel = () => layoutStore.toggleRegion("bottom_panel");
export const toggleChat = () => chatStore.toggleVisible();
export const toggleDetailsPanel = () => qrateStore.toggleDetailsPanel();

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
					{
						id: VIEW_COMMANDS.TOGGLE_DETAILS_PANEL,
						label: "Toggle Details Panel",
						shortcut: shortcut("l", "ctrl"),
						action: toggleDetailsPanel,
					},
				],
			},
		],
	};

	menuService.registerMenu(viewMenu);
}
