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
	import ChatSidebarShell from "$lib/components/chat/ChatSidebarShell.svelte";
	import CommentsPanel from "./panels/CommentsPanel.svelte";
	import ProblemsPanel from "./panels/ProblemsPanel.svelte";
	import { annotationsService } from "$lib/services/annotations";
	import AlertCircleIcon from "@lucide/svelte/icons/alert-circle";
	import MessageSquareIcon from "@lucide/svelte/icons/message-square";

	interface Props {
		children?: any;
	}

	let { children }: Props = $props();

	let activeTab = $state<"problems" | "comments">("comments");

	const commentsCount = $derived(
		annotationsService.byProvider("user-comment").length,
	);
	const problemsCount = $derived(
		annotationsService.byProvider("validation").length,
	);

	// Track current sizes during drag (don't persist until drag ends)
	let pendingLeftWidth: number | null = null;
	let pendingRightWidth: number | null = null;
	let pendingBottomHeight: number | null = null;

	// Calculate initial sizes as percentages
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

	// Handle layout changes - just track pending values
	const handleMainLayoutChange = (sizes: number[]) => {
		if (!layoutStore.layout) return;

		const containerWidth = window.innerWidth;

		if (
			layoutStore.layout.left_sidebar.visible &&
			layoutStore.layout.right_sidebar.visible
		) {
			// Both sidebars visible: [left, center, right]
			pendingLeftWidth = Math.round((sizes[0] / 100) * containerWidth);
			pendingRightWidth = Math.round((sizes[2] / 100) * containerWidth);
		} else if (layoutStore.layout.left_sidebar.visible) {
			// Only left sidebar: [left, center]
			pendingLeftWidth = Math.round((sizes[0] / 100) * containerWidth);
		} else if (layoutStore.layout.right_sidebar.visible) {
			// Only right sidebar: [center, right]
			pendingRightWidth = Math.round((sizes[1] / 100) * containerWidth);
		}
	};

	const handleVerticalLayoutChange = (sizes: number[]) => {
		if (!layoutStore.layout?.bottom_panel.visible) return;

		const containerHeight = window.innerHeight - 80;
		// Bottom panel is the second pane (index 1)
		pendingBottomHeight = Math.round((sizes[1] / 100) * containerHeight);
	};

	// Save on drag end
	const handleHorizontalDragEnd = (isDragging: boolean) => {
		if (isDragging) return; // Only save when drag ends

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
				<aside
					class="flex h-full flex-col overflow-hidden border-r border-border bg-muted/30"
					aria-label="Left sidebar"
				>
					<!-- Left sidebar content placeholder -->
				</aside>
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
					<div
						class="editor-area flex h-full min-h-0 flex-1 flex-col overflow-hidden"
					>
						{@render children?.()}
					</div>
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
						<div class="flex h-full flex-col bg-muted/30">
							<div
								class="flex items-center gap-1 border-b border-border px-2"
							>
								<button
									type="button"
									class="flex h-8 items-center gap-1.5 border-b-2 px-2 text-sm font-medium transition-colors"
									class:border-primary={activeTab ===
										"comments"}
									class:text-foreground={activeTab ===
										"comments"}
									class:border-transparent={activeTab !==
										"comments"}
									class:text-muted-foreground={activeTab !==
										"comments"}
									class:hover:text-foreground={activeTab !==
										"comments"}
									onclick={() => (activeTab = "comments")}
								>
									<MessageSquareIcon class="size-3.5" />
									<span>Comments</span>
									{#if commentsCount > 0}
										<span
											class="ml-0.5 rounded-full bg-muted px-1.5 text-xs tabular-nums"
										>
											{commentsCount}
										</span>
									{/if}
								</button>

								<button
									type="button"
									class="flex h-8 items-center gap-1.5 border-b-2 px-2 text-sm font-medium transition-colors"
									class:border-primary={activeTab ===
										"problems"}
									class:text-foreground={activeTab ===
										"problems"}
									class:border-transparent={activeTab !==
										"problems"}
									class:text-muted-foreground={activeTab !==
										"problems"}
									class:hover:text-foreground={activeTab !==
										"problems"}
									onclick={() => (activeTab = "problems")}
								>
									<AlertCircleIcon class="size-3.5" />
									<span>Problems</span>
									{#if problemsCount > 0}
										<span
											class="ml-0.5 rounded-full bg-destructive/10 px-1.5 text-xs tabular-nums text-destructive"
										>
											{problemsCount}
										</span>
									{/if}
								</button>
							</div>

							<div class="min-h-0 flex-1 overflow-auto">
								{#if activeTab === "comments"}
									<CommentsPanel />
								{:else if activeTab === "problems"}
									<ProblemsPanel />
								{/if}
							</div>
						</div>
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
				<aside
					class="flex h-full flex-col overflow-hidden border-l border-border bg-muted/30"
					aria-label="Chat sidebar"
				>
					<ChatSidebarShell />
				</aside>
			</Resizable.Pane>
		{/if}
	</Resizable.PaneGroup>

	<StatusBar />
</div>
