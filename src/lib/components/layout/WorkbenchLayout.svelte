<script lang="ts">
	import { onMount } from "svelte";
	import { getCurrentWindow } from "@tauri-apps/api/window";
	import TitleBar from "$lib/components/TitleBar.svelte";
	import StatusBar from "$lib/components/StatusBar";
	import { layoutStore } from "$lib/stores/layoutStore.svelte";
	import { windowStore } from "$lib/stores/windowStore.svelte";
	import { menuService } from "$lib/services/menu/index";
	import { registerViewMenu } from "$lib/services/menu/viewMenu";
	import * as Resizable from "$lib/components/ui/resizable/index";
	import LeftSidebar from "./LeftSidebar.svelte";
	import RightSidebar from "./RightSidebar.svelte";
	import BottomPanel from "./BottomPanel.svelte";
	import EditorArea from "./EditorArea.svelte";

	interface Props {
		children?: any;
	}

	let { children }: Props = $props();

	let pendingLeftWidth: number | null = null;
	let pendingRightWidth: number | null = null;
	let pendingBottomHeight: number | null = null;

	const getLeftSidebarSize = () => {
		if (!layoutStore.layout?.left_sidebar.visible) return 0;
		return Math.max(
			10,
			Math.min(
				40,
				(layoutStore.layout.left_sidebar.width / window.innerWidth) *
					100,
			),
		);
	};

	const getRightSidebarSize = () => {
		if (!layoutStore.layout?.right_sidebar.visible) return 0;
		return Math.max(
			15,
			Math.min(
				50,
				(layoutStore.layout.right_sidebar.width / window.innerWidth) *
					100,
			),
		);
	};

	const getBottomPanelSize = () => {
		if (!layoutStore.layout?.bottom_panel.visible) return 0;
		const containerHeight = window.innerHeight - 80;
		return Math.max(
			10,
			Math.min(
				60,
				(layoutStore.layout.bottom_panel.height / containerHeight) *
					100,
			),
		);
	};

	const handleMainLayoutChange = (sizes: number[]) => {
		if (!layoutStore.layout) return;

		const containerWidth = window.innerWidth;

		if (
			layoutStore.layout.left_sidebar.visible &&
			layoutStore.layout.right_sidebar.visible
		) {
			pendingLeftWidth = Math.round((sizes[0] / 100) * containerWidth);
			pendingRightWidth = Math.round((sizes[2] / 100) * containerWidth);
		} else if (layoutStore.layout.left_sidebar.visible) {
			pendingLeftWidth = Math.round((sizes[0] / 100) * containerWidth);
		} else if (layoutStore.layout.right_sidebar.visible) {
			pendingRightWidth = Math.round((sizes[1] / 100) * containerWidth);
		}
	};

	const handleVerticalLayoutChange = (sizes: number[]) => {
		if (!layoutStore.layout?.bottom_panel.visible) return;

		const containerHeight = window.innerHeight - 80;
		pendingBottomHeight = Math.round((sizes[1] / 100) * containerHeight);
	};

	const handleHorizontalDragEnd = (isDragging: boolean) => {
		if (isDragging) return;

		if (
			pendingLeftWidth !== null &&
			pendingLeftWidth > 100 &&
			pendingLeftWidth < 600
		) {
			layoutStore.updateRegionSize("left_sidebar", pendingLeftWidth);
			pendingLeftWidth = null;
		}
		if (
			pendingRightWidth !== null &&
			pendingRightWidth > 200 &&
			pendingRightWidth < 800
		) {
			layoutStore.updateRegionSize("right_sidebar", pendingRightWidth);
			pendingRightWidth = null;
		}
	};

	const handleVerticalDragEnd = (isDragging: boolean) => {
		if (isDragging) return;

		if (
			pendingBottomHeight !== null &&
			pendingBottomHeight > 80 &&
			pendingBottomHeight < 500
		) {
			layoutStore.updateRegionSize("bottom_panel", pendingBottomHeight);
			pendingBottomHeight = null;
		}
	};

	onMount(() => {
		const init = async () => {
			const window = await getCurrentWindow();
			const windowId = window.label;

			await layoutStore.loadLayout(windowId);
			windowStore.currentWindowId = windowId;

			await menuService.init();
			registerViewMenu();

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
	<TitleBar />

	<Resizable.PaneGroup
		direction="horizontal"
		class="flex-1"
		onLayoutChange={handleMainLayoutChange}
	>
		{#if layoutStore.layout?.left_sidebar.visible}
			<Resizable.Pane
				defaultSize={getLeftSidebarSize()}
				minSize={10}
				maxSize={40}
				order={1}
			>
				<LeftSidebar />
			</Resizable.Pane>
			<Resizable.Handle
				class="w-px bg-border transition-colors hover:bg-primary/50"
				onDraggingChange={handleHorizontalDragEnd}
			/>
		{/if}

		<Resizable.Pane
			defaultSize={100 - getLeftSidebarSize() - getRightSidebarSize()}
			minSize={30}
			order={2}
		>
			<Resizable.PaneGroup
				direction="vertical"
				onLayoutChange={handleVerticalLayoutChange}
			>
				<Resizable.Pane
					defaultSize={100 - getBottomPanelSize()}
					minSize={20}
					order={1}
				>
					<EditorArea>
						{@render children?.()}
					</EditorArea>
				</Resizable.Pane>

				{#if layoutStore.layout?.bottom_panel.visible}
					<Resizable.Handle
						class="h-px bg-border transition-colors hover:bg-primary/50"
						onDraggingChange={handleVerticalDragEnd}
					/>
					<Resizable.Pane
						defaultSize={getBottomPanelSize()}
						minSize={10}
						maxSize={60}
						order={2}
					>
						<BottomPanel />
					</Resizable.Pane>
				{/if}
			</Resizable.PaneGroup>
		</Resizable.Pane>

		{#if layoutStore.layout?.right_sidebar.visible}
			<Resizable.Handle
				class="w-px bg-border transition-colors hover:bg-primary/50"
				onDraggingChange={handleHorizontalDragEnd}
			/>
			<Resizable.Pane
				defaultSize={getRightSidebarSize()}
				minSize={15}
				maxSize={50}
				order={3}
			>
				<RightSidebar />
			</Resizable.Pane>
		{/if}
	</Resizable.PaneGroup>

	<StatusBar />
</div>
