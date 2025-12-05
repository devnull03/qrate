<script lang="ts">
	import { annotationsService } from "$lib/services/annotations";
	import AlertCircleIcon from "@lucide/svelte/icons/alert-circle";
	import MessageSquareIcon from "@lucide/svelte/icons/message-square";
	import CommentsPanel from "./panels/CommentsPanel.svelte";
	import ProblemsPanel from "./panels/ProblemsPanel.svelte";

	let activeTab = $state<"problems" | "comments">("comments");

	const commentsCount = $derived(
		annotationsService.byProvider("user-comment").length,
	);
	const problemsCount = $derived(
		annotationsService.byProvider("validation").length,
	);
</script>

<div class="flex h-full flex-col bg-muted/30">
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
