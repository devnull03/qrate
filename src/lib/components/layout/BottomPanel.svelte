<script lang="ts">
	import Resizer from "./Resizer.svelte";
	import { layoutStore } from "$lib/stores/layoutStore.svelte";
	import { annotationsService } from "$lib/services/annotations";
	import AlertCircleIcon from "@lucide/svelte/icons/alert-circle";
	import MessageSquareIcon from "@lucide/svelte/icons/message-square";
	import CommentsPanel from "./panels/CommentsPanel.svelte";
	import ProblemsPanel from "./panels/ProblemsPanel.svelte";

	let currentHeight = $state(200);
	let activeTab = $state<"problems" | "comments">("comments");

	const MIN_HEIGHT = 100;
	const MAX_HEIGHT = 500;

	const commentsCount = $derived(
		annotationsService.byProvider("user-comment").length,
	);
	const problemsCount = $derived(
		annotationsService.byProvider("validation").length,
	);

	$effect(() => {
		if (layoutStore.layout?.bottom_panel) {
			currentHeight = layoutStore.layout.bottom_panel.height;
		}
	});

	const handleResize = async (delta: number) => {
		const newHeight = Math.max(
			MIN_HEIGHT,
			Math.min(MAX_HEIGHT, currentHeight - delta),
		);
		if (newHeight !== currentHeight) {
			currentHeight = newHeight;
			await layoutStore.updateRegionSize("bottom_panel", newHeight);
		}
	};
</script>

{#if layoutStore.layout?.bottom_panel.visible}
	<div
		class="bottom-panel flex shrink-0 flex-col border-t border-border bg-muted/30"
		style="height: {currentHeight}px;"
	>
		<Resizer direction="vertical" onResize={handleResize} />

		<div class="flex items-center gap-1 border-b border-border px-2">
			<button
				type="button"
				class="flex h-8 items-center gap-1.5 border-b-2 px-2 text-sm font-medium transition-colors"
				class:border-primary={activeTab === "comments"}
				class:text-foreground={activeTab === "comments"}
				class:border-transparent={activeTab !== "comments"}
				class:text-muted-foreground={activeTab !== "comments"}
				class:hover:text-foreground={activeTab !== "comments"}
				onclick={() => (activeTab = "comments")}
			>
				<MessageSquareIcon class="size-3.5" />
				<span>Comments</span>
				{#if commentsCount > 0}
					<span
						class="ml-0.5 rounded-full bg-muted px-1.5 text-xs tabular-nums"
					>
						{commentsCount}
					</span>
				{/if}
			</button>

			<button
				type="button"
				class="flex h-8 items-center gap-1.5 border-b-2 px-2 text-sm font-medium transition-colors"
				class:border-primary={activeTab === "problems"}
				class:text-foreground={activeTab === "problems"}
				class:border-transparent={activeTab !== "problems"}
				class:text-muted-foreground={activeTab !== "problems"}
				class:hover:text-foreground={activeTab !== "problems"}
				onclick={() => (activeTab = "problems")}
			>
				<AlertCircleIcon class="size-3.5" />
				<span>Problems</span>
				{#if problemsCount > 0}
					<span
						class="ml-0.5 rounded-full bg-destructive/10 px-1.5 text-xs tabular-nums text-destructive"
					>
						{problemsCount}
					</span>
				{/if}
			</button>
		</div>

		<div class="min-h-0 flex-1 overflow-auto">
			{#if activeTab === "comments"}
				<CommentsPanel />
			{:else if activeTab === "problems"}
				<ProblemsPanel />
			{/if}
		</div>
	</div>
{/if}

<style>
	.bottom-panel {
		transition: height 0.05s ease-out;
	}
</style>
