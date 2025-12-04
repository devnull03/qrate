<script lang="ts">
	import { onMount } from "svelte";
	import { getCurrentWindow } from "@tauri-apps/api/window";
	import ChatSidebarShell from "$lib/components/chat/ChatSidebarShell.svelte";
	import { layoutStore } from "$lib/stores/layoutStore.svelte";
	import { windowStore } from "$lib/stores/windowStore.svelte";
	import { chatStore } from "$lib/stores/chatStore.svelte";
	import TitleBar from "$lib/components/TitleBar.svelte";

	onMount(async () => {
		// Initialize window ID
		const window = await getCurrentWindow();
		const windowId = window.label;

		// Load layout for this window
		await layoutStore.loadLayout(windowId);
		windowStore.currentWindowId = windowId;
	});
</script>

<div class="flex h-screen w-screen flex-col overflow-hidden">
	<TitleBar />
	<div class="min-h-0 flex-1 overflow-hidden">
		<ChatSidebarShell standalone={true} />
	</div>
</div>

