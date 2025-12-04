<script lang="ts">
	import { onMount } from "svelte";
	import { getCurrentWindow } from "@tauri-apps/api/window";
	import TitleBar from "$lib/components/TitleBar.svelte";
	import StatusBar from "$lib/components/StatusBar";
	import LeftSidebar from "./LeftSidebar.svelte";
	import RightSidebar from "./RightSidebar.svelte";
	import BottomPanel from "./BottomPanel.svelte";
	import EditorArea from "./EditorArea.svelte";
	import { layoutStore } from "$lib/stores/layoutStore.svelte";
	import { windowStore } from "$lib/stores/windowStore.svelte";
	import { menuService } from "$lib/services/menu/index";
	import { registerViewMenu } from "$lib/services/menu/viewMenu";

	interface Props {
		children?: any;
	}

	let { children }: Props = $props();

	onMount(() => {
		// Initialize asynchronously
		const init = async () => {
			// Initialize window ID
			const window = await getCurrentWindow();
			const windowId = window.label;

			// Load layout for this window
			await layoutStore.loadLayout(windowId);
			windowStore.currentWindowId = windowId;

			// Initialize menu service and register menus
			await menuService.init();
			registerViewMenu();

			// Register additional shortcuts not in menus
			menuService.registerShortcut(
				"window.new",
				{ key: "n", modifiers: ["ctrl", "shift"] },
				async () => {
					await windowStore.createWindow(`window-${Date.now()}`);
				},
			);

			menuService.registerShortcut(
				"editor.escape",
				{ key: "Escape", modifiers: [] },
				() => {
					const editorArea = document.querySelector(".editor-area");
					if (editorArea instanceof HTMLElement) {
						editorArea.focus();
					}
				},
			);
		};

		init();

		return () => {
			menuService.destroy();
		};
	});
</script>

<div class="workbench flex h-screen w-screen flex-col overflow-hidden">
	<!-- Title Bar -->
	<TitleBar />

	<!-- Main Workbench Area -->
	<div class="workbench-main relative flex min-h-0 flex-1 overflow-hidden">
		<!-- Left Sidebar -->
		<LeftSidebar>
			<!-- Future: Additional left sidebar content -->
		</LeftSidebar>

		<!-- Editor Area -->
		<EditorArea>
			{@render children()}
		</EditorArea>

		<!-- Right Sidebar (Chat) -->
		<RightSidebar />
	</div>

	<!-- Bottom Panel (Problems) -->
	<BottomPanel />

	<!-- Status Bar -->
	<StatusBar />
</div>

<style>
	.workbench {
		display: flex;
		flex-direction: column;
		height: 100vh;
		width: 100vw;
		overflow: hidden;
	}

	.workbench-main {
		display: flex;
		flex: 1;
		min-height: 0;
		overflow: hidden;
	}
</style>
