<script lang="ts">
	import type { Snippet } from "svelte";
	import ChatSidebarShell from "$lib/components/chat/ChatSidebarShell.svelte";

	interface Props {
		children?: Snippet;
	}

	let { children }: Props = $props();
</script>

<aside
	class="flex h-full flex-col overflow-hidden border-l border-border bg-muted/30"
	aria-label="Chat sidebar"
>
	{#if children}
		{@render children()}
	{:else}
		<ChatSidebarShell />
	{/if}
</aside>
