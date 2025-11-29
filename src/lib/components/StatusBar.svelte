<script lang="ts">
	import { qrateStore } from "$lib/stores/qrateStore.svelte";
</script>

<footer
	class="flex h-6 shrink-0 items-center justify-between border-t border-border bg-muted px-3 text-xs"
>
	<div class="flex items-center gap-2">
		{#if qrateStore.isFileOpen}
			<span class="whitespace-nowrap">
				{qrateStore.totalRows.toLocaleString()} rows
			</span>
			<span class="text-muted-foreground/50">|</span>
			<span class="whitespace-nowrap">
				{qrateStore.columns.length} columns
			</span>
			{#if qrateStore.selectedRowId !== null}
				<span class="text-muted-foreground/50">|</span>
				<span class="whitespace-nowrap">
					Row {qrateStore.selectedRowId} selected
				</span>
			{/if}
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
