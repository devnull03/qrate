<script lang="ts">
	import Resizer from "./Resizer.svelte";
	import { layoutStore } from "$lib/stores/layoutStore.svelte";
	import AlertCircleIcon from "@lucide/svelte/icons/alert-circle";

	interface Props {
		children?: any;
	}

	let { children }: Props = $props();

	let currentHeight = $state(300);

	$effect(() => {
		if (layoutStore.layout?.bottom_panel) {
			currentHeight = layoutStore.layout.bottom_panel.height;
		}
	});

	const handleResize = async (delta: number) => {
		const newHeight = Math.max(50, Math.min(600, currentHeight - delta));
		currentHeight = newHeight;
		await layoutStore.updateRegionSize("bottom_panel", newHeight);
	};

	const toggle = async () => {
		await layoutStore.toggleRegion("bottom_panel");
	};
</script>

{#if layoutStore.layout?.bottom_panel.visible}
	<div
		class="bottom-panel flex flex-col border-t border-border bg-muted/30"
		style="height: {currentHeight}px;"
	>
		<!-- Tab Bar -->
		<div class="flex items-center gap-1 border-b border-border px-2">
			<div class="flex h-8 items-center gap-1.5 px-2 text-sm font-medium">
				<AlertCircleIcon class="size-3.5" />
				<span>Problems</span>
			</div>
		</div>

		<!-- Panel Content -->
		<div class="min-h-0 flex-1 overflow-auto">
			{#if children}
				{@render children()}
			{:else}
				<div class="p-4 text-sm text-muted-foreground">
					No problems detected
				</div>
			{/if}
		</div>

		<Resizer direction="vertical" onResize={handleResize} />
	</div>
{/if}

<style>
	.bottom-panel {
		transition: height 0.2s cubic-bezier(0.4, 0, 0.2, 1);
	}
</style>
