<script lang="ts">
	import { annotationsService } from "$lib/services/annotations";
	import { qrateStore } from "$lib/stores/qrateStore.svelte";
	import MessageSquareIcon from "@lucide/svelte/icons/message-square";
	import CheckCircleIcon from "@lucide/svelte/icons/check-circle";
	import CircleIcon from "@lucide/svelte/icons/circle";

	const comments = $derived(annotationsService.byProvider("user-comment"));

	function getLocationLabel(
		rowId: number | null,
		columnId: string | null,
	): string {
		if (rowId !== null && columnId !== null) {
			const column = qrateStore.columns.find((c) => c.id === columnId);
			const columnName = column?.name ?? columnId;
			return `Row ${rowId}, ${columnName}`;
		}
		if (rowId !== null) {
			return `Row ${rowId}`;
		}
		if (columnId !== null) {
			const column = qrateStore.columns.find((c) => c.id === columnId);
			return column?.name ?? columnId;
		}
		return "Unknown location";
	}

	function formatTimestamp(isoString: string): string {
		const date = new Date(isoString);
		const now = new Date();
		const diffMs = now.getTime() - date.getTime();
		const diffMins = Math.floor(diffMs / 60000);
		const diffHours = Math.floor(diffMs / 3600000);
		const diffDays = Math.floor(diffMs / 86400000);

		if (diffMins < 1) return "Just now";
		if (diffMins < 60) return `${diffMins}m ago`;
		if (diffHours < 24) return `${diffHours}h ago`;
		if (diffDays < 7) return `${diffDays}d ago`;

		return date.toLocaleDateString();
	}

	function navigateToComment(rowId: number | null, columnId: string | null) {
		if (rowId !== null) {
			qrateStore.selectRow(rowId);
		}
		if (columnId !== null) {
			qrateStore.selectColumn(columnId);
		}
	}
</script>

{#if comments.length === 0}
	<div class="flex h-full items-center justify-center p-4">
		<div class="text-center text-muted-foreground">
			<MessageSquareIcon class="mx-auto mb-2 size-8 opacity-50" />
			<p class="text-sm">No comments yet</p>
			<p class="text-xs opacity-75">
				Right-click a cell to add a comment
			</p>
		</div>
	</div>
{:else}
	<div class="flex flex-col divide-y divide-border">
		{#each comments as comment (comment.id)}
			<button
				type="button"
				class="flex w-full items-start gap-3 px-3 py-2 text-left transition-colors hover:bg-muted/50"
				onclick={() =>
					navigateToComment(comment.rowId, comment.columnId)}
			>
				<div class="mt-0.5 shrink-0">
					{#if comment.resolved}
						<CheckCircleIcon class="size-4 text-green-500" />
					{:else}
						<CircleIcon class="size-4 text-muted-foreground" />
					{/if}
				</div>
				<div class="min-w-0 flex-1">
					<div class="flex items-center gap-2">
						<span class="text-xs font-medium text-muted-foreground">
							{getLocationLabel(comment.rowId, comment.columnId)}
						</span>
						<span class="text-xs text-muted-foreground/60">
							{formatTimestamp(comment.createdAt)}
						</span>
					</div>
					<p
						class="mt-0.5 line-clamp-2 text-sm"
						class:line-through={comment.resolved}
						class:text-muted-foreground={comment.resolved}
					>
						{comment.message}
					</p>
				</div>
			</button>
		{/each}
	</div>
{/if}
