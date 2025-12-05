<script lang="ts">
	import type { Snippet } from "svelte";

	interface Props {
		children?: Snippet;
	}

	let { children }: Props = $props();
</script>

<aside
	class="flex h-full flex-col overflow-hidden border-r border-border bg-muted/30"
	aria-label="Left sidebar"
>
	{#if children}
		{@render children()}
	{/if}
</aside>
