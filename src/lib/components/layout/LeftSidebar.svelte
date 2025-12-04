<script lang="ts">
	import Resizer from "./Resizer.svelte";
	import { layoutStore } from "$lib/stores/layoutStore.svelte";

	interface Props {
		children?: any;
	}

	let { children }: Props = $props();

	let containerElement: HTMLElement | null = $state(null);
	let currentWidth = $state(250);

	$effect(() => {
		if (layoutStore.layout?.left_sidebar) {
			currentWidth = layoutStore.layout.left_sidebar.width;
		}
	});

	const handleResize = async (delta: number) => {
		const newWidth = Math.max(100, Math.min(800, currentWidth + delta));
		currentWidth = newWidth;
		await layoutStore.updateRegionSize("left_sidebar", newWidth);
	};

	const toggle = async () => {
		await layoutStore.toggleRegion("left_sidebar");
	};
</script>

{#if layoutStore.layout?.left_sidebar.visible}
	<aside
		bind:this={containerElement}
		class="left-sidebar flex border-r border-border bg-muted/30"
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
		transition: width 0.2s cubic-bezier(0.4, 0, 0.2, 1);
	}
</style>
