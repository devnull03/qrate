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

<footer class="status-bar">
	<div class="status-left">
		{#if qrateStore.isFileOpen && currentFileName}
			<span class="status-item" title={qrateStore.currentFilePath}>
				{currentFileName}
			</span>
			<span class="status-separator">|</span>
			<span class="status-item">
				{qrateStore.totalRows.toLocaleString()} rows
			</span>
			<span class="status-separator">|</span>
			<span class="status-item">
				{qrateStore.columns.length} columns
			</span>
		{:else}
			<span class="status-item text-muted-foreground">No file open</span>
		{/if}
	</div>
	<div class="status-right">
		{#if qrateStore.isLoading}
			<span class="status-item">Loading...</span>
		{:else if qrateStore.error}
			<span class="status-item text-destructive">{qrateStore.error}</span>
		{:else}
			<span class="status-item text-muted-foreground">Ready</span>
		{/if}
	</div>
</footer>

<style>
	.status-bar {
		display: flex;
		align-items: center;
		justify-content: space-between;
		height: 24px;
		padding: 0 0.75rem;
		font-size: 0.75rem;
		background-color: var(--muted);
		border-top: 1px solid var(--border);
		flex-shrink: 0;
	}

	.status-left,
	.status-right {
		display: flex;
		align-items: center;
		gap: 0.5rem;
	}

	.status-item {
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}

	.status-separator {
		color: var(--muted-foreground);
		opacity: 0.5;
	}
</style>
