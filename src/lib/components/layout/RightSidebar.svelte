<script lang="ts">
	import Resizer from "./Resizer.svelte";
	import { layoutStore } from "$lib/stores/layoutStore.svelte";
	import ChatSidebarShell from "$lib/components/chat/ChatSidebarShell.svelte";

	let currentWidth = $state(360);

	$effect(() => {
		if (layoutStore.layout?.right_sidebar) {
			currentWidth = layoutStore.layout.right_sidebar.width;
		}
	});

	const handleResize = async (delta: number) => {
		const newWidth = Math.max(200, Math.min(800, currentWidth - delta));
		currentWidth = newWidth;
		await layoutStore.updateRegionSize("right_sidebar", newWidth);
	};
</script>

{#if layoutStore.layout?.right_sidebar.visible}
	<aside
		class="right-sidebar flex border-l border-border bg-muted/30"
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
		transition: width 0.2s cubic-bezier(0.4, 0, 0.2, 1);
	}
</style>
