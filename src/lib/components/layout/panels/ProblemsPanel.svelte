<script lang="ts">
	import { annotationsService } from "$lib/services/annotations";
	import AlertCircleIcon from "@lucide/svelte/icons/alert-circle";

	const problems = $derived(annotationsService.byProvider("validation"));
</script>

{#if problems.length === 0}
	<div class="flex h-full items-center justify-center p-4">
		<div class="text-center text-muted-foreground">
			<AlertCircleIcon class="mx-auto mb-2 size-8 opacity-50" />
			<p class="text-sm">No problems detected</p>
		</div>
	</div>
{:else}
	<div class="flex flex-col divide-y divide-border">
		{#each problems as problem (problem.id)}
			<div class="flex items-start gap-3 px-3 py-2">
				<div class="mt-0.5 shrink-0">
					{#if problem.severity === "error"}
						<AlertCircleIcon class="size-4 text-destructive" />
					{:else if problem.severity === "warning"}
						<AlertCircleIcon class="size-4 text-yellow-500" />
					{:else}
						<AlertCircleIcon class="size-4 text-blue-500" />
					{/if}
				</div>
				<div class="min-w-0 flex-1">
					<p class="text-sm">{problem.message}</p>
				</div>
			</div>
		{/each}
	</div>
{/if}
