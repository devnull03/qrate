<script lang="ts">
	import Resizer from "./Resizer.svelte";
	import { layoutStore } from "$lib/stores/layoutStore.svelte";

	interface Props {
		children?: any;
	}

	let { children }: Props = $props();

	let currentWidth = $state(250);

	const MIN_WIDTH = 150;
	const MAX_WIDTH = 600;

	$effect(() => {
		if (layoutStore.layout?.left_sidebar) {
			currentWidth = layoutStore.layout.left_sidebar.width;
		}
	});

	const handleResize = async (delta: number) => {
		const newWidth = Math.max(
			MIN_WIDTH,
			Math.min(MAX_WIDTH, currentWidth + delta),
		);
		if (newWidth !== currentWidth) {
			currentWidth = newWidth;
			await layoutStore.updateRegionSize("left_sidebar", newWidth);
		}
	};
</script>

{#if layoutStore.layout?.left_sidebar.visible}
	<aside
		class="left-sidebar flex h-full shrink-0 border-r border-border bg-muted/30"
		style="width: {currentWidth}px;"
		aria-label="Left sidebar"
	>
		<div class="flex min-w-0 flex-1 flex-col overflow-hidden">
			{#if children}
				{@render children()}
			{/if}
		</div>
		<Resizer direction="horizontal" onResize={handleResize} />
	</aside>
{/if}

<style>
	.left-sidebar {
		transition: width 0.05s ease-out;
	}
</style>
