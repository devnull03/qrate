<script lang="ts">
	import Resizer from "./Resizer.svelte";
	import { layoutStore } from "$lib/stores/layoutStore.svelte";
	import ChatSidebarShell from "$lib/components/chat/ChatSidebarShell.svelte";

	let currentWidth = $state(360);

	const MIN_WIDTH = 250;
	const MAX_WIDTH = 800;

	$effect(() => {
		if (layoutStore.layout?.right_sidebar) {
			currentWidth = layoutStore.layout.right_sidebar.width;
		}
	});

	const handleResize = async (delta: number) => {
		// For right sidebar, negative delta means dragging left (making it wider)
		const newWidth = Math.max(
			MIN_WIDTH,
			Math.min(MAX_WIDTH, currentWidth - delta),
		);
		if (newWidth !== currentWidth) {
			currentWidth = newWidth;
			await layoutStore.updateRegionSize("right_sidebar", newWidth);
		}
	};
</script>

{#if layoutStore.layout?.right_sidebar.visible}
	<aside
		class="right-sidebar flex h-full shrink-0 border-l border-border bg-muted/30"
		style="width: {currentWidth}px;"
		aria-label="Chat sidebar"
	>
		<Resizer direction="horizontal" onResize={handleResize} />
		<div class="flex min-w-0 flex-1 flex-col overflow-hidden">
			<ChatSidebarShell />
		</div>
	</aside>
{/if}

<style>
	.right-sidebar {
		transition: width 0.05s ease-out;
	}
</style>
