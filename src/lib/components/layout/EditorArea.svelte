<script lang="ts">
	import { layoutStore } from "$lib/stores/layoutStore.svelte";

	interface Props {
		children?: any;
	}

	let { children }: Props = $props();

	let activeView = $state("spreadsheet");

	$effect(() => {
		if (layoutStore.layout?.editor_area) {
			activeView = layoutStore.layout.editor_area.active_view;
		}
	});
</script>

<main
	class="editor-area flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden"
	aria-label="Editor area"
>
	{#if children}
		{@render children()}
	{:else}
		<div
			class="flex h-full w-full items-center justify-center text-muted-foreground"
		>
			Editor Area
		</div>
	{/if}
</main>
