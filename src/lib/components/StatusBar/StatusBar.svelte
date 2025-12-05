<script lang="ts">
	import { qrateStore } from "$lib/stores/qrateStore.svelte";
	import { thumbnailService } from "$lib/services/thumbnails";
	import { annotationsService } from "$lib/services/annotations";
	import { layoutStore } from "$lib/stores/layoutStore.svelte";
	import { Button } from "$lib/components/ui/button/index.js";
	import LoaderCircleIcon from "@lucide/svelte/icons/loader-circle";
	import ArrowDownIcon from "@lucide/svelte/icons/arrow-down";
	import ImageIcon from "@lucide/svelte/icons/image";
	import XIcon from "@lucide/svelte/icons/x";
	import MessageSquareIcon from "@lucide/svelte/icons/message-square";

	interface Props {
		class?: string;
	}

	let { class: className = "" }: Props = $props();

	const handleLoadMore = () => {
		qrateStore.loadMoreRows(100);
	};

	const getColumnName = (columnId: string | null): string | null => {
		if (!columnId) return null;
		const col = qrateStore.columns.find((c) => c.id === columnId);
		return col?.name ?? columnId;
	};

	const formatSelection = () => {
		const range = qrateStore.selectedRange;
		if (range) {
			const rows = Math.abs(range.endRow - range.startRow) + 1;
			const cols = Math.abs(range.endCol - range.startCol) + 1;
			return `${rows}R × ${cols}C`;
		}
		return null;
	};

	let selectionText = $derived(formatSelection());

	let thumbnailProgress = $derived(thumbnailService.progress);
	let isProcessingThumbnails = $derived(thumbnailService.isProcessing);
	let thumbnailPercentage = $derived(
		thumbnailProgress.total === 0
			? 0
			: Math.round(
					(thumbnailProgress.processed / thumbnailProgress.total) *
						100,
				),
	);

	const handleCancelThumbnails = async () => {
		try {
			await thumbnailService.cancel();
		} catch (e) {
			console.error("Failed to cancel thumbnail processing:", e);
		}
	};

	const commentsCount = $derived(
		annotationsService.byProvider("user-comment").length,
	);

	const toggleCommentsPanel = async () => {
		if (!layoutStore.layout?.bottom_panel.visible) {
			await layoutStore.toggleRegion("bottom_panel");
		}
	};
</script>

<footer
	class="flex h-6 shrink-0 items-center justify-between border-t border-border bg-muted px-3 text-xs {className}"
	role="status"
>
	<div class="flex items-center gap-2">
		{#if qrateStore.isFileOpen}
			{#if qrateStore.activeView === "spreadsheet"}
				<span>{qrateStore.totalRows.toLocaleString()} rows</span>
				<span class="text-muted-foreground/50">|</span>
				<span>{qrateStore.columns.length} columns</span>
				{#if selectionText}
					<span class="text-muted-foreground/50">|</span>
					<span>{selectionText}</span>
				{:else}
					{#if qrateStore.selectedRowId !== null}
						<span class="text-muted-foreground/50">|</span>
						<span>Row {qrateStore.selectedRowId}</span>
					{/if}
					{#if qrateStore.selectedColumnId}
						<span class="text-muted-foreground/50">|</span>
						<span
							>Col {getColumnName(
								qrateStore.selectedColumnId,
							)}</span
						>
					{/if}
				{/if}
			{:else}
				<span>
					{qrateStore.filesGridFilteredCount} file{qrateStore.filesGridFilteredCount !==
					1
						? "s"
						: ""}
					{#if qrateStore.filesGridSearchQuery}
						matching "{qrateStore.filesGridSearchQuery}"
					{/if}
				</span>
				{#if qrateStore.filesGridTotalCount !== qrateStore.filesGridFilteredCount}
					<span class="text-muted-foreground/50">|</span>
					<span>{qrateStore.filesGridTotalCount} total</span>
				{/if}
			{/if}

			{#if commentsCount > 0}
				<span class="text-muted-foreground/50">|</span>
				<button
					type="button"
					class="flex items-center gap-1 rounded px-1 py-0.5 transition-colors hover:bg-background/50"
					onclick={toggleCommentsPanel}
					title="View comments"
				>
					<MessageSquareIcon class="size-3" />
					<span>{commentsCount}</span>
				</button>
			{/if}
		{:else}
			<span class="text-muted-foreground">No file open</span>
		{/if}
	</div>

	<div class="flex items-center gap-2">
		<!-- Thumbnail processing indicator -->
		{#if isProcessingThumbnails}
			<div
				class="flex items-center gap-1.5 rounded bg-background/50 px-2 py-0.5"
				title="Processing thumbnails..."
			>
				<ImageIcon class="size-3 text-muted-foreground" />
				<LoaderCircleIcon class="size-3 animate-spin text-primary" />
				<span class="text-muted-foreground">
					{thumbnailProgress.processed}/{thumbnailProgress.total}
				</span>
				<span class="text-muted-foreground/70">
					({thumbnailPercentage}%)
				</span>
				<button
					type="button"
					class="ml-0.5 rounded p-0.5 opacity-60 transition-opacity hover:bg-muted hover:opacity-100"
					onclick={handleCancelThumbnails}
					title="Cancel thumbnail processing"
				>
					<XIcon class="size-3" />
				</button>
			</div>
			<span class="text-muted-foreground/50">|</span>
		{/if}

		{#if qrateStore.isLoading}
			<span class="flex items-center gap-1">
				<LoaderCircleIcon class="size-3 animate-spin" />
				Loading...
			</span>
		{:else if qrateStore.error}
			<span class="text-destructive">{qrateStore.error}</span>
		{:else if qrateStore.isFileOpen && qrateStore.activeView === "spreadsheet"}
			{#if qrateStore.isLoadingMore}
				<span class="flex items-center gap-1 text-muted-foreground">
					<LoaderCircleIcon class="size-3 animate-spin" />
					Loading...
				</span>
			{:else if qrateStore.hasMoreRows}
				<span class="text-muted-foreground">
					{qrateStore.rows.length.toLocaleString()} of {qrateStore.totalRows.toLocaleString()}
					loaded
				</span>
				<Button
					variant="ghost"
					size="sm"
					class="h-4 gap-0.5 px-1.5 text-xs cursor-pointer"
					onclick={handleLoadMore}
				>
					Load More
					<ArrowDownIcon class="size-3" />
				</Button>
			{:else if qrateStore.rows.length > 0}
				<span class="text-muted-foreground">
					All {qrateStore.totalRows.toLocaleString()} loaded
				</span>
			{:else}
				<span class="text-muted-foreground">Ready</span>
			{/if}
		{:else}
			<span class="text-muted-foreground">Ready</span>
		{/if}
	</div>
</footer>
