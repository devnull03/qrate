<script lang="ts">
	import { qrateStore } from "$lib/stores/qrateStore.svelte";
	import { getFileName } from "$lib/utils/path";

	// Get current file name using utility
	let currentFileName = $derived(
		qrateStore.currentFilePath
			? getFileName(qrateStore.currentFilePath)
			: null,
	);
</script>

<footer
	class="flex h-6 shrink-0 items-center justify-between border-t border-border bg-muted px-3 text-xs"
>
	<div class="flex items-center gap-2">
		{#if qrateStore.isFileOpen && currentFileName}
			<span
				class="truncate whitespace-nowrap"
				title={qrateStore.currentFilePath}
			>
				{currentFileName}
			</span>
			<span class="text-muted-foreground/50">|</span>
			<span class="whitespace-nowrap">
				{qrateStore.totalRows.toLocaleString()} rows
			</span>
			<span class="text-muted-foreground/50">|</span>
			<span class="whitespace-nowrap">
				{qrateStore.columns.length} columns
			</span>
		{:else}
			<span class="text-muted-foreground">No file open</span>
		{/if}
	</div>
	<div class="flex items-center gap-2">
		{#if qrateStore.isLoading}
			<span>Loading...</span>
		{:else if qrateStore.error}
			<span class="text-destructive">{qrateStore.error}</span>
		{:else}
			<span class="text-muted-foreground">Ready</span>
		{/if}
	</div>
</footer>
