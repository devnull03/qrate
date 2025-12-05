<script lang="ts">
	import type { Snippet } from "svelte";

	interface Props {
		children?: Snippet;
	}

	let { children }: Props = $props();
</script>

<div class="editor-area flex h-full min-h-0 flex-1 flex-col overflow-hidden">
	{#if children}
		{@render children()}
	{/if}
</div>
