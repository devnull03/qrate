<script lang="ts">
	import ChatHeader from "./ChatHeader.svelte";
	import ChatPlaceholder from "./ChatPlaceholder.svelte";
	import { chatStore } from "$lib/stores/chatStore.svelte";

	interface Props {
		standalone?: boolean;
	}

	let { standalone = false }: Props = $props();

	let mode = $derived(chatStore.mode);
</script>

<div class="chat-sidebar flex h-full w-full flex-col overflow-hidden">
	{#if !standalone}
		<ChatHeader
			{mode}
			onDetach={async () => {
				await chatStore.detach();
			}}
			onClose={async () => {
				await chatStore.toggleVisible();
			}}
		/>
	{/if}

	<div class="chat-content min-h-0 flex-1 overflow-auto">
		<ChatPlaceholder />
		<!-- Future: actual chat UI will go here -->
	</div>
</div>

<style>
	.chat-sidebar {
		display: flex;
		flex-direction: column;
		height: 100%;
		width: 100%;
		overflow: hidden;
	}

	.chat-content {
		flex: 1;
		min-height: 0;
		overflow: auto;
	}
</style>

