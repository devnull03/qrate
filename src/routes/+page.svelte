<script lang="ts">
	import RevoGrid from "$lib/grid/RevoGrid.svelte";
	import FilesGrid from "$lib/components/FilesGrid.svelte";
	import { Button } from "$lib/components/ui/button/index.js";
	import TableIcon from "@lucide/svelte/icons/table";
	import FolderIcon from "@lucide/svelte/icons/folder";
	import { qrateStore } from "$lib/stores/qrateStore.svelte";
</script>

<div class="flex h-full flex-col overflow-hidden">
	<!-- View Tabs -->
	<div
		class="flex items-center gap-1 border-b border-border bg-muted/30 px-4 py-1.5"
	>
		<Button
			variant={qrateStore.activeView === "spreadsheet"
				? "secondary"
				: "ghost"}
			size="sm"
			class="h-7 gap-1.5 px-3"
			onclick={() => (qrateStore.activeView = "spreadsheet")}
		>
			<TableIcon class="size-3.5" />
			<span>Spreadsheet</span>
		</Button>
		<Button
			variant={qrateStore.activeView === "files" ? "secondary" : "ghost"}
			size="sm"
			class="h-7 gap-1.5 px-3"
			onclick={() => (qrateStore.activeView = "files")}
		>
			<FolderIcon class="size-3.5" />
			<span>Files</span>
		</Button>
	</div>

	<!-- View Content -->
	<div class="min-h-0 flex-1 overflow-hidden">
		{#if qrateStore.activeView === "spreadsheet"}
			<RevoGrid />
		{:else}
			<FilesGrid />
		{/if}
	</div>
</div>
